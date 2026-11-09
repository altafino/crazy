//! Agent commands (PRD M2 steps 8-9): profiles, runs, session replay and the changes of a
//! run. Same shape as the M1 commands: thin async wrappers, disk and DB work on the
//! blocking pool. Logic that needs testing lives in plain functions over `Db` and
//! `SnapshotStore`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;
use tokio::sync::broadcast::{Receiver, error::RecvError};

use super::{AppState, CommandError, blocking};
use crate::persistence::{Db, DbError, JobState, SessionMode, SessionRow};
use crate::services::agents::claude::ClaudeAdapter;
use crate::services::agents::profiles::{
    self, AgentProfile, ProbeResult, ProposedProfile, SaveError,
};
use crate::services::agents::{AgentAdapter, AgentEvent, AutoApprove, Decision};
use crate::services::diagnostics::ToolState;
use crate::services::git::{FileChange, FileDiff, GitError, RevertReport, SnapshotStore};
use crate::services::jobs::{AttachRequest, JobError, Resume};
use crate::services::settings::{self, ApprovalSettings};

// ------------------------------------------------------------------ errors

impl From<DbError> for CommandError {
    fn from(e: DbError) -> Self {
        Self::msg(e.to_string())
    }
}

impl From<GitError> for CommandError {
    fn from(e: GitError) -> Self {
        Self::msg(e.to_string())
    }
}

impl From<JobError> for CommandError {
    fn from(e: JobError) -> Self {
        Self::msg(e.to_string())
    }
}

impl From<SaveError> for CommandError {
    fn from(e: SaveError) -> Self {
        match e {
            SaveError::Invalid(field_errors) => Self {
                message: "Please fix the highlighted fields.".into(),
                field_errors,
            },
            SaveError::Db(e) => e.into(),
        }
    }
}

// ------------------------------------------------------------------ events

/// One event of one run. A view subscribes, then replays `get_session_events` and drops
/// anything with a `seq` it already has.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentRunEvent {
    pub job_id: String,
    pub event: AgentEvent,
}

/// Forwards every supervisor event to the UI. A lagging receiver just continues: the UI
/// replays the session log to fill any gap.
pub(crate) fn forward_events(app: tauri::AppHandle, mut rx: Receiver<(String, AgentEvent)>) {
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok((job_id, event)) => {
                    if let Err(e) = (AgentRunEvent { job_id, event }).emit(&app) {
                        eprintln!("could not emit agent event: {e}");
                    }
                }
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            }
        }
    });
}

// ---------------------------------------------------------------- profiles

fn save_profile(db: &Db, profile: &AgentProfile) -> Result<AgentProfile, CommandError> {
    Ok(profiles::save(db, profile)?)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_agent_profiles(
    state: State<'_, AppState>,
) -> Result<Vec<AgentProfile>, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(profiles::list(&st.db)?)).await
}

/// Validation problems (including secret-looking env, D40) come back as `fieldErrors`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn save_agent_profile(
    state: State<'_, AppState>,
    profile: AgentProfile,
) -> Result<AgentProfile, CommandError> {
    let st = state.inner().clone();
    blocking(move || save_profile(&st.db, &profile)).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn delete_agent_profile(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(profiles::delete(&st.db, &id)?)).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_agent_profile(
    state: State<'_, AppState>,
    profile: AgentProfile,
) -> Result<ProbeResult, CommandError> {
    Ok(ClaudeAdapter::default()
        .probe(&profile, &state.doctor.spawn_env())
        .await)
}

/// Pre-fills the first profile from the doctor (D33). Never saves.
#[tauri::command]
#[specta::specta]
pub(crate) async fn propose_agent_profile(
    state: State<'_, AppState>,
) -> Result<ProposedProfile, CommandError> {
    let st = state.inner().clone();
    let report = st.doctor_report(false).await;
    let claude = report.tools.iter().find_map(|t| match &t.state {
        ToolState::Installed { path, .. } if t.id == "claude" => Some(path.as_str()),
        _ => None,
    });
    let alias_env: Vec<String> = report
        .shell
        .aliases
        .iter()
        .filter(|a| a.tool == "claude")
        .flat_map(|a| a.env_assignments.clone())
        .collect();
    Ok(profiles::propose_default(
        claude,
        &alias_env,
        st.home.as_deref(),
    ))
}

// -------------------------------------------------------------------- runs

/// Override, then the project's default, then the global default. Must be enabled.
fn pick_profile(
    db: &Db,
    default_agent_id: Option<&str>,
    override_id: Option<&str>,
) -> Result<AgentProfile, CommandError> {
    let all = profiles::list(db)?;
    let by_id = |id: &str| all.iter().find(|p| p.id == id).cloned();
    let chosen = override_id
        .map(|id| {
            by_id(id)
                .ok_or_else(|| CommandError::msg("The selected agent profile no longer exists."))
        })
        .transpose()?
        .or_else(|| default_agent_id.and_then(by_id))
        .or(profiles::get_default(db)?)
        .ok_or_else(|| {
            CommandError::msg("No agent profile is set up yet. Add one in Settings > AI Agents.")
        })?;
    if !chosen.enabled {
        return Err(CommandError::msg(format!(
            "The agent profile \"{}\" is disabled.",
            chosen.name
        )));
    }
    Ok(chosen)
}

/// The composer's Auto-approve choice plus Settings > Approvals' "ask for risky" rule.
fn auto_policy(db: &Db, on: bool) -> Option<AutoApprove> {
    on.then(|| AutoApprove {
        ask_for_risky: settings::load_approval_settings(db).ask_for_risky,
    })
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn get_approval_settings(
    state: State<'_, AppState>,
) -> Result<ApprovalSettings, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(settings::load_approval_settings(&st.db))).await
}

/// Stores Settings > Approvals. New runs read it; a running run keeps its choice.
#[tauri::command]
#[specta::specta]
pub(crate) async fn set_approval_settings(
    state: State<'_, AppState>,
    approvals: ApprovalSettings,
) -> Result<ApprovalSettings, CommandError> {
    let st = state.inner().clone();
    blocking(move || {
        settings::save_approval_settings(&st.db, approvals)
            .map_err(|e| CommandError::msg(e.to_string()))
    })
    .await
}

/// D6/R1/S8: which session a new run continues, and whether it begins a conversation. The
/// newest session with a provider id wins, looking back no further than the last
/// "new conversation" start. A different profile (its own Claude config) can't resume it, so
/// the run starts fresh and says why. `sessions` is oldest first.
fn conversation_target(
    sessions: &[SessionRow],
    profile_id: &str,
    new_conversation: bool,
) -> (Option<Resume>, Option<&'static str>) {
    if new_conversation {
        return (None, Some("new"));
    }
    for s in sessions.iter().rev() {
        if let Some(provider_session_id) = &s.provider_session_id {
            return if s.profile_id.as_deref() == Some(profile_id) {
                let resume = Resume {
                    session_id: s.id.clone(),
                    provider_session_id: provider_session_id.clone(),
                    allowed_tools: Vec::new(),
                };
                (Some(resume), None)
            } else {
                (None, Some("profile"))
            };
        }
        if s.conversation_start.is_some() {
            break;
        }
    }
    (None, None)
}

/// Starts a run and returns its job id. Find the session with `list_sessions`. Unless
/// `new_conversation`, it continues the project's conversation (see `conversation_target`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)] // one IPC argument per composer field
pub(crate) async fn start_run(
    state: State<'_, AppState>,
    project_id: String,
    prompt: String,
    mode: SessionMode,
    profile_id: Option<String>,
    draft_id: Option<String>,
    attachment_ids: Option<Vec<String>>,
    auto_approve: bool,
    new_conversation: bool,
) -> Result<String, CommandError> {
    let st = state.inner().clone();
    if prompt.trim().is_empty() {
        return Err(CommandError {
            message: "Please fix the highlighted fields.".into(),
            field_errors: vec![crate::domain::FieldError {
                field: "prompt".into(),
                message: "Tell the agent what to do".into(),
            }],
        });
    }
    let db = st.db.clone();
    let (project, profile, auto, (resume, start)) = blocking(move || {
        let project = db
            .get_project(&project_id)?
            .ok_or_else(|| CommandError::msg("Project not found."))?;
        let profile = pick_profile(
            &db,
            project.default_agent_id.as_deref(),
            profile_id.as_deref(),
        )?;
        let target = conversation_target(
            &db.list_sessions_by_project(&project.id)?,
            &profile.id,
            new_conversation,
        );
        Ok((project, profile, auto_policy(&db, auto_approve), target))
    })
    .await?;
    let job_id = st
        .jobs()
        .start_agent_run(
            &project,
            profile,
            prompt,
            mode,
            resume,
            draft_id
                .zip(attachment_ids)
                .map(|(draft_id, item_ids)| AttachRequest { draft_id, item_ids }),
            auto,
            st.doctor.spawn_env(),
        )
        .await?;
    if let Some(kind) = start {
        let (db, id) = (st.db.clone(), job_id.clone());
        blocking(move || Ok(db.set_conversation_start(&id, kind)?)).await?;
    }
    Ok(job_id)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn cancel_run(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<(), CommandError> {
    Ok(state.jobs().cancel(&job_id).await?)
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum ApprovalChoice {
    Allow,
    Deny { message: Option<String> },
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn answer_approval(
    state: State<'_, AppState>,
    job_id: String,
    request_id: String,
    choice: ApprovalChoice,
) -> Result<(), CommandError> {
    let decision = match choice {
        ApprovalChoice::Allow => Decision::Allow,
        ApprovalChoice::Deny { message } => {
            Decision::Deny(message.filter(|m| !m.trim().is_empty()))
        }
    };
    Ok(state
        .jobs()
        .answer_approval(&job_id, &request_id, decision)?)
}

fn read_events(log_path: &Path) -> Vec<AgentEvent> {
    // A run that failed before starting has no log; an unreadable line is skipped.
    std::fs::read_to_string(log_path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// The exact rules the CLI refused in the session's last completed run.
fn denied_rules(events: &[AgentEvent]) -> Vec<String> {
    events
        .iter()
        .rev()
        .find_map(|e| match e {
            AgentEvent::Completed { denials, .. } => {
                Some(denials.iter().filter_map(|d| d.rule.clone()).collect())
            }
            _ => None,
        })
        .unwrap_or_default()
}

/// "Allow and retry": resumes the session with exactly one denied tool rule permitted for
/// that run only. The rule must be one the last run was actually denied.
#[tauri::command]
#[specta::specta]
pub(crate) async fn allow_and_retry(
    state: State<'_, AppState>,
    session_id: String,
    rule: String,
) -> Result<String, CommandError> {
    let st = state.inner().clone();
    let db = st.db.clone();
    let (project, profile, mode, resume) = blocking(move || {
        let session = get_session(&db, &session_id)?;
        if !denied_rules(&read_events(Path::new(&session.log_path))).contains(&rule) {
            return Err(CommandError::msg("That action was not denied in this run."));
        }
        let provider_session_id = session
            .provider_session_id
            .ok_or_else(|| CommandError::msg("This session can't be resumed."))?;
        let project = db
            .get_project(&session.project_id)?
            .ok_or_else(|| CommandError::msg("Project not found."))?;
        let profile = pick_profile(&db, None, session.profile_id.as_deref())?;
        let resume = Resume {
            session_id: session.id,
            provider_session_id,
            allowed_tools: vec![rule],
        };
        Ok((project, profile, session.mode, resume))
    })
    .await?;
    Ok(st
        .jobs()
        .start_agent_run(
            &project,
            profile,
            "That action is now allowed. Please continue with the task.".into(),
            mode,
            Some(resume),
            None,
            None,
            st.doctor.spawn_env(),
        )
        .await?)
}

/// The resume for "Make these changes": only a Plan first session with a provider id.
fn plan_resume(session: SessionRow) -> Result<Resume, CommandError> {
    if session.mode != SessionMode::Plan {
        return Err(CommandError::msg("Only a plan can be turned into changes."));
    }
    let provider_session_id = session
        .provider_session_id
        .ok_or_else(|| CommandError::msg("This session can't be resumed."))?;
    Ok(Resume {
        session_id: session.id,
        provider_session_id,
        allowed_tools: Vec::new(),
    })
}

/// "Make these changes" (D53): a Make changes run that resumes the Plan first session.
/// Refused when the session is not a plan, can't be resumed, or another run is active.
#[tauri::command]
#[specta::specta]
pub(crate) async fn continue_plan(
    state: State<'_, AppState>,
    session_id: String,
    auto_approve: bool,
) -> Result<String, CommandError> {
    let st = state.inner().clone();
    let db = st.db.clone();
    let auto = auto_policy(&db, auto_approve);
    let (project, profile, resume) = blocking(move || {
        let session = get_session(&db, &session_id)?;
        let project = db
            .get_project(&session.project_id)?
            .ok_or_else(|| CommandError::msg("Project not found."))?;
        let profile = pick_profile(&db, None, session.profile_id.as_deref())?;
        Ok((project, profile, plan_resume(session)?))
    })
    .await?;
    Ok(st
        .jobs()
        .start_agent_run(
            &project,
            profile,
            "Go ahead and implement the plan above.".into(),
            SessionMode::Changes,
            Some(resume),
            None,
            auto,
            st.doctor.spawn_env(),
        )
        .await?)
}

// ---------------------------------------------------------------- sessions

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionSummary {
    pub id: String,
    pub job_id: String,
    pub project_id: String,
    pub profile_id: Option<String>,
    pub mode: SessionMode,
    pub prompt: String,
    pub resumed_from: Option<String>,
    /// `new` or `profile` when the run began a new conversation (the caption says which).
    pub conversation_start: Option<String>,
    /// The resume failed before Claude started; the run was respawned fresh (R2).
    pub resume_fell_back: bool,
    pub created_at: String,
    /// The agent's own id exists, so "Allow and retry" can resume this session.
    pub can_resume: bool,
    /// BEFORE snapshot exists, so a diff is possible.
    pub has_snapshot: bool,
    /// BEFORE was recorded but its snapshot is gone (pruned past the last 20 runs, or deleted).
    pub snapshot_pruned: bool,
    /// The run ended normally. Without it, Revert works from BEFORE vs the current files.
    pub has_after: bool,
    /// `refs/studio/checkpoints/<run id>` in the project's own repository, when recorded.
    pub checkpoint_ref: Option<String>,
    pub job_state: JobState,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error_kind: Option<String>,
    pub error_message: Option<String>,
}

/// Newest first, each with its job's state.
fn session_summaries(
    db: &Db,
    store: &SnapshotStore,
    project_id: &str,
) -> Result<Vec<SessionSummary>, DbError> {
    let jobs: HashMap<_, _> = db
        .list_jobs_by_project(project_id)?
        .into_iter()
        .map(|j| (j.id.clone(), j))
        .collect();
    let sessions = db.list_sessions_by_project(project_id)?;
    // One git call per project. If it fails, don't claim anything was pruned.
    let live = sessions
        .iter()
        .any(|s| s.before_ref.is_some())
        .then(|| store.live_commits(project_id).ok())
        .flatten();
    let mut out: Vec<_> = sessions
        .into_iter()
        .filter_map(|s| {
            let j = jobs.get(&s.job_id)?;
            Some(SessionSummary {
                can_resume: s.provider_session_id.is_some(),
                has_snapshot: s.before_ref.is_some(),
                snapshot_pruned: match (&s.before_ref, &live) {
                    (Some(b), Some(live)) => !live.contains(b),
                    _ => false,
                },
                has_after: s.after_ref.is_some(),
                job_state: j.state,
                started_at: j.started_at.clone(),
                completed_at: j.completed_at.clone(),
                error_kind: j.error_kind.clone(),
                error_message: j.error_message.clone(),
                id: s.id,
                job_id: s.job_id,
                project_id: s.project_id,
                profile_id: s.profile_id,
                mode: s.mode,
                prompt: s.prompt,
                resumed_from: s.resumed_from,
                conversation_start: s.conversation_start,
                resume_fell_back: s.resume_fell_back,
                created_at: s.created_at,
                checkpoint_ref: s.checkpoint_ref,
            })
        })
        .collect();
    out.reverse();
    Ok(out)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_sessions(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<SessionSummary>, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(session_summaries(&st.db, &st.snapshots, &project_id)?)).await
}

fn get_session(db: &Db, id: &str) -> Result<SessionRow, CommandError> {
    db.get_session(id)?
        .ok_or_else(|| CommandError::msg("Session not found."))
}

/// The session log, in order, for subscribe-then-replay (dedupe by `seq` in the view).
#[tauri::command]
#[specta::specta]
pub(crate) async fn get_session_events(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<AgentEvent>, CommandError> {
    let st = state.inner().clone();
    blocking(move || {
        let s = get_session(&st.db, &session_id)?;
        Ok(read_events(&PathBuf::from(s.log_path)))
    })
    .await
}

// ----------------------------------------------------------------- changes

/// The session's BEFORE ref and its project dir. Changes can only be read or reverted when
/// no run is touching the files.
fn changes_ctx(db: &Db, session: &SessionRow) -> Result<(String, PathBuf), CommandError> {
    let job = db
        .get_job(&session.job_id)?
        .ok_or_else(|| CommandError::msg("Run not found."))?;
    if !job.state.is_terminal() || db.has_running_modifying_job(&session.project_id)? {
        return Err(CommandError::msg("A run is still active in this project."));
    }
    let before = session.before_ref.clone().ok_or_else(|| {
        CommandError::msg("This run has no snapshot, so its changes can't be shown.")
    })?;
    let project = db
        .get_project(&session.project_id)?
        .ok_or_else(|| CommandError::msg("Project not found."))?;
    Ok((before, PathBuf::from(project.workspace_path)))
}

/// AFTER of the run, or for a run that never reached one (interrupted, cancelled) a fresh
/// snapshot of the current files. ponytail: the fresh one is not stored on the session;
/// each call re-snapshots, which is fine for a viewer that fetches the diff once.
fn after_commit(
    store: &SnapshotStore,
    session: &SessionRow,
    dir: &Path,
) -> Result<String, CommandError> {
    if let Some(a) = &session.after_ref {
        return Ok(a.clone());
    }
    Ok(store
        .snapshot(
            &session.project_id,
            dir,
            &session.job_id,
            crate::services::git::Phase::After,
            "current files",
        )?
        .commit)
}

fn session_diff(
    db: &Db,
    store: &SnapshotStore,
    session_id: &str,
) -> Result<Vec<FileChange>, CommandError> {
    let s = get_session(db, session_id)?;
    let (before, dir) = changes_ctx(db, &s)?;
    let after = after_commit(store, &s, &dir)?;
    Ok(store.diff(&s.project_id, &before, &after)?)
}

fn session_file_diff(
    db: &Db,
    store: &SnapshotStore,
    session_id: &str,
    path: &str,
    full: bool,
) -> Result<FileDiff, CommandError> {
    let s = get_session(db, session_id)?;
    let (before, dir) = changes_ctx(db, &s)?;
    let after = after_commit(store, &s, &dir)?;
    Ok(store.file_diff(&s.project_id, &before, &after, path, full)?)
}

/// With an AFTER the normal revert applies; without one (interrupted run) the current
/// files stand in for it and only the selected paths are restored.
fn revert_session(
    db: &Db,
    store: &SnapshotStore,
    session_id: &str,
    selected: &[String],
) -> Result<RevertReport, CommandError> {
    let s = get_session(db, session_id)?;
    let (before, dir) = changes_ctx(db, &s)?;
    Ok(match &s.after_ref {
        Some(after) => store.revert(&s.project_id, &dir, &before, after, selected)?,
        None => store.revert_interrupted(&s.project_id, &dir, &before, selected)?,
    })
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn run_diff(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<FileChange>, CommandError> {
    let st = state.inner().clone();
    blocking(move || session_diff(&st.db, &st.snapshots, &session_id)).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn run_file_diff(
    state: State<'_, AppState>,
    session_id: String,
    path: String,
    full: bool,
) -> Result<FileDiff, CommandError> {
    let st = state.inner().clone();
    blocking(move || session_file_diff(&st.db, &st.snapshots, &session_id, &path, full)).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn revert_run(
    state: State<'_, AppState>,
    session_id: String,
    selected: Vec<String>,
) -> Result<RevertReport, CommandError> {
    let st = state.inner().clone();
    blocking(move || revert_session(&st.db, &st.snapshots, &session_id, &selected)).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn delete_all_snapshots(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<(), CommandError> {
    let st = state.inner().clone();
    blocking(move || {
        if st.db.has_running_modifying_job(&project_id)? {
            return Err(CommandError::msg("A run is still active in this project."));
        }
        Ok(st.snapshots.delete_all(&project_id)?)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::TemplateId;
    use crate::persistence::{JobUpdate, NewProject};
    use crate::services::agents::profiles::EnvVar;
    use crate::services::git::{GitRunner, Phase};
    use std::fs;

    fn profile(env: Vec<EnvVar>) -> AgentProfile {
        AgentProfile {
            id: String::new(),
            name: "Claude".into(),
            provider: "claude".into(),
            executable: "/bin/true".into(),
            env,
            model: None,
            inactivity_secs: 600,
            instructions: "x".into(),
            is_default: true,
            enabled: true,
        }
    }

    #[test]
    fn save_profile_surfaces_field_errors_for_secrets() {
        let db = Db::open_in_memory().unwrap();
        let bad = profile(vec![EnvVar {
            name: "ANTHROPIC_API_KEY".into(),
            value: "sk-ant-abc".into(),
        }]);
        let e = save_profile(&db, &bad).unwrap_err();
        assert!(!e.field_errors.is_empty());
        assert!(profiles::list(&db).unwrap().is_empty());
        assert!(save_profile(&db, &profile(vec![])).is_ok());
    }

    struct Fx {
        _tmp: tempfile::TempDir,
        db: Db,
        store: SnapshotStore,
        dir: PathBuf,
    }

    /// A project with one finished run whose BEFORE is taken; AFTER only when asked.
    fn fx(with_after: bool) -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("a.txt"), "one\n").unwrap();
        let db = Db::open_in_memory().unwrap();
        db.insert_project(&NewProject {
            id: "p1".into(),
            name: "p".into(),
            slug: "p".into(),
            template_id: TemplateId::TauriVue,
            app_id: "com.x.p".into(),
            workspace_path: dir.display().to_string(),
            pending_op: None,
        })
        .unwrap();
        let store = SnapshotStore::new(
            &tmp.path().join("data"),
            GitRunner {
                program: "git".into(),
                env: vec![
                    ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
                    ("HOME".into(), tmp.path().display().to_string()),
                    ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
                    ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
                ],
            },
        );
        db.insert_job("j1", "p1", JobState::Queued).unwrap();
        let before = store
            .snapshot("p1", &dir, "j1", Phase::Before, "b")
            .unwrap();
        fs::write(dir.join("a.txt"), "two\n").unwrap(); // the agent's edit
        let after =
            with_after.then(|| store.snapshot("p1", &dir, "j1", Phase::After, "a").unwrap());
        db.insert_session(&SessionRow {
            id: "s1".into(),
            job_id: "j1".into(),
            project_id: "p1".into(),
            profile_id: None,
            mode: SessionMode::Changes,
            prompt: "x".into(),
            provider_session_id: None,
            resumed_from: None,
            log_path: String::new(),
            before_ref: Some(before.commit.clone()),
            after_ref: after.map(|a| a.commit),
            checkpoint_ref: None,
            conversation_start: None,
            resume_fell_back: false,
            created_at: String::new(),
        })
        .unwrap();
        let end = if with_after {
            JobState::Succeeded
        } else {
            JobState::Interrupted
        };
        db.transition_job("j1", &[JobState::Queued], end, &JobUpdate::default())
            .unwrap();
        Fx {
            _tmp: tmp,
            db,
            store,
            dir,
        }
    }

    #[test]
    fn revert_with_after_restores_the_run_edit() {
        let f = fx(true);
        let changes = session_diff(&f.db, &f.store, "s1").unwrap();
        assert_eq!(changes.len(), 1);
        let r = revert_session(&f.db, &f.store, "s1", &["a.txt".into()]).unwrap();
        assert_eq!(r.restored, vec!["a.txt"]);
        assert_eq!(fs::read_to_string(f.dir.join("a.txt")).unwrap(), "one\n");
    }

    #[test]
    fn summary_flags_pruned_snapshots() {
        let f = fx(true);
        let s = session_summaries(&f.db, &f.store, "p1").unwrap();
        assert!(s[0].has_snapshot && !s[0].snapshot_pruned);
        f.store.delete_all("p1").unwrap();
        let s = session_summaries(&f.db, &f.store, "p1").unwrap();
        assert!(s[0].has_snapshot && s[0].snapshot_pruned);
    }

    #[test]
    fn revert_without_after_uses_the_interrupted_path() {
        let f = fx(false);
        // The diff works too: current files stand in for the missing AFTER.
        assert_eq!(session_diff(&f.db, &f.store, "s1").unwrap().len(), 1);
        let r = revert_session(&f.db, &f.store, "s1", &["a.txt".into()]).unwrap();
        assert_eq!(r.restored, vec!["a.txt"]);
        assert_eq!(fs::read_to_string(f.dir.join("a.txt")).unwrap(), "one\n");
    }

    #[test]
    fn changes_are_refused_while_the_run_is_live() {
        let f = fx(true);
        f.db.insert_job("j2", "p1", JobState::Running).unwrap();
        f.db.insert_session(&SessionRow {
            id: "s2".into(),
            job_id: "j2".into(),
            project_id: "p1".into(),
            profile_id: None,
            mode: SessionMode::Changes,
            prompt: "x".into(),
            provider_session_id: None,
            resumed_from: None,
            log_path: String::new(),
            before_ref: Some("0".repeat(40)),
            after_ref: None,
            checkpoint_ref: None,
            conversation_start: None,
            resume_fell_back: false,
            created_at: String::new(),
        })
        .unwrap();
        assert!(revert_session(&f.db, &f.store, "s2", &[]).is_err());
        assert!(revert_session(&f.db, &f.store, "s1", &[]).is_err());
    }

    #[test]
    fn continuing_resumes_the_newest_same_profile_session_after_the_last_new_conversation() {
        let row =
            |id: &str, profile: &str, provider: Option<&str>, start: Option<&str>| SessionRow {
                id: id.into(),
                job_id: format!("j-{id}"),
                project_id: "p1".into(),
                profile_id: Some(profile.into()),
                mode: SessionMode::Changes,
                prompt: "x".into(),
                provider_session_id: provider.map(Into::into),
                resumed_from: None,
                log_path: String::new(),
                before_ref: None,
                after_ref: None,
                checkpoint_ref: None,
                conversation_start: start.map(Into::into),
                resume_fell_back: false,
                created_at: String::new(),
            };
        let pick = |rows: &[SessionRow], profile: &str, new: bool| {
            let (r, start) = conversation_target(rows, profile, new);
            (r.map(|r| r.session_id), start)
        };
        assert_eq!(pick(&[], "a", false), (None, None));
        let rows = [
            row("s1", "a", Some("prov-1"), None),
            // The newest run failed before Claude started: the one before it is the target.
            row("s2", "a", None, None),
        ];
        assert_eq!(pick(&rows, "a", false), (Some("s1".into()), None));
        assert_eq!(pick(&rows, "a", true), (None, Some("new")));
        // Another profile has another Claude config: fresh, and the caption says why.
        assert_eq!(pick(&rows, "b", false), (None, Some("profile")));
        // Nothing before a new-conversation start is reachable, a failed start included.
        let rows = [
            row("s1", "a", Some("prov-1"), None),
            row("s2", "a", None, Some("new")),
        ];
        assert_eq!(pick(&rows, "a", false), (None, None));
        let rows = [
            row("s1", "a", Some("prov-1"), None),
            row("s2", "a", Some("prov-2"), Some("new")),
            row("s3", "a", Some("prov-3"), None),
        ];
        assert_eq!(pick(&rows, "a", false), (Some("s3".into()), None));
    }

    #[test]
    fn continue_plan_needs_a_resumable_plan_session() {
        let row = |mode, provider: Option<&str>| SessionRow {
            id: "s1".into(),
            job_id: "j1".into(),
            project_id: "p1".into(),
            profile_id: None,
            mode,
            prompt: "x".into(),
            provider_session_id: provider.map(Into::into),
            resumed_from: None,
            log_path: String::new(),
            before_ref: None,
            after_ref: None,
            checkpoint_ref: None,
            conversation_start: None,
            resume_fell_back: false,
            created_at: String::new(),
        };
        let r = plan_resume(row(SessionMode::Plan, Some("abc"))).unwrap();
        assert_eq!(
            (r.session_id.as_str(), r.provider_session_id.as_str()),
            ("s1", "abc")
        );
        assert!(r.allowed_tools.is_empty());
        assert!(plan_resume(row(SessionMode::Changes, Some("abc"))).is_err());
        assert!(plan_resume(row(SessionMode::Plan, None)).is_err());
    }
}
