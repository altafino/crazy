//! The preview runner (PRD M3 step 6): `tauri dev` for a Tauri + Vue project, as a job.
//!
//! One preview per project, in the supervisor's slot table. Lifecycle lives in `jobs.state`
//! (running; Stop is `cancelled`; the app window closing with exit 0 is `succeeded`; any
//! other exit is `failed`). The live phase (Installing, Compiling, App running, ...) is
//! derived from output lines and is not stored.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::io::AsyncReadExt;
use tokio::sync::{broadcast, mpsc, watch};

use crate::persistence::{Db, DbError, JobState, JobUpdate, ProjectRow};
use crate::platform::process::spawn_group;
use crate::services::diagnostics::{DoctorReport, ToolState};
use crate::services::jobs::{JobKind, LiveJob, Slots};
use crate::services::settings;

mod output;
mod phase;
mod project;

use output::{LogSink, Tail};
pub(crate) use phase::PreviewPhase;
use phase::{PhaseParser, clean_line};
use project::PackageManager;

#[cfg(test)]
mod tests;

const BATCH_EVERY: Duration = Duration::from_millis(100);
const STOP_GRACE: Duration = Duration::from_secs(3);
/// A doctor report older than this is refreshed before a preview starts (M3-D27).
const REPORT_MAX_AGE_SECS: u32 = 10 * 60;
const INSTALL_SKIPPED: &str = "Dependencies not refreshed while the agent is working; restart the preview after the run if it added packages";

// ------------------------------------------------------------------ types

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewStatus {
    pub job_id: String,
    pub project_id: String,
    pub state: JobState,
    /// Only while the preview is live; never stored.
    pub phase: Option<PreviewPhase>,
    /// The port Studio picked for the dev server; `None` when the project's own dev command
    /// decides.
    pub port: Option<u16>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
    /// The preview process is still running.
    pub live: bool,
    /// A log file exists for "View log".
    pub has_log: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewLogTail {
    pub job_id: String,
    pub lines: Vec<String>,
    /// Live `PreviewLogEvent`s carry the sequence number of their first line; a view
    /// ignores events below this.
    pub next_seq: u32,
    /// The log file hit its size cap (the in-memory tail kept going while live).
    pub log_truncated: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum PreviewEvent {
    Status(PreviewStatus),
    /// Output lines, batched every 100 ms.
    Log {
        project_id: String,
        job_id: String,
        from_seq: u32,
        lines: Vec<String>,
    },
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PreviewError {
    #[error("Preview isn't available for imported projects.")]
    Unsupported,
    #[error("A preview is already running for this project.")]
    Busy,
    #[error(transparent)]
    Db(#[from] DbError),
}

/// The host environment a preview runs in: the doctor, its overrides and PATH. Implemented
/// over the real doctor in `commands`, and by a fake in tests.
pub(crate) trait Toolchain: Send + Sync + 'static {
    fn doctor_report(&self, force: bool)
    -> Pin<Box<dyn Future<Output = DoctorReport> + Send + '_>>;
    /// The doctor's `spawn_env()`.
    fn spawn_env(&self) -> Vec<(String, String)>;
    /// A tool's binary, honouring the user's override.
    fn resolve_binary(&self, tool: &str) -> Option<PathBuf>;
}

// ---------------------------------------------------------------- service

/// What `Stop` flips; the driver task watches it.
struct PreviewHandle {
    cancel: watch::Sender<bool>,
}

impl LiveJob for PreviewHandle {
    fn cancel(&self) {
        let _ = self.cancel.send(true);
    }
}

/// In-memory side of one preview run; kept for the session after it ends (M3-D40).
struct Run {
    job_id: String,
    project_id: String,
    status: Mutex<PreviewStatus>,
    parser: Mutex<PhaseParser>,
    sink: Mutex<LogSink>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Inner {
    db: Arc<Db>,
    slots: Arc<Slots>,
    toolchain: Arc<dyn Toolchain>,
    data_dir: PathBuf,
    /// project_id -> its latest preview.
    runs: Mutex<HashMap<String, Arc<Run>>>,
    events: broadcast::Sender<PreviewEvent>,
}

#[derive(Clone)]
pub(crate) struct PreviewService {
    inner: Arc<Inner>,
}

enum Outcome {
    Success,
    Cancelled,
    Failed {
        kind: &'static str,
        message: String,
        code: Option<i32>,
    },
}

impl Outcome {
    fn failed(kind: &'static str, message: impl Into<String>) -> Self {
        Self::Failed {
            kind,
            message: message.into(),
            code: None,
        }
    }
}

enum ProcEnd {
    Exited(std::process::ExitStatus),
    Cancelled,
    SpawnFailed(String),
}

struct Command {
    program: PathBuf,
    args: Vec<String>,
}

impl PreviewService {
    pub(crate) fn new(
        db: Arc<Db>,
        slots: Arc<Slots>,
        toolchain: Arc<dyn Toolchain>,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                db,
                slots,
                toolchain,
                data_dir,
                runs: Mutex::default(),
                events: broadcast::channel(256).0,
            }),
        }
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<PreviewEvent> {
        self.inner.events.subscribe()
    }

    /// Starts a preview and returns at once with the `running` status; preflight, install
    /// and `tauri dev` happen in the background and report through events and the job row.
    pub(crate) fn start(&self, project: &ProjectRow) -> Result<PreviewStatus, PreviewError> {
        let inner = &self.inner;
        if !project.template_id.supports_operations() {
            return Err(PreviewError::Unsupported);
        }
        let job_id = uuid::Uuid::new_v4().to_string();
        if inner
            .db
            .has_live_job(&project.id, JobKind::Preview.as_str())?
            || !inner.slots.claim(&project.id, JobKind::Preview, &job_id)
        {
            return Err(PreviewError::Busy);
        }
        let limits = settings::load_preview_limits(&inner.db);
        let started = (|| {
            inner.db.insert_job_of_kind(
                &job_id,
                &project.id,
                JobKind::Preview.as_str(),
                JobState::Running,
            )?;
            let ids: Vec<String> = inner
                .db
                .list_jobs_of_kind(&project.id, JobKind::Preview.as_str())?
                .into_iter()
                .map(|j| j.id)
                .collect();
            output::prune_logs(&inner.data_dir, &ids, limits.keep_logs as usize);
            inner.db.get_job(&job_id)
        })();
        let job = match started {
            Ok(job) => job,
            Err(e) => {
                inner.slots.release(&project.id, JobKind::Preview);
                return Err(e.into());
            }
        };
        let log = output::log_path(&inner.data_dir, &job_id);
        let status = PreviewStatus {
            job_id: job_id.clone(),
            project_id: project.id.clone(),
            state: JobState::Running,
            phase: Some(PreviewPhase::Starting),
            port: None,
            started_at: job.and_then(|j| j.started_at),
            completed_at: None,
            error_message: None,
            live: true,
            has_log: false,
        };
        let run = Arc::new(Run {
            job_id: job_id.clone(),
            project_id: project.id.clone(),
            status: Mutex::new(status.clone()),
            parser: Mutex::new(PhaseParser::new(PreviewPhase::Starting)),
            sink: Mutex::new(LogSink::new(&log, &limits)),
        });
        lock(&inner.runs).insert(project.id.clone(), run.clone());

        let (cancel_tx, cancel_rx) = watch::channel(false);
        let (finished_tx, finished_rx) = watch::channel(false);
        inner.slots.attach(
            &project.id,
            JobKind::Preview,
            Arc::new(PreviewHandle { cancel: cancel_tx }),
            finished_rx,
        );
        let _ = inner.events.send(PreviewEvent::Status(status.clone()));
        tokio::spawn(inner.clone().drive(
            run,
            PathBuf::from(&project.workspace_path),
            cancel_rx,
            finished_tx,
        ));
        Ok(status)
    }

    /// Stops the project's preview and returns once its final state is recorded. Nothing
    /// running is not an error.
    pub(crate) async fn stop(&self, project_id: &str) {
        if let Some(job_id) = self.inner.slots.job_id(project_id, JobKind::Preview) {
            let _ = self.inner.slots.cancel(&job_id).await;
        }
    }

    pub(crate) async fn restart(
        &self,
        project: &ProjectRow,
    ) -> Result<PreviewStatus, PreviewError> {
        self.stop(&project.id).await;
        self.start(project)
    }

    /// The project's latest preview: from memory this session, else its last job row.
    pub(crate) fn status(&self, project_id: &str) -> Result<Option<PreviewStatus>, PreviewError> {
        if let Some(run) = lock(&self.inner.runs).get(project_id).cloned() {
            let mut s = lock(&run.status).clone();
            s.has_log = output::log_path(&self.inner.data_dir, &s.job_id).exists();
            return Ok(Some(s));
        }
        let Some(job) = self
            .inner
            .db
            .list_jobs_of_kind(project_id, JobKind::Preview.as_str())?
            .into_iter()
            .next()
        else {
            return Ok(None);
        };
        Ok(Some(PreviewStatus {
            has_log: output::log_path(&self.inner.data_dir, &job.id).exists(),
            job_id: job.id,
            project_id: job.project_id,
            state: job.state,
            phase: None,
            port: None,
            started_at: job.started_at,
            completed_at: job.completed_at,
            error_message: job.error_message,
            live: false,
        }))
    }

    /// The tail of a preview's output (default: the project's latest). Memory while the
    /// session still has it, else the log file; `None` when the file was pruned.
    pub(crate) fn log_tail(
        &self,
        project_id: &str,
        job_id: Option<&str>,
    ) -> Result<Option<PreviewLogTail>, PreviewError> {
        let run = lock(&self.inner.runs).get(project_id).cloned();
        if let Some(run) = run.filter(|r| job_id.is_none_or(|j| j == r.job_id)) {
            let t = lock(&run.sink).tail();
            return Ok(Some(tail_of(&run.job_id, t)));
        }
        let job_id = match job_id {
            Some(j) => j.to_owned(),
            None => match self
                .inner
                .db
                .list_jobs_of_kind(project_id, JobKind::Preview.as_str())?
                .into_iter()
                .next()
            {
                Some(j) => j.id,
                None => return Ok(None),
            },
        };
        // Only this project's jobs; a job id from elsewhere must not read arbitrary logs.
        match self.inner.db.get_job(&job_id)? {
            Some(j) if j.project_id == project_id && j.kind == JobKind::Preview.as_str() => {}
            _ => return Ok(None),
        }
        let max = settings::load_preview_limits(&self.inner.db).max_lines as usize;
        Ok(
            output::read_tail(&output::log_path(&self.inner.data_dir, &job_id), max)
                .map(|t| tail_of(&job_id, t)),
        )
    }
}

fn tail_of(job_id: &str, t: Tail) -> PreviewLogTail {
    PreviewLogTail {
        job_id: job_id.to_owned(),
        lines: t.lines,
        next_seq: t.next_seq,
        log_truncated: t.log_truncated,
    }
}

// ----------------------------------------------------------------- driver

fn now_secs() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX))
}

impl Inner {
    fn emit_status(&self, run: &Run) {
        let s = lock(&run.status).clone();
        let _ = self.events.send(PreviewEvent::Status(s));
    }

    fn set_phase(&self, run: &Run, phase: PreviewPhase) {
        lock(&run.parser).set(phase.clone());
        self.publish_phase(run, phase);
    }

    fn publish_phase(&self, run: &Run, phase: PreviewPhase) {
        lock(&run.status).phase = Some(phase);
        self.emit_status(run);
    }

    /// Output that is not from the process (Studio's own notes).
    fn note(&self, run: &Run, text: &str) {
        lock(&run.sink).push(text.to_owned());
    }

    fn flush(&self, run: &Run) {
        if let Some((from_seq, lines)) = lock(&run.sink).take_batch() {
            let _ = self.events.send(PreviewEvent::Log {
                project_id: run.project_id.clone(),
                job_id: run.job_id.clone(),
                from_seq,
                lines,
            });
        }
    }

    /// One output line: the phase parser sees every line, then memory and the file keep it.
    fn on_line(&self, run: &Run, raw: &str, parse: bool) {
        let line = clean_line(raw);
        if line.is_empty() {
            return;
        }
        if parse {
            let changed = lock(&run.parser).feed(&line).cloned();
            if let Some(p) = changed {
                self.publish_phase(run, p);
            }
        }
        lock(&run.sink).push(line);
    }

    async fn drive(
        self: Arc<Self>,
        run: Arc<Run>,
        dir: PathBuf,
        mut cancel: watch::Receiver<bool>,
        finished: watch::Sender<bool>,
    ) {
        let outcome = self.execute(&run, &dir, &mut cancel).await;
        self.flush(&run);

        let (state, update) = if self.slots.is_shutting_down() {
            (
                JobState::Interrupted,
                JobUpdate {
                    set_completed: true,
                    ..JobUpdate::default()
                },
            )
        } else {
            match &outcome {
                Outcome::Success => (
                    JobState::Succeeded,
                    JobUpdate {
                        set_completed: true,
                        exit_code: Some(0),
                        ..JobUpdate::default()
                    },
                ),
                Outcome::Cancelled => (
                    JobState::Cancelled,
                    JobUpdate {
                        set_completed: true,
                        error_kind: Some("cancelled".into()),
                        ..JobUpdate::default()
                    },
                ),
                Outcome::Failed {
                    kind,
                    message,
                    code,
                } => (
                    JobState::Failed,
                    JobUpdate {
                        set_completed: true,
                        exit_code: *code,
                        error_kind: Some((*kind).into()),
                        error_message: Some(message.clone()),
                        ..JobUpdate::default()
                    },
                ),
            }
        };
        // Free the slot before the final state is visible, so a finished preview never reads
        // as busy.
        self.slots.release(&run.project_id, JobKind::Preview);
        if let Err(e) = self
            .db
            .transition_job(&run.job_id, &[JobState::Running], state, &update)
        {
            eprintln!("preview {}: could not record {state:?}: {e}", run.job_id);
        }
        let completed_at = self
            .db
            .get_job(&run.job_id)
            .ok()
            .flatten()
            .and_then(|j| j.completed_at);
        {
            let mut s = lock(&run.status);
            s.state = state;
            s.live = false;
            s.phase = None;
            s.completed_at = completed_at;
            s.error_message = update.error_message.clone();
            s.has_log = output::log_path(&self.data_dir, &run.job_id).exists();
        }
        self.emit_status(&run);
        let _ = finished.send(true);
    }

    async fn execute(&self, run: &Run, dir: &Path, cancel: &mut watch::Receiver<bool>) -> Outcome {
        let manager = project::detect_package_manager(dir);
        if let Err(message) = self.preflight(manager).await {
            return Outcome::failed("preflight", message);
        }
        if *cancel.borrow() {
            return Outcome::Cancelled;
        }
        let program = self
            .toolchain
            .resolve_binary(manager.name())
            .unwrap_or_else(|| manager.name().into());

        // Install (M3-D8, M3-D23, M3-D28).
        if self
            .db
            .has_running_modifying_job(&run.project_id)
            .unwrap_or(false)
        {
            self.note(run, INSTALL_SKIPPED);
        } else {
            self.set_phase(run, PreviewPhase::Installing);
            let cmd = Command {
                program: program.clone(),
                args: manager
                    .install_args()
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect(),
            };
            match self.run_proc(run, dir, cmd, false, cancel).await {
                ProcEnd::Cancelled => return Outcome::Cancelled,
                ProcEnd::Exited(st) if st.success() => {}
                ProcEnd::Exited(_) | ProcEnd::SpawnFailed(_) => {
                    return Outcome::failed("installFailed", "Installing dependencies failed");
                }
            }
        }

        // tauri dev, with one retry on a taken port (M3-D7, M3-D32).
        let mut retried = false;
        let mut next_port = None;
        loop {
            let mut args: Vec<String> = manager
                .tauri_dev_args()
                .iter()
                .map(|s| (*s).to_owned())
                .collect();
            let port = next_port.take().or_else(|| project::free_port().ok());
            let overridden = port.and_then(|p| project::port_override(dir, p).map(|c| (p, c)));
            lock(&run.status).port = overridden.as_ref().map(|(p, _)| *p);
            if let Some((_, config)) = &overridden {
                args.extend(["--config".to_owned(), config.clone()]);
            }
            *lock(&run.parser) = PhaseParser::new(PreviewPhase::Compiling);
            self.publish_phase(run, PreviewPhase::Compiling);

            let cmd = Command {
                program: program.clone(),
                args,
            };
            let st = match self.run_proc(run, dir, cmd, true, cancel).await {
                ProcEnd::Cancelled => return Outcome::Cancelled,
                ProcEnd::SpawnFailed(e) => {
                    return Outcome::failed(
                        "spawnFailed",
                        format!("Could not start the preview: {e}"),
                    );
                }
                ProcEnd::Exited(st) if st.success() => {
                    let crash = lock(&run.parser).crash().map(str::to_owned);
                    return match crash {
                        Some(message) => Outcome::failed("crashed", message),
                        None => Outcome::Success,
                    };
                }
                ProcEnd::Exited(st) => st,
            };
            let (taken, first_error) = {
                let p = lock(&run.parser);
                let error = p.first_error().or(p.crash()).map(str::to_owned);
                (p.port_in_use(), error)
            };
            if let Some(taken) = taken {
                if overridden.is_some() && !retried {
                    retried = true;
                    let next = project::free_port().unwrap_or(taken.wrapping_add(1));
                    next_port = Some(next);
                    self.note(run, &format!("Port {taken} was taken; retrying on {next}"));
                    continue;
                }
                return Outcome::failed(
                    "portInUse",
                    format!(
                        "Port {taken} is already in use by another program. Close it (another preview or dev server?) and start the preview again."
                    ),
                );
            }
            let message = first_error.unwrap_or_else(|| match st.code() {
                Some(c) => format!("The preview stopped with exit code {c}."),
                None => "The preview was stopped by a signal.".into(),
            });
            return Outcome::Failed {
                kind: "exited",
                message,
                code: st.code(),
            };
        }
    }

    /// M3-D27: every tool the preview needs is installed, judged by a doctor report that is
    /// at most 10 minutes old. The messages are the doctor's own hints.
    async fn preflight(&self, manager: PackageManager) -> Result<(), String> {
        if manager != PackageManager::Pnpm
            && self.toolchain.resolve_binary(manager.name()).is_none()
        {
            return Err(format!(
                "This project uses {0}, which isn't installed",
                manager.name()
            ));
        }
        let mut report = self.toolchain.doctor_report(false).await;
        if now_secs().saturating_sub(report.checked_at) > REPORT_MAX_AGE_SECS {
            report = self.toolchain.doctor_report(true).await;
        }
        let mut needed = vec!["node", "cargo", "rustc", "webkit2gtk"];
        if manager == PackageManager::Pnpm {
            needed.push("pnpm");
        }
        for id in needed {
            let Some(tool) = report.tools.iter().find(|t| t.id == id) else {
                return Err(format!("Preview needs {id}"));
            };
            match &tool.state {
                ToolState::Installed { .. } => {}
                ToolState::Missing { hint } => {
                    return Err(format!("Preview needs {}: {hint}", tool.name));
                }
                ToolState::Misconfigured { reason, .. } => {
                    return Err(format!("Preview needs {}: {reason}", tool.name));
                }
            }
        }
        Ok(())
    }

    /// Runs one process in the project folder, streaming its lines to the preview's output
    /// until it exits or the preview is cancelled.
    async fn run_proc(
        &self,
        run: &Run,
        dir: &Path,
        cmd: Command,
        parse: bool,
        cancel: &mut watch::Receiver<bool>,
    ) -> ProcEnd {
        let env = self.toolchain.spawn_env();
        let mut group =
            match spawn_group(&cmd.program, &cmd.args, &env, Some(dir), Stdio::null()).await {
                Ok(g) => g,
                Err(e) => return ProcEnd::SpawnFailed(format!("{}: {e}", cmd.program.display())),
            };
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        if let Some(out) = group.child.stdout.take() {
            tokio::spawn(pump(out, tx.clone()));
        }
        if let Some(err) = group.child.stderr.take() {
            tokio::spawn(pump(err, tx.clone()));
        }
        drop(tx);

        let mut tick = tokio::time::interval(BATCH_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let end = loop {
            tokio::select! {
                () = async { let _ = cancel.wait_for(|c| *c).await; } => {
                    let _ = group.terminate_group(STOP_GRACE).await;
                    break ProcEnd::Cancelled;
                }
                st = group.child.wait() => match st {
                    Ok(st) => break ProcEnd::Exited(st),
                    Err(e) => break ProcEnd::SpawnFailed(e.to_string()),
                },
                Some(line) = rx.recv() => self.on_line(run, &line, parse),
                _ = tick.tick() => self.flush(run),
            }
        };
        // Whatever the process wrote last, then sweep stragglers (a dev server that outlived
        // the app) so nothing keeps the port.
        let _ = tokio::time::timeout(Duration::from_millis(500), async {
            while let Some(line) = rx.recv().await {
                self.on_line(run, &line, parse);
            }
        })
        .await;
        let _ = group.kill_group().await;
        self.flush(run);
        end
    }
}

/// Splits a pipe into lines (on `\n`; `clean_line` handles `\r` redraws). A line without a
/// newline is cut at 64 KiB so one runaway line can't grow memory.
async fn pump(mut pipe: impl tokio::io::AsyncRead + Unpin, tx: mpsc::UnboundedSender<String>) {
    const MAX_LINE: usize = 64 * 1024;
    let (mut buf, mut chunk) = (Vec::new(), [0u8; 8192]);
    loop {
        let n = match pipe.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..n]);
        while let Some(i) = buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buf.drain(..=i).collect();
            let _ = tx.send(String::from_utf8_lossy(&line[..i]).into_owned());
        }
        if buf.len() > MAX_LINE {
            let _ = tx.send(String::from_utf8_lossy(&std::mem::take(&mut buf)).into_owned());
        }
    }
    if !buf.is_empty() {
        let _ = tx.send(String::from_utf8_lossy(&buf).into_owned());
    }
}
