//! The job supervisor (PRD M2 step 8, D25, D30, D34): owns which agent runs are live,
//! takes the BEFORE/AFTER snapshots around each run, and records the outcome. State
//! changes go through `Db::transition_job`'s guard, so a stale writer can't resurrect a
//! finished job.
// Wired into the commands layer in a later step.
#![allow(dead_code)]

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

use crate::persistence::{Db, DbError, JobState, JobUpdate, ProjectRow, SessionMode, SessionRow};
use crate::services::agents::{
    AgentAdapter, AgentEvent, Decision, EventSink, Outcome, RunHandle, RunRequest, StartError,
    profiles::AgentProfile,
};
use crate::services::git::{KEEP_RUNS, Phase, SnapshotRef, SnapshotStore};

#[cfg(test)]
mod tests;

const EVENT_BUFFER: usize = 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum JobError {
    #[error("an agent run is already active for this project")]
    Busy,
    #[error("could not snapshot the project before the run: {0}")]
    Snapshot(String),
    #[error(transparent)]
    Start(#[from] StartError),
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("could not prepare the session log: {0}")]
    Io(#[from] io::Error),
    #[error("no live run with that id")]
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

/// A run in flight. `run` is empty while the BEFORE snapshot is being taken.
struct Live {
    job_id: String,
    run: Option<(Arc<RunHandle>, watch::Receiver<bool>)>,
}

struct Inner<A> {
    db: Arc<Db>,
    snapshots: Arc<SnapshotStore>,
    adapter: A,
    data_dir: PathBuf,
    /// project_id -> live run. One run per project, plan runs included.
    live: Mutex<HashMap<String, Live>>,
    events: broadcast::Sender<(String, AgentEvent)>,
    shutting_down: AtomicBool,
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
    fn live(&self) -> MutexGuard<'_, HashMap<String, Live>> {
        self.live.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn transition(&self, job_id: &str, from: &[JobState], to: JobState, update: &JobUpdate) {
        // A false result means someone else already moved the job on; that is fine.
        if let Err(e) = self.db.transition_job(job_id, from, to, update) {
            eprintln!("job {job_id}: could not record {to:?}: {e}");
        }
    }

    /// Persists state changes the events imply, then hands the event to subscribers.
    fn sink(self: &Arc<Self>, job_id: &str) -> EventSink {
        let (inner, job_id) = (self.clone(), job_id.to_owned());
        Arc::new(move |e| {
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

    /// Waits for the run, snapshots AFTER on a normal exit, records the final state.
    async fn monitor(
        self: Arc<Self>,
        job_id: String,
        session_id: String,
        project: (String, PathBuf),
        handle: Arc<RunHandle>,
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
        let state = if self.shutting_down.load(Ordering::SeqCst) {
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
        // Free the slot before the final state is visible, so a finished job never reads as busy.
        self.live().remove(&project_id);
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

        let _ = finished.send(true);
    }
}

impl<A: AgentAdapter> JobSupervisor<A> {
    pub(crate) fn new(
        db: Arc<Db>,
        snapshots: Arc<SnapshotStore>,
        adapter: A,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                db,
                snapshots,
                adapter,
                data_dir,
                live: Mutex::default(),
                events: broadcast::channel(EVENT_BUFFER).0,
                shutting_down: AtomicBool::new(false),
            }),
        }
    }

    /// Every event of every run, tagged with its job id.
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<(String, AgentEvent)> {
        self.inner.events.subscribe()
    }

    /// Nothing survives an app restart: live jobs become `interrupted`.
    pub(crate) fn reconcile_on_startup(&self) -> Result<usize, DbError> {
        self.inner.db.mark_interrupted_on_startup()
    }

    /// Starts a run and returns its job id. Fails with `Busy` when the project already
    /// has a live run. If anything fails after the job row exists, the job is recorded
    /// as failed and nothing is left running.
    pub(crate) async fn start_agent_run(
        &self,
        project: &ProjectRow,
        profile: AgentProfile,
        prompt: String,
        mode: SessionMode,
        resume: Option<Resume>,
        spawn_env: Vec<(String, String)>,
    ) -> Result<String, JobError> {
        let inner = &self.inner;
        let job_id = uuid::Uuid::new_v4().to_string();
        {
            let mut live = inner.live();
            if live.contains_key(&project.id) || inner.db.has_running_modifying_job(&project.id)? {
                return Err(JobError::Busy);
            }
            live.insert(
                project.id.clone(),
                Live {
                    job_id: job_id.clone(),
                    run: None,
                },
            );
        }
        let launched = self
            .launch(&job_id, project, &profile, prompt, mode, resume, spawn_env)
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
            inner.live().remove(&project.id);
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
            created_at: String::new(),
        })?;

        // BEFORE comes first: without it there is nothing to diff or revert against.
        let before = inner
            .snapshot(&project.id, &project_dir, job_id, Phase::Before)
            .await
            .map_err(JobError::Snapshot)?;
        inner
            .db
            .set_session_refs(&session_id, Some(&before.commit), None)?;
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
            spawn_env,
            log_path,
        };
        let handle = Arc::new(
            inner
                .adapter
                .start(profile, request, inner.sink(job_id))
                .await?,
        );

        let (finished_tx, finished_rx) = watch::channel(false);
        if let Some(live) = inner.live().get_mut(&project.id) {
            live.run = Some((handle.clone(), finished_rx));
        }
        tokio::spawn(inner.clone().monitor(
            job_id.to_owned(),
            session_id,
            (project.id.clone(), project_dir),
            handle,
            finished_tx,
        ));
        Ok(())
    }

    fn find(&self, job_id: &str) -> Result<(Arc<RunHandle>, watch::Receiver<bool>), JobError> {
        self.inner
            .live()
            .values()
            .find(|l| l.job_id == job_id)
            .and_then(|l| l.run.clone())
            .ok_or(JobError::NotRunning)
    }

    pub(crate) fn answer_approval(
        &self,
        job_id: &str,
        request_id: &str,
        decision: Decision,
    ) -> Result<(), JobError> {
        let (handle, _) = self.find(job_id)?;
        handle
            .answer_approval(request_id, decision)
            .map_err(|_| JobError::NotRunning)
    }

    /// Stops the run (SIGTERM, then SIGKILL after 3 s) and returns once its final state
    /// is recorded.
    pub(crate) async fn cancel(&self, job_id: &str) -> Result<(), JobError> {
        let (handle, mut finished) = self.find(job_id)?;
        handle.cancel();
        // An error means the monitor is gone, which is as finished as it gets.
        let _ = finished.wait_for(|done| *done).await;
        Ok(())
    }

    /// App exit: kills every run's process group. Their jobs end as `interrupted`.
    pub(crate) async fn shutdown(&self) {
        self.inner.shutting_down.store(true, Ordering::SeqCst);
        let runs: Vec<_> = self
            .inner
            .live()
            .values()
            .filter_map(|l| l.run.clone())
            .collect();
        for (handle, _) in &runs {
            handle.cancel();
        }
        for (_, mut finished) in runs {
            let _ = finished.wait_for(|done| *done).await;
        }
    }
}
