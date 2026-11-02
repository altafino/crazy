use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use tempfile::TempDir;

use super::testkit::*;
use super::*;
use crate::services::agents::{Decision, Denial, Outcome};

fn fixture_lines(name: &str) -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/claude")
        .join(format!("{name}.jsonl"));
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|v| v["dir"] == "out")
        .map(|v| v["msg"].to_string())
        .collect()
}

/// Feeds a recorded transcript through the run state like the read loop would.
fn replay(name: &str) -> (Vec<AgentEvent>, Vec<String>, RunOutcome) {
    let mut state = RunState::new("/project".into());
    let (mut events, mut stdin) = (Vec::new(), Vec::new());
    for line in fixture_lines(name) {
        let step = state.on_line(&line);
        events.extend(step.events);
        stdin.extend(step.stdin);
    }
    let (step, outcome) = state.finish(Some(0), "", Cause::Exited);
    events.extend(step.events);
    (events, stdin, outcome)
}

#[test]
fn every_fixture_parses_to_the_expected_events() {
    let table: &[(&str, &[&str])] = &[
        (
            "fallback_accept_edits_bash",
            &[
                "status",
                "toolCall",
                "toolResult",
                "text",
                "usage",
                "completed",
            ],
        ),
        (
            "fallback_accept_edits_write",
            &[
                "status",
                "toolCall",
                "fileChange",
                "toolResult",
                "text",
                "usage",
                "completed",
            ],
        ),
        (
            "host_bash_allow",
            &[
                "status",
                "toolCall",
                "approvalNeeded",
                "toolResult",
                "text",
                "usage",
                "completed",
            ],
        ),
        (
            "host_bash_deny",
            &[
                "status",
                "toolCall",
                "approvalNeeded",
                "toolResult",
                "text",
                "usage",
                "completed",
            ],
        ),
        (
            "host_edit_allow",
            &[
                "status",
                "toolCall",
                "approvalNeeded",
                "toolResult",
                "text",
                "usage",
                "completed",
            ],
        ),
        (
            "host_plan",
            &[
                "status",
                "toolCall",
                "toolResult",
                "toolCall",
                "fileChange",
                "toolResult",
                "toolCall",
                "planReady",
                "toolResult",
                "text",
                "usage",
                "completed",
            ],
        ),
        (
            "no_host_write_denied",
            &[
                "status",
                "toolCall",
                "toolResult",
                "toolCall",
                "fileChange",
                "toolResult",
                "text",
                "usage",
                "completed",
            ],
        ),
    ];
    for (name, want) in table {
        let (events, _, outcome) = replay(name);
        assert_eq!(kinds(&events), *want, "{name}");
        let seqs: Vec<u32> = events.iter().map(AgentEvent::seq).collect();
        assert_eq!(
            seqs,
            (0..events.len() as u32).collect::<Vec<_>>(),
            "{name}: seq"
        );
        assert_eq!(outcome.outcome, Outcome::Success, "{name}");
        assert!(outcome.provider_session_id.is_some(), "{name}");
    }
}

#[test]
fn denied_bash_is_an_error_result_with_a_retry_rule() {
    let (events, _, outcome) = replay("host_bash_deny");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::ToolResult { is_error: true, .. }))
    );
    let [Denial { tool, rule, .. }] = &outcome.denials[..] else {
        panic!("{:?}", outcome.denials)
    };
    assert_eq!(tool, "Bash");
    assert_eq!(
        rule.as_deref(),
        Some("Bash(touch denied.txt && ls -l denied.txt)")
    );
    let Some(AgentEvent::Completed { denials, .. }) = events.last() else {
        panic!()
    };
    assert_eq!(denials, &outcome.denials);
}

#[test]
fn plan_request_is_captured_and_denied_without_asking() {
    let (events, stdin, _) = replay("host_plan");
    let plan = events.iter().find_map(|e| match e {
        AgentEvent::PlanReady { plan, .. } => Some(plan),
        _ => None,
    });
    assert!(plan.unwrap().starts_with("# Plan: add README.md"));
    assert!(!kinds(&events).contains(&"approvalNeeded".to_owned()));
    let [reply] = &stdin[..] else {
        panic!("{stdin:?}")
    };
    let v: Value = serde_json::from_str(reply).unwrap();
    assert_eq!(v["response"]["response"]["behavior"], "deny");
    assert_eq!(v["response"]["response"]["message"], PLAN_DENY_MESSAGE);
    assert_eq!(
        v["response"]["request_id"],
        "2a48e465-36e5-41ec-a993-1fcb6ee61acf"
    );
}

#[test]
fn answers_are_written_as_the_fixture_shapes() {
    let mut state = RunState::new("/project".into());
    for line in fixture_lines("host_bash_allow") {
        state.on_line(&line);
    }
    let step = state.answer("65fc02df-a89a-4352-8481-ce6b3a8fe09d", &Decision::Allow);
    let sent: Value = serde_json::from_str(&step.stdin[0]).unwrap();
    let recorded: Value = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude/host_bash_allow.jsonl"),
    )
    .unwrap()
    .lines()
    .map(|l| serde_json::from_str::<Value>(l).unwrap())
    .find(|v| v["msg"]["type"] == "control_response")
    .unwrap()["msg"]
        .clone();
    assert_eq!(sent, recorded);

    let mut state = RunState::new("/project".into());
    for line in fixture_lines("host_bash_deny") {
        state.on_line(&line);
    }
    let step = state.answer(
        "0fb62b21-76cb-45aa-847b-b4eb004f9016",
        &Decision::Deny(None),
    );
    let sent: Value = serde_json::from_str(&step.stdin[0]).unwrap();
    assert_eq!(sent["response"]["response"]["message"], USER_DENY_MESSAGE);
    assert!(matches!(
        step.events[0],
        AgentEvent::ApprovalAnswered { allowed: false, .. }
    ));
    // Answering twice, or an unknown id, only warns.
    let again = state.answer("0fb62b21-76cb-45aa-847b-b4eb004f9016", &Decision::Allow);
    assert!(again.stdin.is_empty() && matches!(again.events[0], AgentEvent::Warning { .. }));
}

// ----------------------------------------------------------------- argv

fn request(mode: SessionMode) -> RunRequest {
    RunRequest {
        project_dir: "/project".into(),
        prompt: "SECRET PROMPT TEXT".into(),
        mode,
        resume_session_id: None,
        allowed_tools: Vec::new(),
        spawn_env: Vec::new(),
        log_path: "/tmp/unused.jsonl".into(),
    }
}

fn profile() -> AgentProfile {
    fake_profile(Path::new("claude"), "x", &[])
}

fn has_pair(argv: &[String], flag: &str, value: &str) -> bool {
    argv.windows(2).any(|w| w[0] == flag && w[1] == value)
}

#[test]
fn argv_for_every_mode_and_transport() {
    let live_plan = build_argv(&profile(), &request(SessionMode::Plan), Transport::Live);
    assert!(has_pair(&live_plan, "--permission-prompt-tool", "stdio"));
    assert!(has_pair(&live_plan, "--permission-mode", "plan"));
    assert!(has_pair(&live_plan, "--setting-sources", "project,local"));
    assert!(live_plan.contains(&"--strict-mcp-config".to_owned()));
    assert!(has_pair(
        &live_plan,
        "--append-system-prompt",
        DEFAULT_INSTRUCTIONS_FOR_TEST
    ));

    let live_changes = build_argv(&profile(), &request(SessionMode::Changes), Transport::Live);
    assert!(has_pair(&live_changes, "--permission-mode", "manual"));

    let fb_plan = build_argv(&profile(), &request(SessionMode::Plan), Transport::Fallback);
    assert!(has_pair(&fb_plan, "--permission-prompts", "none"));
    assert!(has_pair(&fb_plan, "--permission-mode", "plan"));
    assert!(!fb_plan.contains(&"--permission-prompt-tool".to_owned()));

    let fb_changes = build_argv(
        &profile(),
        &request(SessionMode::Changes),
        Transport::Fallback,
    );
    assert!(has_pair(&fb_changes, "--permission-prompts", "none"));
    assert!(
        !fb_changes.contains(&"--permission-mode".to_owned()),
        "CLI default mode"
    );
}

const DEFAULT_INSTRUCTIONS_FOR_TEST: &str = crate::services::agents::profiles::DEFAULT_INSTRUCTIONS;

#[test]
fn argv_adds_model_resume_and_allowed_tools_last() {
    let mut p = profile();
    p.model = Some("opus".into());
    let mut r = request(SessionMode::Changes);
    r.resume_session_id = Some("sess-9".into());
    r.allowed_tools = vec!["Bash(touch a.txt)".into(), "Write(/p/b.txt)".into()];
    let argv = build_argv(&p, &r, Transport::Live);
    assert!(has_pair(&argv, "--model", "opus"));
    assert!(has_pair(&argv, "--resume", "sess-9"));
    let at = argv.iter().position(|a| a == "--allowedTools").unwrap();
    assert_eq!(&argv[at + 1..], ["Bash(touch a.txt)", "Write(/p/b.txt)"]);
}

#[test]
fn argv_never_bypasses_permissions_and_never_carries_the_prompt() {
    for mode in [SessionMode::Plan, SessionMode::Changes] {
        for t in [Transport::Live, Transport::Fallback] {
            let argv = build_argv(&profile(), &request(mode), t);
            let joined = argv.join(" ");
            assert!(!joined.contains("acceptEdits"), "{joined}");
            assert!(!joined.contains("dangerously-skip-permissions"), "{joined}");
            assert!(!joined.contains("bypassPermissions"), "{joined}");
            assert!(!joined.contains("SECRET PROMPT TEXT"), "{joined}");
        }
    }
}

// ------------------------------------------------------------- transport

#[test]
fn transport_table() {
    let live_help = "--permission-prompt-tool <t>\n--permission-prompts <m>";
    let fb_help = "--permission-prompts <m>";
    let cases: &[(Option<&str>, &str, Option<Transport>)] = &[
        (
            Some("2.1.316 (Claude Code)"),
            live_help,
            Some(Transport::Live),
        ),
        (Some("2.1.400"), live_help, Some(Transport::Live)),
        (Some("2.1.315"), live_help, Some(Transport::Fallback)),
        (Some("2.2.0"), live_help, Some(Transport::Fallback)),
        (Some("3.0.0"), live_help, Some(Transport::Fallback)),
        (None, live_help, Some(Transport::Fallback)),
        (Some("2.1.316"), fb_help, Some(Transport::Fallback)),
        (Some("2.1.316"), "--verbose", None),
        (Some("1.0.0"), "--permission-prompt-tool", None),
    ];
    for (version, help, want) in cases {
        let got = choose_transport(*version, help);
        assert_eq!(got.as_ref().ok().map(|(t, _)| *t), *want, "{version:?}");
        if let Ok((t, warning)) = got {
            assert_eq!(warning.is_some(), t == Transport::Fallback);
        }
    }
}

// ------------------------------------------------------- through the fake

struct Fx {
    dir: TempDir,
    exe: PathBuf,
    project: PathBuf,
}

fn fx() -> Fx {
    let dir = TempDir::new().unwrap();
    let exe = install_fake(dir.path());
    let project = dir.path().join("project");
    fs::create_dir(&project).unwrap();
    Fx { dir, exe, project }
}

impl Fx {
    fn request(&self, mode: SessionMode) -> RunRequest {
        RunRequest {
            project_dir: self.project.clone(),
            prompt: "do it".into(),
            mode,
            resume_session_id: None,
            allowed_tools: Vec::new(),
            spawn_env: spawn_env(),
            log_path: self.dir.path().join("logs/s1.jsonl"),
        }
    }

    fn file(&self, name: &str) -> String {
        self.dir.path().join(name).display().to_string()
    }
}

async fn run_to_end(
    fx: &Fx,
    scenario: &str,
    env: &[(&str, &str)],
) -> (RunOutcome, Vec<AgentEvent>) {
    let (sink, events) = collecting_sink();
    let profile = fake_profile(&fx.exe, scenario, env);
    let handle = ClaudeAdapter::default()
        .start(&profile, fx.request(SessionMode::Changes), sink)
        .await
        .unwrap();
    let outcome = handle.wait().await;
    let events = events.lock().unwrap().clone();
    (outcome, events)
}

#[tokio::test]
async fn full_run_logs_normalized_events() {
    let fx = fx();
    let (outcome, events) = run_to_end(&fx, "write", &[]).await;
    assert_eq!(outcome.outcome, Outcome::Success);
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(outcome.provider_session_id.as_deref(), Some("sess-1"));
    assert!(outcome.exited_normally);
    assert_eq!(
        kinds(&events),
        [
            "status",
            "toolCall",
            "fileChange",
            "toolResult",
            "usage",
            "completed"
        ]
    );
    assert!(matches!(&events[2], AgentEvent::FileChange { path, .. } if path == "out.txt"));
    assert_eq!(
        fs::read_to_string(fx.project.join("out.txt")).unwrap(),
        "hello\n"
    );

    let log = fs::read_to_string(fx.dir.path().join("logs/s1.jsonl")).unwrap();
    assert_eq!(log.lines().count(), events.len());
    let raw = fs::read_to_string(fx.dir.path().join("logs/s1.raw.jsonl")).unwrap();
    assert!(raw.lines().next().unwrap().contains("\"init\""));
}

#[tokio::test]
async fn prompt_goes_to_stdin_and_argv_matches_live_changes() {
    let fx = fx();
    let (stdin_log, argv_log) = (fx.file("stdin.txt"), fx.file("argv.txt"));
    run_to_end(
        &fx,
        "write",
        &[("FAKE_STDIN", &stdin_log), ("FAKE_ARGV", &argv_log)],
    )
    .await;
    let first: Value = serde_json::from_str(
        fs::read_to_string(stdin_log)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(first["message"]["content"], "do it");
    let argv = fs::read_to_string(argv_log).unwrap();
    assert!(argv.contains("--permission-prompt-tool\nstdio\n--permission-mode\nmanual"));
    assert!(!argv.contains("do it"));
}

#[tokio::test]
async fn approval_allow_round_trip() {
    let fx = fx();
    let stdin_log = fx.file("stdin.txt");
    let (sink, events) = collecting_sink();
    let profile = fake_profile(&fx.exe, "approve", &[("FAKE_STDIN", &stdin_log)]);
    let handle = ClaudeAdapter::default()
        .start(&profile, fx.request(SessionMode::Changes), sink)
        .await
        .unwrap();
    wait_for_event(&events, |e| matches!(e, AgentEvent::ApprovalNeeded { .. })).await;
    handle.answer_approval("req-1", Decision::Allow).unwrap();
    let outcome = handle.wait().await;
    assert_eq!(outcome.outcome, Outcome::Success);
    assert!(fx.project.join("approved.txt").exists());
    let events = events.lock().unwrap().clone();
    assert_eq!(
        kinds(&events),
        [
            "status",
            "toolCall",
            "approvalNeeded",
            "approvalAnswered",
            "toolResult",
            "usage",
            "completed"
        ]
    );
    let reply: Value = serde_json::from_str(
        fs::read_to_string(stdin_log)
            .unwrap()
            .lines()
            .nth(1)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(reply["response"]["response"]["behavior"], "allow");
    assert_eq!(
        reply["response"]["response"]["updatedInput"]["command"],
        "touch approved.txt"
    );
}

#[tokio::test]
async fn approval_deny_round_trip_reports_a_retry_rule() {
    let fx = fx();
    let (sink, events) = collecting_sink();
    let profile = fake_profile(&fx.exe, "approve", &[]);
    let handle = ClaudeAdapter::default()
        .start(&profile, fx.request(SessionMode::Changes), sink)
        .await
        .unwrap();
    wait_for_event(&events, |e| matches!(e, AgentEvent::ApprovalNeeded { .. })).await;
    handle
        .answer_approval("req-1", Decision::Deny(None))
        .unwrap();
    let outcome = handle.wait().await;
    assert!(!fx.project.join("approved.txt").exists());
    assert_eq!(
        outcome.denials[0].rule.as_deref(),
        Some("Bash(touch approved.txt)")
    );
    let events = events.lock().unwrap().clone();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::ApprovalAnswered { allowed: false, .. }))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::ToolResult { is_error: true, .. }))
    );
}

#[tokio::test]
async fn plan_run_captures_the_plan_and_denies_exit_plan_mode() {
    let fx = fx();
    let stdin_log = fx.file("stdin.txt");
    let (sink, events) = collecting_sink();
    let profile = fake_profile(&fx.exe, "plan", &[("FAKE_STDIN", &stdin_log)]);
    let handle = ClaudeAdapter::default()
        .start(&profile, fx.request(SessionMode::Plan), sink)
        .await
        .unwrap();
    assert_eq!(handle.wait().await.outcome, Outcome::Success);
    let events = events.lock().unwrap().clone();
    assert_eq!(
        kinds(&events),
        ["status", "planReady", "usage", "completed"]
    );
    assert!(matches!(&events[1], AgentEvent::PlanReady { plan, .. } if plan == "# Plan\n1. do it"));
    assert!(
        fs::read_to_string(stdin_log)
            .unwrap()
            .contains(PLAN_DENY_MESSAGE)
    );
}

#[tokio::test]
async fn old_cli_without_the_prompt_tool_runs_in_fallback_with_a_warning() {
    let fx = fx();
    let argv_log = fx.file("argv.txt");
    let (outcome, events) = run_to_end(
        &fx,
        "write",
        &[("FAKE_HELP", "fallback"), ("FAKE_ARGV", &argv_log)],
    )
    .await;
    assert_eq!(outcome.outcome, Outcome::Success);
    assert!(
        matches!(&events[0], AgentEvent::Warning { text, .. } if text.contains("without live approvals"))
    );
    let argv = fs::read_to_string(argv_log).unwrap();
    assert!(argv.contains("--permission-prompts\nnone"));
    assert!(!argv.contains("--permission-prompt-tool"));
}

#[tokio::test]
async fn unverified_version_falls_back_too() {
    let fx = fx();
    let (_, events) = run_to_end(&fx, "write", &[("FAKE_VERSION", "2.0.9")]).await;
    assert!(matches!(&events[0], AgentEvent::Warning { text, .. } if text.contains("2.0.9")));
}

#[tokio::test]
async fn blocked_when_neither_flag_exists() {
    let fx = fx();
    let (sink, _) = collecting_sink();
    let profile = fake_profile(&fx.exe, "write", &[("FAKE_HELP", "none")]);
    let err = ClaudeAdapter::default()
        .start(&profile, fx.request(SessionMode::Changes), sink)
        .await
        .err()
        .unwrap();
    assert!(matches!(err, StartError::IncompatibleCli(_)), "{err}");
    assert_eq!(err.outcome(), Outcome::IncompatibleCli);
}

#[tokio::test]
async fn missing_executable_is_not_installed() {
    let fx = fx();
    let (sink, _) = collecting_sink();
    let profile = fake_profile(&fx.dir.path().join("nope"), "write", &[]);
    let err = ClaudeAdapter::default()
        .start(&profile, fx.request(SessionMode::Changes), sink)
        .await
        .err()
        .unwrap();
    assert_eq!(err.outcome(), Outcome::NotInstalled);
}

#[tokio::test]
async fn failures_are_classified() {
    let fx = fx();
    let (outcome, events) = run_to_end(&fx, "fail", &[]).await;
    assert_eq!(outcome.outcome, Outcome::Failed);
    assert_eq!(outcome.exit_code, Some(3));
    assert!(outcome.message.unwrap().contains("boom"));
    assert_eq!(kinds(&events), ["error", "completed"]);

    let (outcome, _) = run_to_end(&fx, "auth", &[]).await;
    assert_eq!(outcome.outcome, Outcome::Unauthenticated);
}

#[tokio::test]
async fn silent_live_cli_fails_the_handshake() {
    let fx = fx();
    let (sink, _) = collecting_sink();
    let adapter = ClaudeAdapter {
        handshake: Duration::from_millis(300),
        ..ClaudeAdapter::default()
    };
    let handle = adapter
        .start(
            &fake_profile(&fx.exe, "noinit", &[]),
            fx.request(SessionMode::Changes),
            sink,
        )
        .await
        .unwrap();
    let outcome = handle.wait().await;
    assert_eq!(outcome.outcome, Outcome::IncompatibleCli);
    assert!(!outcome.exited_normally);
}
