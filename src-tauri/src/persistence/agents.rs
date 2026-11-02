//! Typed access to `agent_profiles`, `jobs` and `agent_sessions` (D30).

use rusqlite::types::Type as SqlType;
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{Db, DbError};

const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ','now')";

fn bad(col: usize, what: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(col, SqlType::Text, what.into())
}

// ---------------------------------------------------------------- profiles

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProfileRow {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub executable: String,
    pub env: Vec<(String, String)>,
    pub model: Option<String>,
    pub inactivity_secs: u32,
    pub instructions: String,
    pub is_default: bool,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

const PROFILE_COLS: &str = "id, name, provider, executable, env_json, model, inactivity_secs, \
                            instructions, is_default, enabled, created_at, updated_at";

fn profile_row(r: &Row<'_>) -> rusqlite::Result<ProfileRow> {
    let env_json: String = r.get(4)?;
    Ok(ProfileRow {
        id: r.get(0)?,
        name: r.get(1)?,
        provider: r.get(2)?,
        executable: r.get(3)?,
        env: serde_json::from_str(&env_json).map_err(|e| bad(4, format!("env_json: {e}")))?,
        model: r.get(5)?,
        inactivity_secs: r.get(6)?,
        instructions: r.get(7)?,
        is_default: r.get(8)?,
        enabled: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
    })
}

// -------------------------------------------------------------------- jobs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum JobState {
    Queued,
    Running,
    WaitingForApproval,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

impl JobState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::WaitingForApproval => "waiting_for_approval",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "waiting_for_approval" => Self::WaitingForApproval,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "interrupted" => Self::Interrupted,
            _ => return None,
        })
    }

    pub(crate) fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct JobRow {
    pub id: String,
    pub project_id: String,
    pub kind: String,
    pub state: JobState,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub exit_code: Option<i32>,
    pub error_kind: Option<String>,
    pub error_message: Option<String>,
}

/// Columns a state transition may set alongside `state`; timestamps come from the database.
#[derive(Debug, Clone, Default)]
pub(crate) struct JobUpdate {
    pub set_started: bool,
    pub set_completed: bool,
    pub exit_code: Option<i32>,
    pub error_kind: Option<String>,
    pub error_message: Option<String>,
}

const JOB_COLS: &str = "id, project_id, kind, state, started_at, completed_at, exit_code, \
                        error_kind, error_message";

fn job_row(r: &Row<'_>) -> rusqlite::Result<JobRow> {
    let state: String = r.get(3)?;
    Ok(JobRow {
        id: r.get(0)?,
        project_id: r.get(1)?,
        kind: r.get(2)?,
        state: JobState::parse(&state).ok_or_else(|| bad(3, format!("unknown state {state}")))?,
        started_at: r.get(4)?,
        completed_at: r.get(5)?,
        exit_code: r.get(6)?,
        error_kind: r.get(7)?,
        error_message: r.get(8)?,
    })
}

// ---------------------------------------------------------------- sessions

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SessionMode {
    Plan,
    Changes,
}

impl SessionMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Changes => "changes",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SessionRow {
    pub id: String,
    pub job_id: String,
    pub project_id: String,
    pub profile_id: Option<String>,
    pub mode: SessionMode,
    pub prompt: String,
    pub provider_session_id: Option<String>,
    pub resumed_from: Option<String>,
    pub log_path: String,
    pub before_ref: Option<String>,
    pub after_ref: Option<String>,
    pub created_at: String,
}

const SESSION_COLS: &str = "id, job_id, project_id, profile_id, mode, prompt, \
                            provider_session_id, resumed_from, log_path, before_ref, after_ref, \
                            created_at";

fn session_row(r: &Row<'_>) -> rusqlite::Result<SessionRow> {
    let mode: String = r.get(4)?;
    Ok(SessionRow {
        id: r.get(0)?,
        job_id: r.get(1)?,
        project_id: r.get(2)?,
        profile_id: r.get(3)?,
        mode: match mode.as_str() {
            "plan" => SessionMode::Plan,
            "changes" => SessionMode::Changes,
            other => return Err(bad(4, format!("unknown mode {other}"))),
        },
        prompt: r.get(5)?,
        provider_session_id: r.get(6)?,
        resumed_from: r.get(7)?,
        log_path: r.get(8)?,
        before_ref: r.get(9)?,
        after_ref: r.get(10)?,
        created_at: r.get(11)?,
    })
}

impl Db {
    // ---- profiles

    /// Inserts or updates by id. When `is_default`, every other profile is unset in the
    /// same transaction, so at most one default exists.
    pub(crate) fn save_profile(&self, p: &ProfileRow) -> Result<(), DbError> {
        let env_json =
            serde_json::to_string(&p.env).map_err(|e| DbError::Corrupt(e.to_string()))?;
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        if p.is_default {
            tx.execute(
                "UPDATE agent_profiles SET is_default = 0 WHERE id != ?1",
                [&p.id],
            )?;
        }
        tx.execute(
            &format!(
                "INSERT INTO agent_profiles (id, name, provider, executable, env_json, model,
                        inactivity_secs, instructions, is_default, enabled, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, {NOW}, {NOW})
                 ON CONFLICT(id) DO UPDATE SET name=excluded.name, provider=excluded.provider,
                    executable=excluded.executable, env_json=excluded.env_json,
                    model=excluded.model, inactivity_secs=excluded.inactivity_secs,
                    instructions=excluded.instructions, is_default=excluded.is_default,
                    enabled=excluded.enabled, updated_at={NOW}"
            ),
            params![
                p.id,
                p.name,
                p.provider,
                p.executable,
                env_json,
                p.model,
                p.inactivity_secs,
                p.instructions,
                p.is_default,
                p.enabled
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    #[allow(dead_code)] // by-id lookup; commands resolve profiles through `list`
    pub(crate) fn get_profile(&self, id: &str) -> Result<Option<ProfileRow>, DbError> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {PROFILE_COLS} FROM agent_profiles WHERE id = ?1"),
                [id],
                profile_row,
            )
            .optional()?)
    }

    pub(crate) fn get_default_profile(&self) -> Result<Option<ProfileRow>, DbError> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {PROFILE_COLS} FROM agent_profiles WHERE is_default = 1"),
                [],
                profile_row,
            )
            .optional()?)
    }

    pub(crate) fn list_profiles(&self) -> Result<Vec<ProfileRow>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {PROFILE_COLS} FROM agent_profiles ORDER BY created_at, rowid"
        ))?;
        Ok(stmt.query_map([], profile_row)?.collect::<Result<_, _>>()?)
    }

    /// Deletes the profile and clears `projects.default_agent_id` where it pointed at it
    /// (that reference has no SQL foreign key; see migration 0002). Sessions keep their row
    /// with `profile_id` set NULL by the FK.
    pub(crate) fn delete_profile(&self, id: &str) -> Result<(), DbError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE projects SET default_agent_id = NULL WHERE default_agent_id = ?1",
            [id],
        )?;
        tx.execute("DELETE FROM agent_profiles WHERE id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    // ---- jobs

    /// Inserts a job; `started_at` is stamped when it starts out `running`.
    pub(crate) fn insert_job(
        &self,
        id: &str,
        project_id: &str,
        state: JobState,
    ) -> Result<(), DbError> {
        self.conn().execute(
            &format!(
                "INSERT INTO jobs (id, project_id, state, started_at)
                 VALUES (?1, ?2, ?3, CASE WHEN ?3 = 'running' THEN {NOW} END)"
            ),
            params![id, project_id, state.as_str()],
        )?;
        Ok(())
    }

    /// One guarded UPDATE: applies only if the job is currently in one of `from`.
    /// Returns whether it applied.
    pub(crate) fn transition_job(
        &self,
        id: &str,
        from: &[JobState],
        to: JobState,
        u: &JobUpdate,
    ) -> Result<bool, DbError> {
        let states = from
            .iter()
            .map(|s| format!("'{}'", s.as_str()))
            .collect::<Vec<_>>()
            .join(",");
        let n = self.conn().execute(
            &format!(
                "UPDATE jobs SET state = ?2,
                    started_at = CASE WHEN ?3 THEN {NOW} ELSE started_at END,
                    completed_at = CASE WHEN ?4 THEN {NOW} ELSE completed_at END,
                    exit_code = COALESCE(?5, exit_code),
                    error_kind = COALESCE(?6, error_kind),
                    error_message = COALESCE(?7, error_message)
                 WHERE id = ?1 AND state IN ({states})"
            ),
            params![
                id,
                to.as_str(),
                u.set_started,
                u.set_completed,
                u.exit_code,
                u.error_kind,
                u.error_message
            ],
        )?;
        Ok(n == 1)
    }

    pub(crate) fn get_job(&self, id: &str) -> Result<Option<JobRow>, DbError> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {JOB_COLS} FROM jobs WHERE id = ?1"),
                [id],
                job_row,
            )
            .optional()?)
    }

    pub(crate) fn list_jobs_by_project(&self, project_id: &str) -> Result<Vec<JobRow>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {JOB_COLS} FROM jobs WHERE project_id = ?1 ORDER BY started_at, rowid"
        ))?;
        Ok(stmt
            .query_map([project_id], job_row)?
            .collect::<Result<_, _>>()?)
    }

    /// Startup recovery: nothing survives an app restart, so live jobs become `interrupted`.
    /// Returns how many were changed.
    pub(crate) fn mark_interrupted_on_startup(&self) -> Result<usize, DbError> {
        Ok(self.conn().execute(
            &format!(
                "UPDATE jobs SET state = 'interrupted', completed_at = {NOW}
                 WHERE state IN ('running', 'waiting_for_approval')"
            ),
            [],
        )?)
    }

    /// True when the project has a live job whose session is in `changes` mode.
    pub(crate) fn has_running_modifying_job(&self, project_id: &str) -> Result<bool, DbError> {
        Ok(self.conn().query_row(
            "SELECT EXISTS (
                SELECT 1 FROM jobs j JOIN agent_sessions s ON s.job_id = j.id
                WHERE j.project_id = ?1 AND s.mode = 'changes'
                  AND j.state IN ('running', 'waiting_for_approval'))",
            [project_id],
            |r| r.get(0),
        )?)
    }

    // ---- sessions

    pub(crate) fn insert_session(&self, s: &SessionRow) -> Result<(), DbError> {
        self.conn().execute(
            &format!(
                "INSERT INTO agent_sessions (id, job_id, project_id, profile_id, mode, prompt,
                        provider_session_id, resumed_from, log_path, before_ref, after_ref,
                        created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, {NOW})"
            ),
            params![
                s.id,
                s.job_id,
                s.project_id,
                s.profile_id,
                s.mode.as_str(),
                s.prompt,
                s.provider_session_id,
                s.resumed_from,
                s.log_path,
                s.before_ref,
                s.after_ref
            ],
        )?;
        Ok(())
    }

    pub(crate) fn get_session(&self, id: &str) -> Result<Option<SessionRow>, DbError> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {SESSION_COLS} FROM agent_sessions WHERE id = ?1"),
                [id],
                session_row,
            )
            .optional()?)
    }

    pub(crate) fn list_sessions_by_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<SessionRow>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {SESSION_COLS} FROM agent_sessions WHERE project_id = ?1
             ORDER BY created_at, rowid"
        ))?;
        Ok(stmt
            .query_map([project_id], session_row)?
            .collect::<Result<_, _>>()?)
    }

    pub(crate) fn set_provider_session_id(
        &self,
        id: &str,
        provider_session_id: &str,
    ) -> Result<(), DbError> {
        self.conn().execute(
            "UPDATE agent_sessions SET provider_session_id = ?2 WHERE id = ?1",
            params![id, provider_session_id],
        )?;
        Ok(())
    }

    /// Sets whichever refs are `Some`; `None` leaves the stored value alone.
    pub(crate) fn set_session_refs(
        &self,
        id: &str,
        before_ref: Option<&str>,
        after_ref: Option<&str>,
    ) -> Result<(), DbError> {
        self.conn().execute(
            "UPDATE agent_sessions SET before_ref = COALESCE(?2, before_ref),
                    after_ref = COALESCE(?3, after_ref) WHERE id = ?1",
            params![id, before_ref, after_ref],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::TemplateId;
    use crate::persistence::NewProject;

    fn db_with_project() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.insert_project(&NewProject {
            id: "p1".into(),
            name: "a".into(),
            slug: "a".into(),
            template_id: TemplateId::TauriVue,
            app_id: "com.x.a".into(),
            workspace_path: "/p/a".into(),
            pending_op: None,
        })
        .unwrap();
        db
    }

    fn profile(id: &str, default: bool) -> ProfileRow {
        ProfileRow {
            id: id.into(),
            name: id.into(),
            provider: "claude".into(),
            executable: "/usr/bin/claude".into(),
            env: vec![("CLAUDE_CONFIG_DIR".into(), "/h/.claude-a".into())],
            model: None,
            inactivity_secs: 600,
            instructions: "rules".into(),
            is_default: default,
            enabled: true,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn session(id: &str, job: &str, mode: SessionMode) -> SessionRow {
        SessionRow {
            id: id.into(),
            job_id: job.into(),
            project_id: "p1".into(),
            profile_id: None,
            mode,
            prompt: "do it".into(),
            provider_session_id: None,
            resumed_from: None,
            log_path: "/l".into(),
            before_ref: None,
            after_ref: None,
            created_at: String::new(),
        }
    }

    #[test]
    fn transition_applies_only_from_allowed_states() {
        let db = db_with_project();
        db.insert_job("j", "p1", JobState::Queued).unwrap();
        assert!(db.get_job("j").unwrap().unwrap().started_at.is_none());
        let start = JobUpdate {
            set_started: true,
            ..Default::default()
        };
        assert!(
            !db.transition_job("j", &[JobState::Running], JobState::Succeeded, &start)
                .unwrap()
        );
        assert!(
            db.transition_job("j", &[JobState::Queued], JobState::Running, &start)
                .unwrap()
        );
        let done = JobUpdate {
            set_completed: true,
            exit_code: Some(0),
            ..Default::default()
        };
        assert!(
            db.transition_job(
                "j",
                &[JobState::Running, JobState::WaitingForApproval],
                JobState::Succeeded,
                &done
            )
            .unwrap()
        );
        // A late cancel must not overwrite the terminal state.
        assert!(
            !db.transition_job(
                "j",
                &[JobState::Queued, JobState::Running],
                JobState::Cancelled,
                &JobUpdate::default()
            )
            .unwrap()
        );
        let j = db.get_job("j").unwrap().unwrap();
        assert_eq!(j.state, JobState::Succeeded);
        assert!(j.started_at.is_some() && j.completed_at.is_some());
        assert_eq!(j.exit_code, Some(0));
        assert!(j.state.is_terminal());
    }

    #[test]
    fn check_constraints_reject_bad_values() {
        let db = db_with_project();
        let bad_state = db.conn().execute(
            "INSERT INTO jobs (id, project_id, state) VALUES ('j', 'p1', 'bogus')",
            [],
        );
        assert!(bad_state.is_err());
        db.insert_job("j", "p1", JobState::Running).unwrap();
        let bad_mode = db.conn().execute(
            "INSERT INTO agent_sessions (id, job_id, project_id, mode, prompt, log_path, created_at)
             VALUES ('s', 'j', 'p1', 'bogus', 'x', '/l', 'now')",
            [],
        );
        assert!(bad_mode.is_err());
    }

    #[test]
    fn project_delete_cascades_to_jobs_and_sessions() {
        let db = db_with_project();
        db.insert_job("j", "p1", JobState::Running).unwrap();
        db.insert_session(&session("s", "j", SessionMode::Plan))
            .unwrap();
        db.delete_project("p1").unwrap();
        assert!(db.get_job("j").unwrap().is_none());
        assert!(db.get_session("s").unwrap().is_none());
    }

    #[test]
    fn mark_interrupted_on_startup_touches_only_live_jobs() {
        let db = db_with_project();
        for (id, st) in [
            ("a", JobState::Running),
            ("b", JobState::WaitingForApproval),
            ("c", JobState::Queued),
            ("d", JobState::Succeeded),
        ] {
            db.insert_job(id, "p1", st).unwrap();
        }
        assert_eq!(db.mark_interrupted_on_startup().unwrap(), 2);
        let state = |id: &str| db.get_job(id).unwrap().unwrap().state;
        assert_eq!(state("a"), JobState::Interrupted);
        assert_eq!(state("b"), JobState::Interrupted);
        assert_eq!(state("c"), JobState::Queued);
        assert_eq!(state("d"), JobState::Succeeded);
        assert!(db.get_job("a").unwrap().unwrap().completed_at.is_some());
    }

    #[test]
    fn modifying_job_detection() {
        let db = db_with_project();
        assert!(!db.has_running_modifying_job("p1").unwrap());
        db.insert_job("j1", "p1", JobState::Running).unwrap();
        db.insert_session(&session("s1", "j1", SessionMode::Plan))
            .unwrap();
        assert!(!db.has_running_modifying_job("p1").unwrap());
        db.insert_job("j2", "p1", JobState::Running).unwrap();
        db.insert_session(&session("s2", "j2", SessionMode::Changes))
            .unwrap();
        assert!(db.has_running_modifying_job("p1").unwrap());
        db.transition_job(
            "j2",
            &[JobState::Running],
            JobState::Failed,
            &JobUpdate::default(),
        )
        .unwrap();
        assert!(!db.has_running_modifying_job("p1").unwrap());
    }

    #[test]
    fn session_roundtrip_and_updates() {
        let db = db_with_project();
        db.insert_job("j", "p1", JobState::Running).unwrap();
        db.insert_session(&session("s", "j", SessionMode::Changes))
            .unwrap();
        db.set_provider_session_id("s", "prov-1").unwrap();
        db.set_session_refs("s", Some("b1"), None).unwrap();
        db.set_session_refs("s", None, Some("a1")).unwrap();
        let s = db.get_session("s").unwrap().unwrap();
        assert_eq!(s.provider_session_id.as_deref(), Some("prov-1"));
        assert_eq!(
            (s.before_ref.as_deref(), s.after_ref.as_deref()),
            (Some("b1"), Some("a1"))
        );
        assert!(s.created_at.ends_with('Z'));
        assert_eq!(db.list_sessions_by_project("p1").unwrap().len(), 1);
        assert_eq!(db.list_jobs_by_project("p1").unwrap().len(), 1);
        // One session per job.
        assert!(
            db.insert_session(&session("s2", "j", SessionMode::Plan))
                .is_err()
        );
    }

    #[test]
    fn profile_crud_and_single_default() {
        let db = db_with_project();
        assert!(db.get_default_profile().unwrap().is_none());
        db.save_profile(&profile("a", true)).unwrap();
        db.save_profile(&profile("b", true)).unwrap();
        assert_eq!(db.get_default_profile().unwrap().unwrap().id, "b");
        assert!(!db.get_profile("a").unwrap().unwrap().is_default);
        let mut b = db.get_profile("b").unwrap().unwrap();
        assert_eq!(b.env, profile("b", true).env);
        b.model = Some("opus".into());
        db.save_profile(&b).unwrap();
        assert_eq!(db.list_profiles().unwrap().len(), 2);
        assert_eq!(
            db.get_profile("b").unwrap().unwrap().model.as_deref(),
            Some("opus")
        );
    }

    #[test]
    fn delete_profile_clears_project_default_and_nulls_sessions() {
        let db = db_with_project();
        db.save_profile(&profile("a", false)).unwrap();
        let mut p = db.get_project("p1").unwrap().unwrap();
        p.default_agent_id = Some("a".into());
        db.update_project(&p).unwrap();
        db.insert_job("j", "p1", JobState::Succeeded).unwrap();
        let mut s = session("s", "j", SessionMode::Plan);
        s.profile_id = Some("a".into());
        db.insert_session(&s).unwrap();

        db.delete_profile("a").unwrap();
        assert!(
            db.get_project("p1")
                .unwrap()
                .unwrap()
                .default_agent_id
                .is_none()
        );
        assert!(db.get_session("s").unwrap().unwrap().profile_id.is_none());
    }
}
