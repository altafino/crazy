//! Supervisor tests: the real adapter and spawn path against the fake `claude`, real
//! snapshots in a temp dir.
use std::{fs, path::Path, time::Duration};

use tempfile::TempDir;

use super::*;
use crate::domain::TemplateId;
use crate::persistence::{JobRow, NewProject};
use crate::services::agents::claude::{ClaudeAdapter, testkit::*};
use crate::services::agents::profiles;
use crate::services::git::{FileStatus, GitRunner};

struct Fx {
    tmp: TempDir,
    db: Arc<Db>,
    store: Arc<SnapshotStore>,
    sup: JobSupervisor<ClaudeAdapter>,
    project: ProjectRow,
    exe: PathBuf,
}

fn fx() -> Fx {
    fx_with("git")
}

fn fx_with(git_program: &str) -> Fx {
    let tmp = TempDir::new().unwrap();
    let exe = install_fake(tmp.path());
    let dir = tmp.path().join("project");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("readme.md"), "start\n").unwrap();

    let db = Arc::new(Db::open_in_memory().unwrap());
    db.insert_project(&NewProject {
        id: "proj1".into(),
        name: "a".into(),
        slug: "a".into(),
        template_id: TemplateId::TauriVue,
        app_id: "com.x.a".into(),
        workspace_path: dir.display().to_string(),
        pending_op: None,
    })
    .unwrap();
    let project = db.get_project("proj1").unwrap().unwrap();
    profiles::save(&db, &fake_profile(&exe, "write", &[])).unwrap();

    let data = tmp.path().join("data");
    let store = Arc::new(SnapshotStore::new(
        &data,
        GitRunner {
            program: git_program.into(),
            env: vec![
                ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
                ("HOME".into(), tmp.path().display().to_string()),
                ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
                ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            ],
        },
    ));
    let sup = JobSupervisor::new(db.clone(), store.clone(), ClaudeAdapter::default(), data);
    Fx {
        tmp,
        db,
        store,
        sup,
        project,
        exe,
    }
}

impl Fx {
    fn file(&self, name: &str) -> String {
        self.tmp.path().join(name).display().to_string()
    }

    fn dir(&self) -> PathBuf {
        PathBuf::from(&self.project.workspace_path)
    }

    async fn start(
        &self,
        scenario: &str,
        env: &[(&str, &str)],
        mode: SessionMode,
    ) -> Result<String, JobError> {
        self.start_profile(fake_profile(&self.exe, scenario, env), mode)
            .await
    }

    async fn start_profile(
        &self,
        p: profiles::AgentProfile,
        mode: SessionMode,
    ) -> Result<String, JobError> {
        self.sup
            .start_agent_run(&self.project, p, "do it".into(), mode, None, spawn_env())
            .await
    }

    fn session(&self, job_id: &str) -> SessionRow {
        self.db
            .list_sessions_by_project("proj1")
            .unwrap()
            .into_iter()
            .find(|s| s.job_id == job_id)
            .unwrap()
    }

    async fn wait_job(&self, id: &str, pred: impl Fn(&JobRow) -> bool) -> JobRow {
        for _ in 0..500 {
            let job = self.db.get_job(id).unwrap().unwrap();
            if pred(&job) {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!(
            "job never reached the expected state: {:?}",
            self.db.get_job(id)
        );
    }

    async fn finished(&self, id: &str) -> JobRow {
        self.wait_job(id, |j| j.state.is_terminal()).await
    }
}

async fn read_pid(path: &str) -> i32 {
    for _ in 0..250 {
        if let Ok(s) = fs::read_to_string(path)
            && let Ok(pid) = s.trim().parse()
        {
            return pid;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("no pid written");
}

fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks existence.
    unsafe { libc::kill(pid, 0) == 0 }
}

async fn gone(pid: i32) -> bool {
    for _ in 0..100 {
        if !alive(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

#[tokio::test]
async fn full_run_snapshots_before_and_after_and_the_diff_shows_the_write() {
    let fx = fx();
    let mut events = fx.sup.subscribe();
    let job = fx.start("write", &[], SessionMode::Changes).await.unwrap();
    let done = fx.finished(&job).await;
    assert_eq!(done.state, JobState::Succeeded);
    assert_eq!((done.exit_code, done.error_kind), (Some(0), None));
    assert!(done.started_at.is_some() && done.completed_at.is_some());

    let session = fx.session(&job);
    assert_eq!(session.provider_session_id.as_deref(), Some("sess-1"));
    let (before, after) = (session.before_ref.unwrap(), session.after_ref.unwrap());
    let diff = fx.store.diff("proj1", &before, &after).unwrap();
    let [change] = &diff[..] else {
        panic!("{diff:?}")
    };
    assert_eq!(
        (change.path.as_str(), change.status),
        ("out.txt", FileStatus::Added)
    );

    // Subscribers saw the run, tagged with the job id; the log holds the same events.
    let mut seen = Vec::new();
    while let Ok((id, e)) = events.try_recv() {
        assert_eq!(id, job);
        seen.push(e);
    }
    assert_eq!(kinds(&seen).last().unwrap(), "completed");
    let log = fs::read_to_string(&session.log_path).unwrap();
    assert_eq!(log.lines().count(), seen.len());
    assert!(fx.sup.inner.live().is_empty(), "slot released");
}

#[test]
fn approval_events_move_the_job_between_running_and_waiting() {
    let fx = fx();
    fx.db.insert_job("j1", "proj1", JobState::Running).unwrap();
    let sink = fx.sup.inner.sink("j1");
    let state = || fx.db.get_job("j1").unwrap().unwrap().state;
    let needed = AgentEvent::ApprovalNeeded {
        seq: 0,
        request_id: "r".into(),
        tool: "Bash".into(),
        input: serde_json::json!({}),
        description: String::new(),
    };
    sink(needed);
    assert_eq!(state(), JobState::WaitingForApproval);
    sink(AgentEvent::ApprovalAnswered {
        seq: 1,
        request_id: "r".into(),
        allowed: true,
    });
    assert_eq!(state(), JobState::Running);
}

#[tokio::test]
async fn approval_allow_and_deny_round_trips() {
    let fx = fx();
    let job = fx
        .start("approve", &[], SessionMode::Changes)
        .await
        .unwrap();
    fx.wait_job(&job, |j| j.state == JobState::WaitingForApproval)
        .await;
    fx.sup
        .answer_approval(&job, "req-1", Decision::Allow)
        .unwrap();
    assert_eq!(fx.finished(&job).await.state, JobState::Succeeded);
    assert!(fx.dir().join("approved.txt").exists());
    let diff = {
        let s = fx.session(&job);
        fx.store
            .diff("proj1", &s.before_ref.unwrap(), &s.after_ref.unwrap())
            .unwrap()
    };
    assert_eq!(diff[0].path, "approved.txt");

    fs::remove_file(fx.dir().join("approved.txt")).unwrap();
    let job = fx
        .start("approve", &[], SessionMode::Changes)
        .await
        .unwrap();
    fx.wait_job(&job, |j| j.state == JobState::WaitingForApproval)
        .await;
    fx.sup
        .answer_approval(&job, "req-1", Decision::Deny(None))
        .unwrap();
    assert_eq!(fx.finished(&job).await.state, JobState::Succeeded);
    assert!(!fx.dir().join("approved.txt").exists());
}

#[tokio::test]
async fn cli_without_the_prompt_tool_flag_runs_in_fallback() {
    let fx = fx();
    let mut events = fx.sup.subscribe();
    let job = fx
        .start("write", &[("FAKE_HELP", "fallback")], SessionMode::Changes)
        .await
        .unwrap();
    assert_eq!(fx.finished(&job).await.state, JobState::Succeeded);
    let (_, first) = events.try_recv().unwrap();
    assert!(matches!(first, AgentEvent::Warning { .. }));
}

#[tokio::test]
async fn blocked_when_neither_flag_exists_and_nothing_is_left_behind() {
    let fx = fx();
    let err = fx
        .start("write", &[("FAKE_HELP", "none")], SessionMode::Changes)
        .await
        .unwrap_err();
    assert!(
        matches!(err, JobError::Start(StartError::IncompatibleCli(_))),
        "{err}"
    );
    let job = &fx.db.list_jobs_by_project("proj1").unwrap()[0];
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.error_kind.as_deref(), Some("incompatibleCli"));
    assert!(fx.sup.inner.live().is_empty());
    assert!(!fx.dir().join("out.txt").exists());
}

#[tokio::test]
async fn cancel_kills_the_whole_process_group() {
    let fx = fx();
    let pidfile = fx.file("pid");
    let job = fx
        .start("hang", &[("FAKE_PIDFILE", &pidfile)], SessionMode::Changes)
        .await
        .unwrap();
    let pid = read_pid(&pidfile).await;
    assert!(alive(pid));
    fx.sup.cancel(&job).await.unwrap();
    let done = fx.db.get_job(&job).unwrap().unwrap();
    assert_eq!(done.state, JobState::Cancelled);
    assert_eq!(done.error_kind.as_deref(), Some("cancelled"));
    assert!(gone(pid).await, "tool subprocess survived");
    assert!(
        fx.session(&job).after_ref.is_none(),
        "no AFTER for a killed run"
    );
    assert!(matches!(
        fx.sup.cancel(&job).await,
        Err(JobError::NotRunning)
    ));
}

#[tokio::test]
async fn inactivity_limit_stops_a_silent_run() {
    let fx = fx();
    let mut p = fake_profile(&fx.exe, "hang", &[]);
    p.inactivity_secs = 2;
    let started = std::time::Instant::now();
    let job = fx.start_profile(p, SessionMode::Changes).await.unwrap();
    let done = fx.finished(&job).await;
    assert!(started.elapsed() >= Duration::from_secs(2));
    assert_eq!(done.state, JobState::Failed);
    assert_eq!(done.error_kind.as_deref(), Some("timeout"));
}

#[tokio::test]
async fn waiting_for_approval_does_not_count_as_inactivity() {
    let fx = fx();
    let mut p = fake_profile(&fx.exe, "approve", &[]);
    p.inactivity_secs = 2;
    let job = fx.start_profile(p, SessionMode::Changes).await.unwrap();
    fx.wait_job(&job, |j| j.state == JobState::WaitingForApproval)
        .await;
    tokio::time::sleep(Duration::from_millis(3200)).await;
    assert_eq!(
        fx.db.get_job(&job).unwrap().unwrap().state,
        JobState::WaitingForApproval
    );
    fx.sup
        .answer_approval(&job, "req-1", Decision::Allow)
        .unwrap();
    assert_eq!(fx.finished(&job).await.state, JobState::Succeeded);
}

#[tokio::test]
async fn one_run_per_project_even_for_plan_runs() {
    let fx = fx();
    let job = fx.start("hang", &[], SessionMode::Plan).await.unwrap();
    let err = fx
        .start("write", &[], SessionMode::Changes)
        .await
        .unwrap_err();
    assert!(matches!(err, JobError::Busy));
    assert_eq!(
        fx.db.list_jobs_by_project("proj1").unwrap().len(),
        1,
        "refused before any row"
    );
    fx.sup.cancel(&job).await.unwrap();
    let next = fx.start("write", &[], SessionMode::Changes).await.unwrap();
    assert_eq!(fx.finished(&next).await.state, JobState::Succeeded);
}

#[test]
fn startup_marks_live_jobs_interrupted() {
    let fx = fx();
    fx.db.insert_job("a", "proj1", JobState::Running).unwrap();
    fx.db.insert_job("b", "proj1", JobState::Queued).unwrap();
    fx.db
        .transition_job(
            "b",
            &[JobState::Queued],
            JobState::Running,
            &JobUpdate::default(),
        )
        .unwrap();
    fx.db
        .transition_job(
            "b",
            &[JobState::Running],
            JobState::WaitingForApproval,
            &JobUpdate::default(),
        )
        .unwrap();
    fx.db.insert_job("c", "proj1", JobState::Succeeded).unwrap();
    assert_eq!(fx.sup.reconcile_on_startup().unwrap(), 2);
    let state = |id: &str| fx.db.get_job(id).unwrap().unwrap().state;
    assert_eq!(
        (state("a"), state("b"), state("c")),
        (
            JobState::Interrupted,
            JobState::Interrupted,
            JobState::Succeeded
        )
    );
}

#[tokio::test]
async fn failed_before_snapshot_means_no_spawn() {
    let fx = fx_with("/nonexistent/git");
    let argv = fx.file("argv.txt");
    let err = fx
        .start("write", &[("FAKE_ARGV", &argv)], SessionMode::Changes)
        .await
        .unwrap_err();
    assert!(matches!(err, JobError::Snapshot(_)), "{err}");
    let job = &fx.db.list_jobs_by_project("proj1").unwrap()[0];
    assert_eq!(job.state, JobState::Failed);
    assert!(job.error_message.as_deref().unwrap().contains("snapshot"));
    assert!(!Path::new(&argv).exists(), "the CLI was spawned");
    assert!(!fx.dir().join("out.txt").exists());
    assert!(fx.sup.inner.live().is_empty());
}

#[tokio::test]
async fn shutdown_kills_groups_and_marks_jobs_interrupted() {
    let fx = fx();
    let pidfile = fx.file("pid");
    let job = fx
        .start("hang", &[("FAKE_PIDFILE", &pidfile)], SessionMode::Changes)
        .await
        .unwrap();
    let pid = read_pid(&pidfile).await;
    fx.sup.shutdown().await;
    assert_eq!(
        fx.db.get_job(&job).unwrap().unwrap().state,
        JobState::Interrupted
    );
    assert!(gone(pid).await);
}

#[tokio::test]
async fn resume_passes_the_session_id_and_one_run_rules() {
    let fx = fx();
    let argv = fx.file("argv.txt");
    let resume = Resume {
        session_id: "old".into(),
        provider_session_id: "sess-9".into(),
        allowed_tools: vec!["Bash(touch approved.txt)".into()],
    };
    // The earlier session need not exist for the FK: resumed_from is plain text.
    let job = fx
        .sup
        .start_agent_run(
            &fx.project,
            fake_profile(&fx.exe, "write", &[("FAKE_ARGV", &argv)]),
            "again".into(),
            SessionMode::Changes,
            Some(resume),
            spawn_env(),
        )
        .await
        .unwrap();
    fx.finished(&job).await;
    let argv = fs::read_to_string(argv).unwrap();
    assert!(argv.contains("--resume\nsess-9"));
    assert!(argv.ends_with("--allowedTools\nBash(touch approved.txt)\n"));
    assert_eq!(fx.session(&job).resumed_from.as_deref(), Some("old"));
    // Nothing was stored on the profile.
    assert!(
        profiles::list(&fx.db).unwrap()[0]
            .env
            .iter()
            .all(|v| v.name != "allowedTools")
    );
}
