//! The build service (PRD M4 step 4): turns a project into a stored, verified, smoke-tested
//! release, as a `build` job. One build per project, in the supervisor's slot table.
//!
//! Lifecycle lives in `jobs.state` (running; Stop is `cancelled`; shutdown is `interrupted`;
//! any failure is `failed` with one of the kinds in `FailureKind`; a build that produced a
//! release, even one whose test launch failed, is `succeeded`). The live phase is derived and
//! never stored. The adapter (`adapter.rs`) runs the project's own build; this service owns
//! the job, the source record (M4-D3), verification, storage (M4-D6), the smoke test (M4-D10)
//! and retention (M4-D7). A failed build never touches `current.AppImage`, the launcher icon
//! or other releases.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::{broadcast, watch};

use crate::persistence::{
    Db, DbError, JobState, JobUpdate, ProjectRow, ReleaseRow, SessionMode, Smoke,
};
use crate::platform::paths::{AppLayout, StudioPaths};
use crate::services::appdb::{AppDb, convention};
use crate::services::diagnostics::{DoctorReport, ToolState};
use crate::services::git::SnapshotStore;
use crate::services::git::checkpoint::CheckpointStore;
use crate::services::jobs::output::{self, LogSink, Tail};
use crate::services::jobs::proc::{LineSink, ProcEnd, ProcSpec, run_proc};
use crate::services::jobs::{JobKind, LiveJob, Slots};
use crate::services::preview::Toolchain;
use crate::services::preview::phase::{PhaseParser, PreviewPhase, clean_line, is_crash_line};
use crate::services::projects::tauri::{self as project, PackageManager};
use crate::services::settings;

mod adapter;
mod store;
mod version;

#[allow(unused_imports)] // the public API for wiring and tests
pub(crate) use adapter::{
    BuildAdapter, BuildCtx, BuildFailure, BuildFuture, BuildOutput, BuiltApp, FailureKind,
    TauriAppImageAdapter,
};
#[allow(unused_imports)]
pub(crate) use store::{remove_leftover_temp_files, sha256_file};

#[cfg(test)]
mod real_build_test;
#[cfg(test)]
mod tests;

/// The per-kind log directory under the data directory.
const LOG_DIR: &str = "builds";
const STOP_GRACE: Duration = Duration::from_secs(3);
/// A doctor report older than this is refreshed before a build starts.
const REPORT_MAX_AGE_SECS: u32 = 10 * 60;
/// How long the stored AppImage must stay up for the test launch to pass (M4-D10).
pub(crate) const SMOKE_AFTER: Duration = Duration::from_secs(5);
/// Releases kept by age after the active and the previously activated one (M4-D7).
const KEEP_NEWEST: usize = 5;
const MAX_NOTES: usize = 2_000;
const SMOKE_PREFIX: &str = "[test launch] ";
/// Under Studio's data folder: the test launches' temporary XDG folders.
const TEST_HOME_DIR: &str = "test-launch";

// ------------------------------------------------------------------ types

/// What a running build is doing; never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum BuildPhase {
    /// Source record and tool checks, before anything runs.
    Preflight,
    Installing,
    Compiling,
    Packaging,
    /// Verifying and copying the AppImage into the release folder.
    Storing,
    /// The 5-second test launch of the stored AppImage.
    Testing,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BuildStatus {
    pub job_id: String,
    pub project_id: String,
    /// Known while this session has the build, or when it produced a release; a failed build
    /// from an earlier session has none.
    pub version: Option<String>,
    pub state: JobState,
    /// Only while the build is live.
    pub phase: Option<BuildPhase>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    /// One of preflight, installFailed, buildFailed, noArtifact, verifyFailed, storeFailed,
    /// cancelled; none on success and when interrupted.
    pub error_kind: Option<String>,
    pub error_message: Option<String>,
    /// The release this build produced.
    pub release_id: Option<String>,
    /// The build is still running.
    pub live: bool,
    /// A log file exists for "View log".
    pub has_log: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BuildLogTail {
    pub job_id: String,
    pub lines: Vec<String>,
    /// Live `BuildEvent::Log`s carry the sequence number of their first line; a view ignores
    /// events below this.
    pub next_seq: u32,
    pub log_truncated: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum BuildEvent {
    Status(BuildStatus),
    /// Output lines, batched every 100 ms.
    Log {
        project_id: String,
        job_id: String,
        from_seq: u32,
        lines: Vec<String>,
    },
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum BuildError {
    #[error("No AppImage builder configured")]
    Unsupported,
    #[error("A build is already running for this project.")]
    Busy,
    #[error("Wait for the running Make-changes run to finish before building.")]
    AgentRunning,
    #[error(
        "\"{0}\" is not a valid version. Use MAJOR.MINOR.PATCH, for example 1.2.0, with an optional suffix such as -beta.1."
    )]
    InvalidVersion(String),
    #[error("Version {0} already has a release. Choose a higher version or delete that release.")]
    VersionExists(String),
    #[error("There is no such release.")]
    NotFound,
    #[error("This is the active release. Activate another release before deleting it.")]
    ActiveRelease,
    #[error("Could not remove the release files: {0}")]
    Io(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

/// Everything the service needs from outside.
pub(crate) struct BuildDeps {
    pub db: Arc<Db>,
    pub slots: Arc<Slots>,
    pub toolchain: Arc<dyn Toolchain>,
    pub adapter: Arc<dyn BuildAdapter>,
    /// The shadow repository store (never `ensure`d against a user repo).
    pub snapshots: Arc<SnapshotStore>,
    /// Plain Git on the user's project: HEAD, status, and fetching the build ref.
    pub checkpoints: Arc<CheckpointStore>,
    pub paths: StudioPaths,
    /// `SMOKE_AFTER`, shortened in tests.
    pub smoke_after: Duration,
}

// ---------------------------------------------------------------- service

struct BuildHandle {
    cancel: watch::Sender<bool>,
}

impl LiveJob for BuildHandle {
    fn cancel(&self) {
        let _ = self.cancel.send(true);
    }
}

/// In-memory side of one build; kept for the session after it ends.
struct Run {
    job_id: String,
    project_id: String,
    status: Mutex<BuildStatus>,
    parser: Mutex<PhaseParser>,
    sink: Mutex<LogSink>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Inner {
    deps: BuildDeps,
    /// project_id -> its latest build.
    runs: Mutex<HashMap<String, Arc<Run>>>,
    events: broadcast::Sender<BuildEvent>,
}

#[derive(Clone)]
pub(crate) struct BuildService {
    inner: Arc<Inner>,
}

/// What `drive` needs to know about the request.
struct Request {
    project: ProjectRow,
    version: String,
    notes: Option<String>,
    release_id: String,
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
    fn failed(kind: FailureKind, message: impl Into<String>) -> Self {
        Self::Failed {
            kind: kind.as_str(),
            message: message.into(),
            code: None,
        }
    }
}

impl From<BuildFailure> for Outcome {
    fn from(f: BuildFailure) -> Self {
        if f.kind == FailureKind::Cancelled {
            return Self::Cancelled;
        }
        Self::Failed {
            kind: f.kind.as_str(),
            message: f.message,
            code: f.exit_code,
        }
    }
}

/// The recorded source of a build (M4-D3).
struct Source {
    commit: Option<String>,
    dirty: bool,
    snapshot_ref: String,
}

fn notes_of(notes: Option<String>) -> Option<String> {
    notes
        .map(|n| n.trim().chars().take(MAX_NOTES).collect::<String>())
        .filter(|n| !n.is_empty())
}

impl BuildService {
    pub(crate) fn new(deps: BuildDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                deps,
                runs: Mutex::default(),
                events: broadcast::channel(256).0,
            }),
        }
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<BuildEvent> {
        self.inner.events.subscribe()
    }

    /// App startup: removes temp files a killed build left under `installed/*/releases/`.
    /// (Running builds are marked interrupted by the supervisor's startup reconcile.)
    pub(crate) fn cleanup_on_startup(&self) {
        let data = &self.inner.deps.paths.data_dir;
        remove_leftover_temp_files(data);
        // A test launch cut short by a crash or kill leaves its temporary folders.
        let _ = std::fs::remove_dir_all(data.join(TEST_HOME_DIR));
    }

    /// Starts a build and returns at once with the `running` status; the rest happens in the
    /// background and reports through events and the job row. Refusals leave nothing behind.
    pub(crate) fn start(
        &self,
        project: &ProjectRow,
        version: String,
        notes: Option<String>,
    ) -> Result<BuildStatus, BuildError> {
        let inner = &self.inner;
        let d = &inner.deps;
        if !project.template_id.supports_operations() {
            return Err(BuildError::Unsupported);
        }
        let job_id = uuid::Uuid::new_v4().to_string();
        if d.db.has_live_job(&project.id, JobKind::Build.as_str())?
            || !d.slots.claim(&project.id, JobKind::Build, &job_id)
        {
            return Err(BuildError::Busy);
        }
        // The slot is ours: claim first, then look for a run, so a Make-changes run starting
        // at the same moment (it claims, then looks for a build) cannot miss us (M4-D11).
        let prepared = (|| {
            if inner.modifying_run_live(&project.id)? {
                return Err(BuildError::AgentRunning);
            }
            if version::Version::parse(&version).is_none() {
                return Err(BuildError::InvalidVersion(version.clone()));
            }
            if d.db
                .list_releases(&project.id, false)?
                .iter()
                .any(|r| r.version == version)
            {
                return Err(BuildError::VersionExists(version.clone()));
            }
            d.db.insert_job_of_kind(
                &job_id,
                &project.id,
                JobKind::Build.as_str(),
                JobState::Running,
            )?;
            let ids: Vec<String> =
                d.db.list_jobs_of_kind(&project.id, JobKind::Build.as_str())?
                    .into_iter()
                    .map(|j| j.id)
                    .collect();
            let limits = settings::load_preview_limits(&d.db);
            output::prune_logs(&d.paths.data_dir, LOG_DIR, &ids, limits.keep_logs as usize);
            Ok((d.db.get_job(&job_id)?, limits))
        })();
        let (job, limits) = match prepared {
            Ok(p) => p,
            Err(e) => {
                d.slots.release(&project.id, JobKind::Build);
                return Err(e);
            }
        };

        let log = output::log_path(&d.paths.data_dir, LOG_DIR, &job_id);
        let status = BuildStatus {
            job_id: job_id.clone(),
            project_id: project.id.clone(),
            version: Some(version.clone()),
            state: JobState::Running,
            phase: Some(BuildPhase::Preflight),
            started_at: job.and_then(|j| j.started_at),
            completed_at: None,
            error_kind: None,
            error_message: None,
            release_id: None,
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
        d.slots.attach(
            &project.id,
            JobKind::Build,
            Arc::new(BuildHandle { cancel: cancel_tx }),
            finished_rx,
        );
        let _ = inner.events.send(BuildEvent::Status(status.clone()));
        let req = Request {
            project: project.clone(),
            version,
            notes: notes_of(notes),
            release_id: uuid::Uuid::new_v4().to_string(),
        };
        tokio::spawn(inner.clone().drive(run, req, cancel_rx, finished_tx));
        Ok(status)
    }

    /// Stops the project's build and returns once its final state is recorded. Nothing
    /// running is not an error.
    pub(crate) async fn stop(&self, project_id: &str) {
        let d = &self.inner.deps;
        if let Some(job_id) = d.slots.job_id(project_id, JobKind::Build) {
            let _ = d.slots.cancel(&job_id).await;
        }
    }

    /// The project's latest build: from memory this session, else its last job row.
    pub(crate) fn status(&self, project_id: &str) -> Result<Option<BuildStatus>, BuildError> {
        let d = &self.inner.deps;
        if let Some(run) = lock(&self.inner.runs).get(project_id).cloned() {
            let mut s = lock(&run.status).clone();
            s.has_log = output::log_path(&d.paths.data_dir, LOG_DIR, &s.job_id).exists();
            return Ok(Some(s));
        }
        let Some(job) =
            d.db.list_jobs_of_kind(project_id, JobKind::Build.as_str())?
                .into_iter()
                .next()
        else {
            return Ok(None);
        };
        let release =
            d.db.list_releases(project_id, true)?
                .into_iter()
                .find(|r| r.job_id.as_deref() == Some(job.id.as_str()));
        Ok(Some(BuildStatus {
            has_log: output::log_path(&d.paths.data_dir, LOG_DIR, &job.id).exists(),
            version: release.as_ref().map(|r| r.version.clone()),
            release_id: release.map(|r| r.id),
            job_id: job.id,
            project_id: job.project_id,
            state: job.state,
            phase: None,
            started_at: job.started_at,
            completed_at: job.completed_at,
            error_kind: job.error_kind,
            error_message: job.error_message,
            live: false,
        }))
    }

    /// The tail of a build's output (default: the project's latest). Memory while the session
    /// still has it, else the log file; `None` when the file was pruned.
    pub(crate) fn log_tail(
        &self,
        project_id: &str,
        job_id: Option<&str>,
    ) -> Result<Option<BuildLogTail>, BuildError> {
        let d = &self.inner.deps;
        let run = lock(&self.inner.runs).get(project_id).cloned();
        if let Some(run) = run.filter(|r| job_id.is_none_or(|j| j == r.job_id)) {
            let t = lock(&run.sink).tail();
            return Ok(Some(tail_of(&run.job_id, t)));
        }
        let job_id = match job_id {
            Some(j) => j.to_owned(),
            None => match d
                .db
                .list_jobs_of_kind(project_id, JobKind::Build.as_str())?
                .into_iter()
                .next()
            {
                Some(j) => j.id,
                None => return Ok(None),
            },
        };
        // Only this project's jobs; a job id from elsewhere must not read arbitrary logs.
        match d.db.get_job(&job_id)? {
            Some(j) if j.project_id == project_id && j.kind == JobKind::Build.as_str() => {}
            _ => return Ok(None),
        }
        let max = settings::load_preview_limits(&d.db).max_lines as usize;
        Ok(
            output::read_tail(&output::log_path(&d.paths.data_dir, LOG_DIR, &job_id), max)
                .map(|t| tail_of(&job_id, t)),
        )
    }

    /// The version to pre-fill in the build dialog: the next patch above the highest release
    /// ever built (removed ones count), or the project's configured version when that is
    /// higher; "0.1.0" when there is neither.
    pub(crate) fn next_version(&self, project: &ProjectRow) -> Result<String, BuildError> {
        let rows = self.inner.deps.db.list_releases(&project.id, true)?;
        let configured =
            project::read_tauri_config(Path::new(&project.workspace_path)).and_then(|c| c.version);
        Ok(version::suggest(
            rows.iter().map(|r| r.version.as_str()),
            configured.as_deref(),
        ))
    }

    /// Deletes a release's files and build refs and marks its row removed. Refused for the
    /// active release (the `current.AppImage` symlink decides, M4-D5).
    pub(crate) async fn delete_release(
        &self,
        project_id: &str,
        release_id: &str,
    ) -> Result<(), BuildError> {
        let inner = self.inner.clone();
        let (pid, rid) = (project_id.to_owned(), release_id.to_owned());
        tokio::task::spawn_blocking(move || delete_release(&inner.deps, &pid, &rid))
            .await
            .map_err(|e| BuildError::Io(e.to_string()))?
    }

    /// Deletes the build refs of `release_ids` from the shadow repo and the project's own
    /// repo (for uninstall). Best effort: a failure is only logged. Blocking.
    pub(crate) fn forget_refs(&self, project: &ProjectRow, release_ids: &[String]) {
        forget_refs(&self.inner.deps, project, release_ids);
    }
}

fn delete_release(d: &BuildDeps, project_id: &str, release_id: &str) -> Result<(), BuildError> {
    let row =
        d.db.get_release(release_id)?
            .filter(|r| r.project_id == project_id)
            .ok_or(BuildError::NotFound)?;
    let layout = AppLayout::new(&d.paths, project_id);
    if row.files_removed_at.is_none() && layout.active_version().as_deref() == Some(&row.version) {
        return Err(BuildError::ActiveRelease);
    }
    remove_release(d, &layout, &row)?;
    if let Some(p) = d.db.get_project(project_id)? {
        forget_refs(d, &p, &[row.id]);
    }
    Ok(())
}

/// Deletes the release's folder and marks the row. Never called for the active release.
fn remove_release(d: &BuildDeps, layout: &AppLayout, row: &ReleaseRow) -> Result<(), BuildError> {
    // The version came from a validated build; check again before it becomes a path.
    if version::Version::parse(&row.version).is_none() {
        return Err(BuildError::Io(format!("bad version {:?}", row.version)));
    }
    match std::fs::remove_dir_all(layout.release_dir(&row.version)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(BuildError::Io(e.to_string())),
    }
    if row.files_removed_at.is_none() {
        d.db.mark_release_files_removed(&row.id)?;
    }
    // DR-8: the automatic "before <version>" backups go with their release.
    if let Err(e) = AppDb::new(d.db.clone(), &d.paths).delete_release_backups(&row.id) {
        eprintln!(
            "release {}: deleting its data backups failed: {e}",
            row.version
        );
    }
    Ok(())
}

fn forget_refs(d: &BuildDeps, project: &ProjectRow, release_ids: &[String]) {
    for id in release_ids {
        if let Err(e) = d.snapshots.delete_build_ref(&project.id, id) {
            eprintln!("build ref {id}: shadow delete failed: {e}");
        }
        if let Err(e) = d
            .checkpoints
            .delete_build_ref(Path::new(&project.workspace_path), id)
        {
            eprintln!("build ref {id}: project repo delete failed: {e}");
        }
    }
}

/// M4-D7: after a successful build keep the active release, the one activated before it, and
/// the 5 newest; everything else loses its files and refs (rows stay).
fn apply_retention(d: &BuildDeps, project: &ProjectRow) -> Result<(), BuildError> {
    let layout = AppLayout::new(&d.paths, &project.id);
    let kept = d.db.list_releases(&project.id, false)?;
    let victims = retention_victims(&kept, layout.active_version().as_deref());
    let mut gone = Vec::new();
    for row in victims {
        match remove_release(d, &layout, row) {
            Ok(()) => gone.push(row.id.clone()),
            Err(e) => eprintln!("retention: release {}: {e}", row.version),
        }
    }
    forget_refs(d, project, &gone);
    Ok(())
}

/// Kept releases (newest first) that retention removes: all but the active one, the one most
/// recently activated besides it, and the `KEEP_NEWEST` newest.
fn retention_victims<'a>(kept: &'a [ReleaseRow], active: Option<&str>) -> Vec<&'a ReleaseRow> {
    let previous = kept
        .iter()
        .filter(|r| Some(r.version.as_str()) != active)
        .filter_map(|r| r.last_activated_at.as_deref().map(|t| (t, &r.id)))
        .max()
        .map(|(_, id)| id);
    kept.iter()
        .enumerate()
        .filter(|(i, r)| {
            Some(r.version.as_str()) != active && Some(&r.id) != previous && *i >= KEEP_NEWEST
        })
        .map(|(_, r)| r)
        .collect()
}

fn tail_of(job_id: &str, t: Tail) -> BuildLogTail {
    BuildLogTail {
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

/// `created_at` in the database's own format (`%Y-%m-%dT%H:%M:%fZ`, UTC, milliseconds).
fn iso_now() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let (secs, ms) = (d.as_secs(), d.subsec_millis());
    // Days since 1970-01-01 to a civil date (Hinnant's algorithm).
    let z = i64::try_from(secs / 86_400).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let t = secs % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{ms:03}Z",
        t / 3_600,
        t / 60 % 60,
        t % 60
    )
}

impl Inner {
    /// True while a Make-changes run is live or may be: a run in its slot whose session is not
    /// a plan run (including one that is still starting). Plan runs only read.
    fn modifying_run_live(&self, project_id: &str) -> Result<bool, BuildError> {
        let d = &self.deps;
        if d.db.has_running_modifying_job(project_id)? {
            return Ok(true);
        }
        let Some(job) = d.slots.job_id(project_id, JobKind::Agent) else {
            return Ok(false);
        };
        let plan =
            d.db.list_sessions_by_project(project_id)?
                .iter()
                .any(|s| s.job_id == job && s.mode == SessionMode::Plan);
        Ok(!plan)
    }

    fn emit_status(&self, run: &Run) {
        let s = lock(&run.status).clone();
        let _ = self.events.send(BuildEvent::Status(s));
    }

    fn set_phase(&self, run: &Run, phase: BuildPhase) {
        lock(&run.status).phase = Some(phase);
        self.emit_status(run);
    }

    /// Output that is not from a process (Studio's own notes).
    fn note(&self, run: &Run, text: &str) {
        lock(&run.sink).push(text.to_owned());
    }

    fn flush(&self, run: &Run) {
        if let Some((from_seq, lines)) = lock(&run.sink).take_batch() {
            let _ = self.events.send(BuildEvent::Log {
                project_id: run.project_id.clone(),
                job_id: run.job_id.clone(),
                from_seq,
                lines,
            });
        }
    }

    /// One output line of the build: the parser keeps the first error, then the log keeps it.
    fn on_line(&self, run: &Run, raw: &str) {
        let line = clean_line(raw);
        if line.is_empty() {
            return;
        }
        lock(&run.parser).feed(&line);
        // Tauri announces the bundle step ("Bundling app_1.0.0_amd64.AppImage").
        if line.trim_start().starts_with("Bundling ")
            && lock(&run.status).phase == Some(BuildPhase::Compiling)
        {
            self.set_phase(run, BuildPhase::Packaging);
        }
        lock(&run.sink).push(line);
    }

    async fn drive(
        self: Arc<Self>,
        run: Arc<Run>,
        req: Request,
        mut cancel: watch::Receiver<bool>,
        finished: watch::Sender<bool>,
    ) {
        let d = &self.deps;
        let mut stored = None;
        let outcome = self.pipeline(&run, &req, &mut cancel, &mut stored).await;
        let ok = matches!(outcome, Outcome::Success);
        if !ok {
            // Nothing half-built stays: the release folder this build made, and its refs.
            if let Some(dir) = stored.take() {
                let _ = tokio::task::spawn_blocking(move || std::fs::remove_dir_all(dir)).await;
            }
            let (project, id) = (req.project.clone(), req.release_id.clone());
            let this = self.clone();
            let _ = tokio::task::spawn_blocking(move || {
                forget_refs(&this.deps, &project, &[id]);
            })
            .await;
        }
        self.flush(&run);

        let (state, update) = if d.slots.is_shutting_down() {
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
                        error_kind: Some(FailureKind::Cancelled.as_str().into()),
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
        // Free the slot before the final state is visible, so a finished build never reads
        // as busy.
        d.slots.release(&run.project_id, JobKind::Build);
        if let Err(e) =
            d.db.transition_job(&run.job_id, &[JobState::Running], state, &update)
        {
            eprintln!("build {}: could not record {state:?}: {e}", run.job_id);
        }
        let completed_at =
            d.db.get_job(&run.job_id)
                .ok()
                .flatten()
                .and_then(|j| j.completed_at);
        {
            let mut s = lock(&run.status);
            s.state = state;
            s.live = false;
            s.phase = None;
            s.completed_at = completed_at;
            s.error_kind = update.error_kind.clone();
            s.error_message = update.error_message.clone();
            s.release_id = ok.then(|| req.release_id.clone());
            s.has_log = output::log_path(&d.paths.data_dir, LOG_DIR, &run.job_id).exists();
        }
        self.emit_status(&run);
        let _ = finished.send(true);
    }

    /// The build, step by step. `stored` holds the release folder this build created while it
    /// has no release row yet, so `drive` can remove it when the build ends any other way.
    async fn pipeline(
        self: &Arc<Self>,
        run: &Run,
        req: &Request,
        cancel: &mut watch::Receiver<bool>,
        stored: &mut Option<PathBuf>,
    ) -> Outcome {
        let d = &self.deps;
        let dir = PathBuf::from(&req.project.workspace_path);
        let layout = AppLayout::new(&d.paths, &req.project.id);

        // Source record (M4-D3).
        let source = match self.record_source(run, req).await {
            Ok(s) => s,
            Err(message) => return Outcome::failed(FailureKind::StoreFailed, message),
        };
        if *cancel.borrow() {
            return Outcome::Cancelled;
        }

        if let Err(message) = self.preflight(project::detect_package_manager(&dir)).await {
            return Outcome::failed(FailureKind::Preflight, message);
        }
        if *cancel.borrow() {
            return Outcome::Cancelled;
        }

        // The project's own build.
        let out = RunOut { svc: self, run };
        let built = d
            .adapter
            .build(BuildCtx {
                project_dir: &dir,
                version: &req.version,
                toolchain: d.toolchain.as_ref(),
                out: &out,
                cancel,
            })
            .await;
        let artifact = match built {
            Ok(b) => b.artifact,
            Err(f) => return f.into(),
        };

        // Verify, then store (A11, M4-D6).
        self.set_phase(run, BuildPhase::Storing);
        let check = artifact.clone();
        match tokio::task::spawn_blocking(move || store::verify_appimage(&check)).await {
            Ok(Ok(())) => {}
            Ok(Err(m)) => return Outcome::failed(FailureKind::VerifyFailed, m),
            Err(e) => return Outcome::failed(FailureKind::VerifyFailed, e.to_string()),
        }
        let Some(cfg) = project::read_tauri_config(&dir) else {
            return Outcome::failed(
                FailureKind::StoreFailed,
                "Could not read the app's name from src-tauri/tauri.conf.json",
            );
        };
        // Re-checked here because the release folder is named after the version: a kept
        // release must never be overwritten.
        match d.db.list_releases(&req.project.id, false) {
            Ok(rows) if rows.iter().any(|r| r.version == req.version) => {
                return Outcome::failed(
                    FailureKind::StoreFailed,
                    BuildError::VersionExists(req.version.clone()).to_string(),
                );
            }
            Ok(_) => {}
            Err(e) => return Outcome::failed(FailureKind::StoreFailed, e.to_string()),
        }
        let release_dir = layout.release_dir(&req.version);
        if !release_dir.exists() {
            *stored = Some(release_dir.clone());
        }
        let (app_path, icon_path) = (
            layout.release_appimage(&req.version),
            layout.release_icon(&req.version),
        );
        let icon = store::find_icon(&dir);
        let (a, ap, ip) = (artifact, app_path.clone(), icon_path);
        let put = tokio::task::spawn_blocking(move || {
            store::store_release(&a, icon.as_deref(), &ap, &ip)
        })
        .await;
        let put = match put {
            Ok(Ok(p)) => p,
            Ok(Err(e)) => {
                return Outcome::failed(
                    FailureKind::StoreFailed,
                    format!("Could not store the release: {e}"),
                );
            }
            Err(e) => return Outcome::failed(FailureKind::StoreFailed, e.to_string()),
        };
        if *cancel.borrow() {
            return Outcome::Cancelled;
        }

        // Test launch (M4-D10).
        self.set_phase(run, BuildPhase::Testing);
        let Some((smoke, smoke_detail)) = self
            .smoke(run, &req.project, &app_path, &release_dir, cancel)
            .await
        else {
            return Outcome::Cancelled;
        };

        let row = ReleaseRow {
            id: req.release_id.clone(),
            project_id: req.project.id.clone(),
            job_id: Some(run.job_id.clone()),
            version: req.version.clone(),
            sha256: put.sha256,
            size_bytes: i64::try_from(put.size_bytes).unwrap_or(i64::MAX),
            architecture: std::env::consts::ARCH.to_owned(),
            build_profile: "local".into(),
            product_name: cfg.product_name,
            main_binary_name: cfg.main_binary_name,
            source_commit: source.commit,
            dirty_source: source.dirty,
            snapshot_ref: Some(source.snapshot_ref),
            smoke,
            smoke_detail,
            notes: req.notes.clone(),
            created_at: iso_now(),
            last_activated_at: None,
            files_removed_at: None,
            schema_version: convention::highest_migration(&dir).map(i64::from),
        };
        if let Err(e) = d.db.insert_release(&row) {
            return Outcome::failed(
                FailureKind::StoreFailed,
                format!("Could not record the release: {e}"),
            );
        }
        *stored = None;

        let this = self.clone();
        let project = req.project.clone();
        // A failed cleanup never fails a build that produced a release.
        match tokio::task::spawn_blocking(move || apply_retention(&this.deps, &project)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => self.note(
                run,
                &format!("Warning: cleanup of old releases failed: {e}"),
            ),
            Err(e) => self.note(
                run,
                &format!("Warning: cleanup of old releases failed: {e}"),
            ),
        }
        Outcome::Success
    }

    /// HEAD, the dirty flag and the shadow snapshot, fetched into the project's repository
    /// as `refs/studio/builds/<release-id>`. A fetch failure is a warning in the log; any
    /// other failure is the build's (the error text).
    async fn record_source(&self, run: &Run, req: &Request) -> Result<Source, String> {
        let d = &self.deps;
        let (snapshots, checkpoints) = (d.snapshots.clone(), d.checkpoints.clone());
        let (project, release_id, version) = (
            req.project.clone(),
            req.release_id.clone(),
            req.version.clone(),
        );
        let done = tokio::task::spawn_blocking(move || {
            let dir = PathBuf::from(&project.workspace_path);
            let (commit, dirty) = (checkpoints.head_commit(&dir), checkpoints.is_dirty(&dir));
            let (_, name) = snapshots
                .snapshot_build(&project.id, &dir, &release_id, &format!("build {version}"))
                .map_err(|e| format!("Could not record the source of this build: {e}"))?;
            let shadow = snapshots.shadow(&project.id).map_err(|e| e.to_string())?;
            let warning = checkpoints
                .capture_build(&dir, &shadow, &release_id)
                .err()
                .map(|e| e.to_string());
            Ok::<_, String>((commit, dirty, name, warning))
        })
        .await
        .map_err(|e| e.to_string())??;
        let (commit, dirty, snapshot_ref, warning) = done;
        if let Some(w) = warning {
            self.note(
                run,
                &format!(
                    "Warning: could not copy the source snapshot into the project's Git repository: {w}"
                ),
            );
        }
        Ok(Source {
            commit,
            dirty,
            snapshot_ref,
        })
    }

    /// Every tool the build needs is installed, judged by a doctor report at most 10 minutes
    /// old. The messages are the doctor's own hints.
    async fn preflight(&self, manager: PackageManager) -> Result<(), String> {
        let tc = &self.deps.toolchain;
        if manager != PackageManager::Pnpm && tc.resolve_binary(manager.name()).is_none() {
            return Err(format!(
                "Build needs {0}: this project uses it, but it isn't installed",
                manager.name()
            ));
        }
        let mut report: DoctorReport = tc.doctor_report(false).await;
        if now_secs().saturating_sub(report.checked_at) > REPORT_MAX_AGE_SECS {
            report = tc.doctor_report(true).await;
        }
        let mut needed = vec![
            "node",
            "cargo",
            "rustc",
            "webkit2gtk",
            "xdg-open",
            "libfuse2",
        ];
        if manager == PackageManager::Pnpm {
            needed.push("pnpm");
        }
        for id in needed {
            let Some(tool) = report.tools.iter().find(|t| t.id == id) else {
                return Err(format!("Build needs {id}"));
            };
            match &tool.state {
                ToolState::Installed { .. } => {}
                ToolState::Missing { hint } => {
                    return Err(format!("Build needs {}: {hint}", tool.name));
                }
                ToolState::Misconfigured { reason, .. } => {
                    return Err(format!("Build needs {}: {reason}", tool.name));
                }
            }
        }
        Ok(())
    }

    /// The test launch (M4-D10): the stored AppImage in its own process group, output into
    /// the build log under "[test launch]". `None` when the build was stopped meanwhile.
    async fn smoke(
        &self,
        run: &Run,
        project: &ProjectRow,
        app: &Path,
        dir: &Path,
        cancel: &mut watch::Receiver<bool>,
    ) -> Option<(Smoke, Option<String>)> {
        let mut env = self.deps.toolchain.spawn_env();
        if !env
            .iter()
            .any(|(k, v)| (k == "WAYLAND_DISPLAY" || k == "DISPLAY") && !v.is_empty())
        {
            let why = "No display is available (WAYLAND_DISPLAY and DISPLAY are not set), so the test launch was skipped";
            self.note(run, &format!("{SMOKE_PREFIX}{why}"));
            return Some((Smoke::Unverified, Some(why.into())));
        }
        env.push(("APPIMAGELAUNCHER_DISABLE".into(), "1".into()));

        // R1/D3: the app gets its own data, config and cache folders, so a build never opens
        // the real ones; deleted when this function returns. Without them the launch is not run.
        let home = match TestHome::create(&self.deps.paths.data_dir, &run.job_id) {
            Ok(h) => h,
            Err(e) => {
                let why = format!("Could not prepare the test launch's data folders: {e}");
                self.note(run, &format!("{SMOKE_PREFIX}{why}"));
                return Some((Smoke::Unverified, Some(why)));
            }
        };
        env.retain(|(k, _)| {
            !matches!(
                k.as_str(),
                "XDG_DATA_HOME" | "XDG_CONFIG_HOME" | "XDG_CACHE_HOME"
            )
        });
        for (var, sub) in [
            ("XDG_DATA_HOME", "data"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
        ] {
            env.push((var.into(), home.root.join(sub).display().to_string()));
        }
        // D3/R11: with a database and an installed app that has data, the app opens a copy of
        // that data, so a crash on it fails the launch. A failed copy means the launch ran on
        // empty data and proves nothing about the real data: Unverified at best.
        let appdb = AppDb::new(self.deps.db.clone(), &self.deps.paths);
        let (pid, temp_data) = (project.id.clone(), home.root.join("data"));
        let copied =
            tokio::task::spawn_blocking(move || appdb.copy_installed_for_test(&pid, &temp_data))
                .await;
        let copy_failed = match copied {
            Ok(Ok(placed)) => {
                if placed {
                    self.note(
                        run,
                        &format!("{SMOKE_PREFIX}Using a copy of the installed app's data"),
                    );
                }
                None
            }
            Ok(Err(e)) => Some(e.to_string()),
            Err(e) => Some(e.to_string()),
        }
        .map(|reason| format!("Couldn't copy the installed app's data: {reason}"));
        if let Some(why) = &copy_failed {
            self.note(
                run,
                &format!("{SMOKE_PREFIX}{why}; the app starts with empty data"),
            );
        }
        let result = self.launch_smoke(run, &env, app, dir, cancel).await;
        match (result, copy_failed) {
            (Some((Smoke::Passed, _)), Some(why)) => Some((Smoke::Unverified, Some(why))),
            (result, _) => result,
        }
    }

    /// The launch itself, in `env`.
    async fn launch_smoke(
        &self,
        run: &Run,
        env: &[(String, String)],
        app: &Path,
        dir: &Path,
        cancel: &mut watch::Receiver<bool>,
    ) -> Option<(Smoke, Option<String>)> {
        let crash = Mutex::new(None::<String>);
        let sink = SmokeSink {
            svc: self,
            run,
            crash: &crash,
        };
        let (stop_tx, mut stop_rx) = watch::channel(false);
        let mut proc = pin!(run_proc(
            ProcSpec {
                program: app,
                args: &[],
                env,
                cwd: dir,
            },
            &sink,
            &mut stop_rx,
            STOP_GRACE,
        ));
        // Resolves to true when the build was stopped, false when the app stayed up long enough.
        let timer = async {
            tokio::select! {
                () = tokio::time::sleep(self.deps.smoke_after) => false,
                _ = cancel.wait_for(|c| *c) => true,
            }
        };
        let first = tokio::select! {
            end = &mut proc => Err(end),
            stopped = timer => Ok(stopped),
        };
        let (end, still_up) = match first {
            Err(end) => (end, false),
            Ok(true) => {
                let _ = stop_tx.send(true);
                let _ = proc.await;
                return None;
            }
            Ok(false) => {
                let _ = stop_tx.send(true);
                (proc.await, true)
            }
        };
        let crash_line = lock(&crash).clone();
        let result = match end {
            ProcEnd::SpawnFailed(e) => (
                Smoke::Unverified,
                Some(format!("Could not start the test launch: {e}")),
            ),
            _ if crash_line.is_some() => (Smoke::Failed, crash_line),
            _ if still_up => (Smoke::Passed, None),
            ProcEnd::Exited(st) if st.success() => (Smoke::Passed, None),
            ProcEnd::Exited(st) => (
                Smoke::Failed,
                Some(match st.code() {
                    Some(c) => format!("exited with code {c}"),
                    None => "was stopped by a signal".to_owned(),
                }),
            ),
            ProcEnd::Cancelled => return None,
        };
        Some(result)
    }
}

/// The test launch's own XDG folders: `<data>/test-launch/<job-id>/{data,config,cache}`. Removed
/// on drop, whichever way the launch ends.
struct TestHome {
    root: PathBuf,
}

impl TestHome {
    fn create(studio_data: &Path, job_id: &str) -> std::io::Result<Self> {
        let root = studio_data.join(TEST_HOME_DIR).join(job_id);
        let _ = std::fs::remove_dir_all(&root);
        // Owned before the folders exist, so a half-made tree is removed too.
        let home = Self { root };
        for sub in ["data", "config", "cache"] {
            std::fs::create_dir_all(home.root.join(sub))?;
        }
        Ok(home)
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The build's view of a process: lines go through the first-error parser into the log.
struct RunOut<'a> {
    svc: &'a Inner,
    run: &'a Run,
}

impl BuildOutput for RunOut<'_> {
    fn line(&self, raw: &str) {
        self.svc.on_line(self.run, raw);
    }
    fn flush(&self) {
        self.svc.flush(self.run);
    }
    fn phase(&self, phase: BuildPhase) {
        self.svc.set_phase(self.run, phase);
    }
    fn first_error(&self) -> Option<String> {
        let p = lock(&self.run.parser);
        p.first_error().or(p.crash()).map(str::to_owned)
    }
}

/// The test launch's output: prefixed into the log and checked for a fatal line.
struct SmokeSink<'a> {
    svc: &'a Inner,
    run: &'a Run,
    crash: &'a Mutex<Option<String>>,
}

impl LineSink for SmokeSink<'_> {
    fn line(&self, raw: &str) {
        let line = clean_line(raw);
        if line.is_empty() {
            return;
        }
        if is_crash_line(&line) {
            lock(self.crash).get_or_insert_with(|| line.clone());
        }
        self.svc.note(self.run, &format!("{SMOKE_PREFIX}{line}"));
    }
    fn flush(&self) {
        self.svc.flush(self.run);
    }
}
