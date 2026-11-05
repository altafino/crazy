//! The job supervisor (PRD M2 step 8, D25, D30, D34; M3-D6, M3-D10, M3-D24): owns which
//! jobs are live, takes the BEFORE/AFTER snapshots around each agent run, and records the
//! outcome. State changes go through `Db::transition_job`'s guard, so a stale writer can't
//! resurrect a finished job.
//!
//! Agent runs and previews share one slot table (`Slots`), keyed by (project, kind): at most
//! one agent run and one preview per project, independently. What a slot holds is a
//! `LiveJob`, so cancel, shutdown and "which job is live" work the same for both kinds.

use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

use tokio::sync::{broadcast, watch};

use crate::domain::{DatabaseEntry, ProjectMeta};
use crate::persistence::{Db, DbError, JobState, JobUpdate, ProjectRow, SessionMode, SessionRow};
use crate::services::agents::{
    AgentAdapter, AgentEvent, AutoApprove, Decision, EventSink, Outcome, RunHandle, RunRequest,
    StartError, profiles::AgentProfile,
};
use crate::services::appdb::convention::{ENGINE_SQLITE, highest_migration};
use crate::services::attachments::{AttachError, AttachmentStore, conversation_id};
use crate::services::git::checkpoint::CheckpointStore;
use crate::services::git::{KEEP_RUNS, Phase, SnapshotRef, SnapshotStore};

pub(crate) mod output;
pub(crate) mod proc;
#[cfg(test)]
mod tests;

/// D6: a run that added `src-tauri/migrations/NNNN_*.sql` to a project without a `database`
/// entry gets the entry now. Studio is the only writer of project.json; written once, since
/// later runs see the entry.
fn record_database_entry(project_dir: &Path) {
    if highest_migration(project_dir).is_none() {
        return;
    }
    match ProjectMeta::read(project_dir) {
        Ok(Some(mut meta)) if meta.database.is_none() => {
            meta.database = Some(DatabaseEntry {
                engine: ENGINE_SQLITE.into(),
            });
            if let Err(e) = meta.write(project_dir) {
                eprintln!("could not record the database entry: {e}");
            }
        }
        _ => {}
    }
}

const EVENT_BUFFER: usize = 1024;
/// A run's `Completed` event, held back until its final state is recorded.
type HeldEvent = Arc<Mutex<Option<AgentEvent>>>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum JobError {
    #[error("an agent run is already active for this project")]
    Busy,
    #[error(
        "A release build is running for this project. Wait for it to finish (or stop it) before making changes."
    )]
    BuildRunning,
    #[error("could not snapshot the project before the run: {0}")]
    Snapshot(String),
    #[error(transparent)]
    Start(#[from] StartError),
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Attach(#[from] AttachError),
    #[error("could not prepare the session log: {0}")]
    Io(#[from] io::Error),
    #[error("no live job with that id")]
    NotRunning,
}

/// Continues an earlier session: `--resume` plus the one-run tool rules from
/// "Allow and retry".
#[derive(Debug, Clone)]
pub(crate) struct Resume {
    pub session_id: String,
    pub provider_session_id: String,
    pub allowed_tools: Vec<String>,
}

/// The staged files a prompt carries: moved into the conversation folder when the run starts.
#[derive(Debug, Clone)]
pub(crate) struct AttachRequest {
    pub draft_id: String,
    pub item_ids: Vec<String>,
}

/// The kinds of long-running job a project can have (one slot each).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum JobKind {
    Agent,
    Preview,
    Build,
}

impl JobKind {
    /// The value of `jobs.kind`.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Preview => "preview",
            Self::Build => "build",
        }
    }
}

/// What the supervisor can do with any running job.
pub(crate) trait LiveJob: Send + Sync {
    /// Stops the job: SIGTERM to its process group, SIGKILL after a grace period.
    fn cancel(&self);

    /// Only agent runs have approvals.
    fn answer_approval(&self, _request_id: &str, _decision: Decision) -> Result<(), JobError> {
        Err(JobError::NotRunning)
    }
}

impl LiveJob for RunHandle {
    fn cancel(&self) {
        RunHandle::cancel(self);
    }

    fn answer_approval(&self, request_id: &str, decision: Decision) -> Result<(), JobError> {
        RunHandle::answer_approval(self, request_id, decision).map_err(|_| JobError::NotRunning)
    }
}

type Handle = (Arc<dyn LiveJob>, watch::Receiver<bool>);

/// A job in flight. `run` is empty while an agent run's BEFORE snapshot is being taken.
struct Live {
    job_id: String,
    run: Option<Handle>,
}

type SlotMap = HashMap<(String, JobKind), Live>;

/// The live-job table shared by the supervisor and the preview service.
#[derive(Default)]
pub(crate) struct Slots {
    live: Mutex<SlotMap>,
    shutting_down: AtomicBool,
}

impl Slots {
    fn live(&self) -> MutexGuard<'_, SlotMap> {
        self.live.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes the (project, kind) slot; false when it is already taken.
    pub(crate) fn claim(&self, project_id: &str, kind: JobKind, job_id: &str) -> bool {
        let mut live = self.live();
        let key = (project_id.to_owned(), kind);
        if live.contains_key(&key) {
            return false;
        }
        live.insert(
            key,
            Live {
                job_id: job_id.to_owned(),
                run: None,
            },
        );
        true
    }

    pub(crate) fn attach(
        &self,
        project_id: &str,
        kind: JobKind,
        job: Arc<dyn LiveJob>,
        finished: watch::Receiver<bool>,
    ) {
        if let Some(live) = self.live().get_mut(&(project_id.to_owned(), kind)) {
            live.run = Some((job, finished));
        }
    }

    pub(crate) fn release(&self, project_id: &str, kind: JobKind) {
        self.live().remove(&(project_id.to_owned(), kind));
    }

    /// The live job of `kind` for the project, if any.
    pub(crate) fn job_id(&self, project_id: &str, kind: JobKind) -> Option<String> {
        self.live()
            .get(&(project_id.to_owned(), kind))
            .map(|l| l.job_id.clone())
    }

    fn find(&self, job_id: &str) -> Result<Handle, JobError> {
        self.live()
            .values()
            .find(|l| l.job_id == job_id)
            .and_then(|l| l.run.clone())
            .ok_or(JobError::NotRunning)
    }

    /// Stops the job and returns once its final state is recorded.
    pub(crate) async fn cancel(&self, job_id: &str) -> Result<(), JobError> {
        let (job, mut finished) = self.find(job_id)?;
        job.cancel();
        // An error means the monitor is gone, which is as finished as it gets.
        let _ = finished.wait_for(|done| *done).await;
        Ok(())
    }

    /// True once the app is exiting: jobs that end now end as `interrupted`.
    pub(crate) fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst)
    }
}

struct Inner<A> {
    db: Arc<Db>,
    snapshots: Arc<SnapshotStore>,
    checkpoints: Arc<CheckpointStore>,
    adapter: A,
    data_dir: PathBuf,
    attachments: Arc<AttachmentStore>,
    /// One agent run per project, plan runs included, plus one preview.
    slots: Arc<Slots>,
    events: broadcast::Sender<(String, AgentEvent)>,
}

pub(crate) struct JobSupervisor<A: AgentAdapter> {
    inner: Arc<Inner<A>>,
}

impl<A: AgentAdapter> Clone for JobSupervisor<A> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

fn error_kind(e: &JobError) -> &'static str {
    match e {
        JobError::Start(e) => e.outcome().as_str(),
        _ => Outcome::Failed.as_str(),
    }
}

impl<A: AgentAdapter> Inner<A> {
    fn live(&self) -> MutexGuard<'_, SlotMap> {
        self.slots.live()
    }

    fn transition(&self, job_id: &str, from: &[JobState], to: JobState, update: &JobUpdate) {
        // A false result means someone else already moved the job on; that is fine.
        if let Err(e) = self.db.transition_job(job_id, from, to, update) {
            eprintln!("job {job_id}: could not record {to:?}: {e}");
        }
    }

    /// Persists state changes the events imply, then hands the event to subscribers.
    /// `Completed` is held in `held` and sent by `monitor` once the final state is recorded,
    /// because subscribers reload the job when it arrives.
    fn sink(self: &Arc<Self>, job_id: &str, held: HeldEvent) -> EventSink {
        let (inner, job_id) = (self.clone(), job_id.to_owned());
        Arc::new(move |e| {
            if matches!(e, AgentEvent::Completed { .. }) {
                *held.lock().unwrap_or_else(|p| p.into_inner()) = Some(e);
                return;
            }
            // ponytail: one pending approval at a time; the CLI asks sequentially.
            match &e {
                AgentEvent::ApprovalNeeded { .. } => inner.transition(
                    &job_id,
                    &[JobState::Running],
                    JobState::WaitingForApproval,
                    &JobUpdate::default(),
                ),
                AgentEvent::ApprovalAnswered { .. } => inner.transition(
                    &job_id,
                    &[JobState::WaitingForApproval],
                    JobState::Running,
                    &JobUpdate::default(),
                ),
                _ => {}
            }
            // No subscribers is normal (the UI may not be open).
            let _ = inner.events.send((job_id.clone(), e));
        })
    }

    async fn snapshot(
        self: &Arc<Self>,
        project_id: &str,
        dir: &Path,
        job_id: &str,
        phase: Phase,
    ) -> Result<SnapshotRef, String> {
        let (store, project_id, dir, job_id) = (
            self.snapshots.clone(),
            project_id.to_owned(),
            dir.to_path_buf(),
            job_id.to_owned(),
        );
        let label = if phase == Phase::Before {
            "before"
        } else {
            "after"
        };
        tokio::task::spawn_blocking(move || {
            store.snapshot(&project_id, &dir, &job_id, phase, label)
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
    }

    /// Fetches the BEFORE snapshot into the project's own repository (M3-D1). `Ok(None)` for
    /// a project that is not inside a Git repository.
    async fn checkpoint(
        self: &Arc<Self>,
        project_id: &str,
        dir: &Path,
        job_id: &str,
    ) -> Result<Option<String>, String> {
        let shadow = self
            .snapshots
            .shadow(project_id)
            .map_err(|e| e.to_string())?;
        let (cps, dir, job_id) = (
            self.checkpoints.clone(),
            dir.to_path_buf(),
            job_id.to_owned(),
        );
        tokio::task::spawn_blocking(move || cps.capture(&dir, &shadow, &job_id))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }

    /// Waits for the run, snapshots AFTER on a normal exit, records the final state.
    async fn monitor(
        self: Arc<Self>,
        job_id: String,
        session_id: String,
        project: (String, PathBuf),
        handle: Arc<RunHandle>,
        held: HeldEvent,
        finished: watch::Sender<bool>,
    ) {
        let (project_id, project_dir) = project;
        let run = handle.wait().await;
        if run.exited_normally {
            match self
                .snapshot(&project_id, &project_dir, &job_id, Phase::After)
                .await
            {
                Ok(snap) => {
                    if let Err(e) = self
                        .db
                        .set_session_refs(&session_id, None, Some(&snap.commit))
                    {
                        eprintln!("job {job_id}: could not store the after snapshot: {e}");
                    }
                }
                Err(e) => eprintln!("job {job_id}: after snapshot failed: {e}"),
            }
        }
        if let Some(id) = &run.provider_session_id
            && let Err(e) = self.db.set_provider_session_id(&session_id, id)
        {
            eprintln!("job {job_id}: could not store the provider session id: {e}");
        }
        let state = if self.slots.is_shutting_down() {
            JobState::Interrupted
        } else {
            match run.outcome {
                Outcome::Success => JobState::Succeeded,
                Outcome::Cancelled => JobState::Cancelled,
                _ => JobState::Failed,
            }
        };
        let (store, pid) = (self.snapshots.clone(), project_id.clone());
        match tokio::task::spawn_blocking(move || store.prune(&pid, KEEP_RUNS)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => eprintln!("job {job_id}: snapshot prune failed: {e}"),
            Err(e) => eprintln!("job {job_id}: snapshot prune did not run: {e}"),
        }
        let (atts, db, pid) = (
            self.attachments.clone(),
            self.db.clone(),
            project_id.clone(),
        );
        match tokio::task::spawn_blocking(move || atts.prune(&db, &pid, KEEP_RUNS)).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => eprintln!("job {job_id}: attachment prune failed: {e}"),
            Err(e) => eprintln!("job {job_id}: attachment prune did not run: {e}"),
        }
        record_database_entry(&project_dir);
        // Free the slot before the final state is visible, so a finished job never reads as busy.
        self.slots.release(&project_id, JobKind::Agent);
        let failed = run.outcome != Outcome::Success;
        self.transition(
            &job_id,
            &[JobState::Running, JobState::WaitingForApproval],
            state,
            &JobUpdate {
                set_completed: true,
                exit_code: run.exit_code,
                error_kind: failed.then(|| run.outcome.as_str().to_owned()),
                error_message: run.message,
                ..JobUpdate::default()
            },
        );
        let completed = held.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(e) = completed {
            let _ = self.events.send((job_id.clone(), e));
        }

        let _ = finished.send(true);
    }
}

impl<A: AgentAdapter> JobSupervisor<A> {
    pub(crate) fn new(
        db: Arc<Db>,
        snapshots: Arc<SnapshotStore>,
        checkpoints: Arc<CheckpointStore>,
        adapter: A,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                db,
                snapshots,
                checkpoints,
                adapter,
                attachments: Arc::new(AttachmentStore::new(&data_dir)),
                data_dir,
                slots: Arc::default(),
                events: broadcast::channel(EVENT_BUFFER).0,
            }),
        }
    }

    /// The attachment store: the commands stage into it, runs commit from it.
    pub(crate) fn attachments(&self) -> Arc<AttachmentStore> {
        self.inner.attachments.clone()
    }

    /// The live-job table, for the preview service.
    pub(crate) fn slots(&self) -> Arc<Slots> {
        self.inner.slots.clone()
    }

    /// Every event of every run, tagged with its job id.
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<(String, AgentEvent)> {
        self.inner.events.subscribe()
    }

    /// Nothing survives an app restart: live jobs (agent runs and previews) become
    /// `interrupted`.
    pub(crate) fn reconcile_on_startup(&self) -> Result<usize, DbError> {
        self.inner.db.mark_interrupted_on_startup()
    }

    /// Starts a run and returns its job id. Fails with `Busy` when the project already
    /// has a live run. If anything fails after the job row exists, the job is recorded
    /// as failed and nothing is left running.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn start_agent_run(
        &self,
        project: &ProjectRow,
        profile: AgentProfile,
        prompt: String,
        mode: SessionMode,
        resume: Option<Resume>,
        attach: Option<AttachRequest>,
        auto: Option<AutoApprove>,
        spawn_env: Vec<(String, String)>,
    ) -> Result<String, JobError> {
        let inner = &self.inner;
        let job_id = uuid::Uuid::new_v4().to_string();
        if inner.db.has_running_modifying_job(&project.id)?
            || !inner.slots.claim(&project.id, JobKind::Agent, &job_id)
        {
            return Err(JobError::Busy);
        }
        // M4-D11: a release must match its recorded source. The slot is already ours, so the
        // build service (which claims its slot, then checks for a run) cannot miss both.
        if mode == SessionMode::Changes
            && (inner.slots.job_id(&project.id, JobKind::Build).is_some()
                || inner
                    .db
                    .has_live_job(&project.id, JobKind::Build.as_str())?)
        {
            inner.slots.release(&project.id, JobKind::Agent);
            return Err(JobError::BuildRunning);
        }
        let launched = self
            .launch(
                &job_id, project, &profile, prompt, mode, resume, attach, auto, spawn_env,
            )
            .await;
        if let Err(e) = &launched {
            inner.transition(
                &job_id,
                &[JobState::Queued, JobState::Running],
                JobState::Failed,
                &JobUpdate {
                    set_completed: true,
                    error_kind: Some(error_kind(e).to_owned()),
                    error_message: Some(e.to_string()),
                    ..JobUpdate::default()
                },
            );
            inner.slots.release(&project.id, JobKind::Agent);
        }
        launched.map(|()| job_id)
    }

    #[allow(clippy::too_many_arguments)]
    async fn launch(
        &self,
        job_id: &str,
        project: &ProjectRow,
        profile: &AgentProfile,
        prompt: String,
        mode: SessionMode,
        resume: Option<Resume>,
        attach: Option<AttachRequest>,
        auto: Option<AutoApprove>,
        spawn_env: Vec<(String, String)>,
    ) -> Result<(), JobError> {
        let inner = &self.inner;
        let session_id = uuid::Uuid::new_v4().to_string();
        let log_path = inner
            .data_dir
            .join("sessions")
            .join(format!("{session_id}.jsonl"));
        let project_dir = PathBuf::from(&project.workspace_path);

        inner.db.insert_job(job_id, &project.id, JobState::Queued)?;
        inner.db.insert_session(&SessionRow {
            id: session_id.clone(),
            job_id: job_id.to_owned(),
            project_id: project.id.clone(),
            profile_id: Some(profile.id.clone()),
            mode,
            prompt: prompt.clone(),
            provider_session_id: None,
            resumed_from: resume.as_ref().map(|r| r.session_id.clone()),
            log_path: log_path.display().to_string(),
            before_ref: None,
            after_ref: None,
            checkpoint_ref: None,
            created_at: String::new(),
        })?;

        // Staged files move into the conversation folder now; a vanished one aborts the run
        // before anything is spawned. Earlier files of the conversation stay readable.
        let conversation = conversation_id(&inner.db, &session_id)?;
        let attachments_dir = inner.attachments.dir_for(&conversation);
        let mut attachment_paths = Vec::new();
        if let Some(a) = attach.filter(|a| !a.item_ids.is_empty()) {
            let rows = inner.attachments.commit(
                &inner.db,
                &a.draft_id,
                &conversation,
                &session_id,
                &a.item_ids,
            )?;
            attachment_paths = rows
                .iter()
                .map(|r| attachments_dir.join(r.converted_path.as_ref().unwrap_or(&r.stored_path)))
                .collect();
        }

        // BEFORE comes first: without it there is nothing to diff or revert against.
        let before = inner
            .snapshot(&project.id, &project_dir, job_id, Phase::Before)
            .await
            .map_err(JobError::Snapshot)?;
        inner
            .db
            .set_session_refs(&session_id, Some(&before.commit), None)?;
        // A missing checkpoint never blocks the run (M3-D5); it becomes a warning row.
        let mut pre_warnings = Vec::new();
        match inner.checkpoint(&project.id, &project_dir, job_id).await {
            Ok(Some(r)) => {
                if let Err(e) = inner.db.set_checkpoint_ref(&session_id, &r) {
                    eprintln!("job {job_id}: could not store the checkpoint ref: {e}");
                }
            }
            Ok(None) => {}
            Err(e) => pre_warnings.push(("No Git checkpoint for this run".to_owned(), Some(e))),
        }
        inner.transition(
            job_id,
            &[JobState::Queued],
            JobState::Running,
            &JobUpdate {
                set_started: true,
                ..JobUpdate::default()
            },
        );

        let request = RunRequest {
            project_dir: project_dir.clone(),
            prompt,
            mode,
            resume_session_id: resume.as_ref().map(|r| r.provider_session_id.clone()),
            allowed_tools: resume.map(|r| r.allowed_tools).unwrap_or_default(),
            attachments_dir: attachments_dir.is_dir().then_some(attachments_dir),
            attachments: attachment_paths,
            // Plan first promises that nothing changes, so it always asks.
            auto_approve: auto.filter(|_| mode == SessionMode::Changes),
            spawn_env,
            pre_warnings,
            log_path,
        };
        let held = HeldEvent::default();
        let handle = Arc::new(
            inner
                .adapter
                .start(profile, request, inner.sink(job_id, held.clone()))
                .await?,
        );

        let (finished_tx, finished_rx) = watch::channel(false);
        inner
            .slots
            .attach(&project.id, JobKind::Agent, handle.clone(), finished_rx);
        tokio::spawn(inner.clone().monitor(
            job_id.to_owned(),
            session_id,
            (project.id.clone(), project_dir),
            handle,
            held,
            finished_tx,
        ));
        Ok(())
    }

    pub(crate) fn answer_approval(
        &self,
        job_id: &str,
        request_id: &str,
        decision: Decision,
    ) -> Result<(), JobError> {
        let (job, _) = self.inner.slots.find(job_id)?;
        job.answer_approval(request_id, decision)
    }

    /// Stops the job (SIGTERM, then SIGKILL after a grace period) and returns once its final
    /// state is recorded. Works for agent runs and previews.
    pub(crate) async fn cancel(&self, job_id: &str) -> Result<(), JobError> {
        self.inner.slots.cancel(job_id).await
    }

    /// App exit: kills every job's process group (agent runs and previews). Their jobs end
    /// as `interrupted`.
    pub(crate) async fn shutdown(&self) {
        self.inner.slots.shutting_down.store(true, Ordering::SeqCst);
        let jobs: Vec<_> = self
            .inner
            .live()
            .values()
            .filter_map(|l| l.run.clone())
            .collect();
        for (job, _) in &jobs {
            job.cancel();
        }
        for (_, mut finished) in jobs {
            let _ = finished.wait_for(|done| *done).await;
        }
    }
}
