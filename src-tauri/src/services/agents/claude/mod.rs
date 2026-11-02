//! The Claude Code adapter (PRD M2 steps 6 and 7, D27, D42, D43): spawns the CLI in its
//! own process group, speaks stream-json over stdin/stdout, answers approvals and
//! reports a classified outcome.
use std::{
    collections::HashMap,
    io,
    process::Stdio,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::ChildStdin,
    sync::{mpsc, watch},
    time::{sleep_until, timeout},
};

use super::{
    AgentAdapter, AgentEvent, Capabilities, Command, Decision, EventSink, Outcome, RunHandle,
    RunOutcome, RunRequest, StartError,
    profiles::{self, AgentProfile, ProbeResult, merged_env},
};
use crate::persistence::SessionMode;
use crate::platform::process::{GroupChild, spawn_group};
use crate::services::diagnostics::spawn::{RunError, read_capped, run_capped};
use classify::{Facts, classify};
use log::{LOG_LIMIT_BYTES, SessionLog};
use stream::{Item, Parser, ResultInfo};

mod classify;
mod log;
mod stream;
#[cfg(test)]
pub(crate) mod testkit;
#[cfg(test)]
mod tests;

/// First CLI version whose live protocol was verified by the M2 spike (D27); the verified
/// range is this version and later patches of the same minor (2.1.x).
const VERIFIED_MIN: &str = "2.1.316";
const DETECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a live run may take to print its system init before it counts as a handshake failure.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
const CANCEL_GRACE: Duration = Duration::from_secs(3);
/// After the final result the CLI should exit on its own; past this it is stopped.
const EXIT_AFTER_RESULT: Duration = Duration::from_secs(10);
const PLAN_DENY_MESSAGE: &str = "Plan captured by AI App Studio; do not make changes.";
const USER_DENY_MESSAGE: &str = "The user denied this action in AI App Studio.";

// ----------------------------------------------------------------- transport

/// How approvals reach the user: asked live over stdin, or not at all (D42).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Transport {
    Live,
    Fallback,
}

fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    s.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find_map(|tok| {
            let mut n = tok.split('.').map(|p| p.parse::<u32>().ok());
            Some((n.next()??, n.next()??, n.next()??))
        })
}

pub(crate) fn version_verified(version: &str) -> bool {
    match (parse_version(version), parse_version(VERIFIED_MIN)) {
        (Some((a, b, c)), Some((ma, mi, pa))) => a == ma && b == mi && c >= pa,
        _ => false,
    }
}

/// Live when the version is verified and `--help` lists the prompt tool; fallback when
/// only `--permission-prompts` exists; otherwise the CLI is incompatible. The second
/// value is the warning to show when falling back.
pub(crate) fn choose_transport(
    version: Option<&str>,
    help: &str,
) -> Result<(Transport, Option<String>), String> {
    let verified = version.is_some_and(version_verified);
    if verified && help.contains("--permission-prompt-tool") {
        return Ok((Transport::Live, None));
    }
    if help.contains("--permission-prompts") {
        let why = if verified {
            "this Claude CLI does not offer live approvals".to_owned()
        } else {
            format!(
                "Claude CLI {} is outside the verified range ({VERIFIED_MIN} or a later 2.1.x)",
                version.unwrap_or("of unknown version")
            )
        };
        return Ok((
            Transport::Fallback,
            Some(format!(
                "Running without live approvals ({why}). Actions that need approval are denied \
                 and listed so you can allow one and retry."
            )),
        ));
    }
    Err(
        "This Claude CLI offers neither live approvals nor --permission-prompts; \
         update Studio or the CLI."
            .into(),
    )
}

fn is_missing(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
    )
}

async fn detect(
    profile: &AgentProfile,
    env: &[(String, String)],
) -> Result<(Transport, Option<String>), StartError> {
    let exe = &profile.executable;
    let ask = async |flag: &str| match run_capped(exe, &[flag], env, DETECT_TIMEOUT).await {
        Ok(out) => Ok(Some(out)),
        Err(RunError::Spawn(e)) if is_missing(&e) => Err(StartError::NotInstalled(exe.clone())),
        Err(RunError::Spawn(e)) => Err(StartError::Io(e)),
        Err(RunError::TimedOut(_)) => Ok(None),
    };
    let version = ask("--version")
        .await?
        .filter(|o| o.status.success())
        .and_then(|o| o.stdout.lines().next().map(str::to_owned));
    let help = ask("--help")
        .await?
        .map(|o| format!("{}\n{}", o.stdout, o.stderr))
        .unwrap_or_default();
    choose_transport(version.as_deref(), &help).map_err(StartError::IncompatibleCli)
}

/// Spawn arguments (D27, D32, D42, D43). Never `acceptEdits`, never
/// `--dangerously-skip-permissions`; the prompt goes in on stdin, not here.
pub(crate) fn build_argv(
    profile: &AgentProfile,
    req: &RunRequest,
    transport: Transport,
) -> Vec<String> {
    let mut a: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--setting-sources",
        "project,local",
        "--strict-mcp-config",
    ]
    .map(String::from)
    .into();
    let mut flag = |name: &str, value: &str| a.extend([name.to_owned(), value.to_owned()]);
    if !profile.instructions.trim().is_empty() {
        flag("--append-system-prompt", &profile.instructions);
    }
    if let Some(m) = profile.model.as_deref().filter(|m| !m.is_empty()) {
        flag("--model", m);
    }
    if let Some(id) = &req.resume_session_id {
        flag("--resume", id);
    }
    match transport {
        Transport::Live => {
            flag("--permission-prompt-tool", "stdio");
            let mode = if req.mode == SessionMode::Plan {
                "plan"
            } else {
                "manual"
            };
            flag("--permission-mode", mode);
        }
        Transport::Fallback => {
            flag("--permission-prompts", "none");
            if req.mode == SessionMode::Plan {
                flag("--permission-mode", "plan");
            }
        }
    }
    if !req.allowed_tools.is_empty() {
        // Variadic, so it goes last.
        a.push("--allowedTools".into());
        a.extend(req.allowed_tools.iter().cloned());
    }
    a
}

// ----------------------------------------------------------------- run state

/// What handling one line or answer produced: events for the log and sink, and lines for
/// the CLI's stdin.
#[derive(Default)]
pub(crate) struct Step {
    pub events: Vec<AgentEvent>,
    pub stdin: Vec<String>,
}

/// Why the read loop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cause {
    Exited,
    Cancelled,
    TimedOut,
    NoHandshake,
    /// The result arrived but the process did not exit; stopped, outcome from the result.
    Lingering,
}

/// Pure state of one run: parsing, approvals, sequence numbers. No IO, so fixtures can
/// drive it directly.
pub(crate) struct RunState {
    parser: Parser,
    seq: u32,
    pending: HashMap<String, Value>,
    session_id: Option<String>,
    result: Option<ResultInfo>,
    init_seen: bool,
}

fn control_response(request_id: &str, body: &Value) -> String {
    json!({"type": "control_response", "response": {
        "subtype": "success", "request_id": request_id, "response": body}})
    .to_string()
}

impl RunState {
    pub(crate) fn new(project_dir: std::path::PathBuf) -> Self {
        Self {
            parser: Parser::new(project_dir),
            seq: 0,
            pending: HashMap::new(),
            session_id: None,
            result: None,
            init_seen: false,
        }
    }

    fn push(&mut self, step: &mut Step, mut e: AgentEvent) {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0));
        e.set_meta(self.seq, ms);
        self.seq += 1;
        step.events.push(e);
    }

    pub(crate) fn warning(&mut self, text: &str) -> Step {
        self.warning_with(text, None)
    }

    pub(crate) fn warning_with(&mut self, text: &str, details: Option<String>) -> Step {
        let mut step = Step::default();
        self.push(
            &mut step,
            AgentEvent::Warning {
                seq: 0,
                at_ms: 0,
                text: text.into(),
                details,
            },
        );
        step
    }

    pub(crate) fn on_line(&mut self, line: &str) -> Step {
        let mut step = Step::default();
        for item in self.parser.parse(line) {
            match item {
                Item::Event(e) => self.push(&mut step, e),
                Item::Init { session_id } => {
                    self.init_seen = true;
                    self.session_id = Some(session_id).filter(|s| !s.is_empty());
                }
                Item::Approval {
                    request_id,
                    tool,
                    input,
                    ..
                } if tool == "ExitPlanMode" => {
                    let plan = input
                        .get("plan")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    self.push(
                        &mut step,
                        AgentEvent::PlanReady {
                            seq: 0,
                            at_ms: 0,
                            plan: plan.into(),
                        },
                    );
                    let deny = json!({"behavior": "deny", "message": PLAN_DENY_MESSAGE});
                    step.stdin.push(control_response(&request_id, &deny));
                }
                Item::Approval {
                    request_id,
                    tool,
                    input,
                    description,
                } => {
                    self.pending.insert(request_id.clone(), input.clone());
                    let e = AgentEvent::ApprovalNeeded {
                        seq: 0,
                        at_ms: 0,
                        request_id,
                        tool,
                        input,
                        description,
                    };
                    self.push(&mut step, e);
                }
                Item::Result(r) => {
                    self.session_id = r.session_id.clone().or(self.session_id.take());
                    self.result = Some(r);
                }
            }
        }
        step
    }

    /// Writes the control_response for a pending request. Unknown ids only warn.
    pub(crate) fn answer(&mut self, request_id: &str, decision: &Decision) -> Step {
        let Some(input) = self.pending.remove(request_id) else {
            return self.warning(&format!("no pending approval {request_id}"));
        };
        let body = match decision {
            Decision::Allow => json!({"behavior": "allow", "updatedInput": input}),
            Decision::Deny(msg) => json!({
                "behavior": "deny",
                "message": msg.as_deref().unwrap_or(USER_DENY_MESSAGE)}),
        };
        let mut step = Step::default();
        step.stdin.push(control_response(request_id, &body));
        let e = AgentEvent::ApprovalAnswered {
            seq: 0,
            at_ms: 0,
            request_id: request_id.into(),
            allowed: *decision == Decision::Allow,
        };
        self.push(&mut step, e);
        step
    }

    fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    fn init_seen(&self) -> bool {
        self.init_seen
    }

    fn has_result(&self) -> bool {
        self.result.is_some()
    }

    /// Classifies the end of the run and emits its `error` and `completed` events.
    pub(crate) fn finish(
        &mut self,
        exit_code: Option<i32>,
        stderr: &str,
        cause: Cause,
    ) -> (Step, RunOutcome) {
        let (outcome, message) = classify(&Facts {
            cancelled: cause == Cause::Cancelled,
            timed_out: cause == Cause::TimedOut,
            handshake_failed: cause == Cause::NoHandshake,
            exit_code,
            result: self.result.as_ref(),
            stderr,
            ..Facts::default()
        });
        self.conclude(
            outcome,
            message,
            exit_code,
            cause == Cause::Exited || cause == Cause::Lingering,
        )
    }

    fn conclude(
        &mut self,
        outcome: Outcome,
        message: Option<String>,
        exit_code: Option<i32>,
        exited_normally: bool,
    ) -> (Step, RunOutcome) {
        let denials = self.result.take().map(|r| r.denials).unwrap_or_default();
        let mut step = Step::default();
        if let Some(message) = &message {
            let e = AgentEvent::Error {
                seq: 0,
                at_ms: 0,
                error_kind: outcome,
                message: message.clone(),
            };
            self.push(&mut step, e);
        }
        self.push(
            &mut step,
            AgentEvent::Completed {
                seq: 0,
                at_ms: 0,
                outcome,
                denials: denials.clone(),
            },
        );
        let run = RunOutcome {
            outcome,
            message,
            exit_code,
            provider_session_id: self.session_id.clone(),
            denials,
            exited_normally,
        };
        (step, run)
    }
}

// ------------------------------------------------------------------ the run

struct Driver {
    state: RunState,
    log: SessionLog,
    sink: EventSink,
    stdin: Option<ChildStdin>,
}

impl Driver {
    async fn apply(&mut self, step: Step) {
        for e in step.events {
            self.log.event(&e);
            (self.sink)(e);
        }
        for line in step.stdin {
            if let Some(stdin) = &mut self.stdin {
                // A closed pipe means the CLI is exiting; the read loop sees that next.
                let _ = stdin.write_all(format!("{line}\n").as_bytes()).await;
                let _ = stdin.flush().await;
            }
        }
    }
}

struct Limits {
    live: bool,
    handshake: Duration,
    inactivity: Duration,
}

/// The next time the run must be stopped, and why. Time spent waiting for the user's
/// approval never counts as inactivity (D34).
fn deadline(
    d: &Driver,
    lim: &Limits,
    started: Instant,
    last_activity: Instant,
    result_at: Option<Instant>,
) -> Option<(Instant, Cause)> {
    if let Some(at) = result_at {
        return Some((at + EXIT_AFTER_RESULT, Cause::Lingering));
    }
    let handshake =
        (lim.live && !d.state.init_seen()).then_some((started + lim.handshake, Cause::NoHandshake));
    let idle = (!d.state.has_pending() && !lim.inactivity.is_zero())
        .then_some((last_activity + lim.inactivity, Cause::TimedOut));
    [handshake, idle]
        .into_iter()
        .flatten()
        .min_by_key(|(at, _)| *at)
}

async fn drive(
    mut d: Driver,
    mut child: GroupChild,
    lim: Limits,
    prompt: String,
    warnings: Vec<(String, Option<String>)>,
    mut commands: mpsc::UnboundedReceiver<Command>,
    done: watch::Sender<Option<RunOutcome>>,
) {
    let stderr = tokio::spawn(read_capped(child.child.stderr.take()));
    let mut lines = BufReader::new(child.child.stdout.take().expect("stdout is piped")).lines();
    d.stdin = child.child.stdin.take();

    for (text, details) in warnings {
        let step = d.state.warning_with(&text, details);
        d.apply(step).await;
    }
    let msg = json!({"type": "user", "message": {"role": "user", "content": prompt},
        "parent_tool_use_id": null, "session_id": ""});
    d.apply(Step {
        events: Vec::new(),
        stdin: vec![msg.to_string()],
    })
    .await;

    let started = Instant::now();
    let (mut last_activity, mut result_at) = (started, None);
    let cause = loop {
        let due = deadline(&d, &lim, started, last_activity, result_at);
        let timer = async {
            match due {
                Some((at, _)) => sleep_until(at.into()).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            line = lines.next_line() => {
                let Ok(Some(line)) = line else { break Cause::Exited };
                last_activity = Instant::now();
                d.log.raw(&line);
                let step = d.state.on_line(&line);
                d.apply(step).await;
                if result_at.is_none() && d.state.has_result() {
                    result_at = Some(Instant::now());
                    d.stdin = None; // end of input lets the CLI exit
                }
            }
            cmd = commands.recv() => match cmd {
                Some(Command::Answer { request_id, decision }) => {
                    let step = d.state.answer(&request_id, &decision);
                    d.apply(step).await;
                    last_activity = Instant::now();
                }
                Some(Command::Cancel) | None => break Cause::Cancelled,
            },
            () = timer => break due.map_or(Cause::TimedOut, |(_, c)| c),
        }
    };

    d.stdin = None;
    let exit_code = match cause {
        Cause::Exited => {
            let status = timeout(Duration::from_secs(5), child.child.wait()).await;
            // Sweep stragglers (tool subprocesses) left in the group.
            let swept = child.kill_group().await;
            status
                .ok()
                .and_then(Result::ok)
                .or(swept.ok())
                .and_then(|s| s.code())
        }
        // The result already said how it went; only the lingering process is stopped.
        Cause::Lingering => {
            let _ = child.terminate_group(CANCEL_GRACE).await;
            Some(0)
        }
        _ => {
            let _ = child.terminate_group(CANCEL_GRACE).await;
            None
        }
    };
    let stderr = timeout(Duration::from_secs(1), stderr)
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
    let (step, outcome) = d.state.finish(exit_code, &stderr, cause);
    d.apply(step).await;
    let _ = done.send(Some(outcome));
}

// ------------------------------------------------------------------ adapter

#[derive(Debug, Clone)]
pub struct ClaudeAdapter {
    pub(crate) handshake: Duration,
    pub(crate) log_limit: u64,
}

impl Default for ClaudeAdapter {
    fn default() -> Self {
        Self {
            handshake: HANDSHAKE_TIMEOUT,
            log_limit: LOG_LIMIT_BYTES,
        }
    }
}

impl AgentAdapter for ClaudeAdapter {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            name: "claude",
            plan_mode: true,
            live_approvals: true,
            resume: true,
        }
    }

    async fn probe(&self, profile: &AgentProfile, spawn_env: &[(String, String)]) -> ProbeResult {
        profiles::probe(profile, spawn_env).await
    }

    async fn start(
        &self,
        profile: &AgentProfile,
        req: RunRequest,
        sink: EventSink,
    ) -> Result<RunHandle, StartError> {
        let env = merged_env(profile, &req.spawn_env);
        let (transport, warning) = detect(profile, &env).await?;
        let argv = build_argv(profile, &req, transport);
        let log = SessionLog::create(&req.log_path, self.log_limit)?;
        let child = spawn_group(
            &profile.executable,
            &argv,
            &env,
            Some(&req.project_dir),
            Stdio::piped(),
        )
        .await
        .map_err(|e| {
            if is_missing(&e) {
                StartError::NotInstalled(profile.executable.clone())
            } else {
                StartError::Io(e)
            }
        })?;

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (done_tx, done_rx) = watch::channel(None);
        let warnings: Vec<_> = req
            .pre_warnings
            .into_iter()
            .chain(warning.map(|w| (w, None)))
            .collect();
        let driver = Driver {
            state: RunState::new(req.project_dir),
            log,
            sink,
            stdin: None,
        };
        let limits = Limits {
            live: transport == Transport::Live,
            handshake: self.handshake,
            inactivity: Duration::from_secs(u64::from(profile.inactivity_secs)),
        };
        tokio::spawn(drive(
            driver, child, limits, req.prompt, warnings, cmd_rx, done_tx,
        ));
        Ok(RunHandle::new(cmd_tx, done_rx))
    }
}
