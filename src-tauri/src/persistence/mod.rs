//! SQLite persistence (rusqlite, bundled). Migrations tracked with PRAGMA user_version.

mod agents;
mod attachments;
mod git_sync;
mod projects;
mod releases;

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use rusqlite::{Connection, OptionalExtension, params};

#[cfg(test)]
pub(crate) use agents::JobRow;
pub(crate) use agents::{JobState, JobUpdate, ProfileRow, SessionMode, SessionRow};
pub(crate) use attachments::{AttachmentKind, AttachmentRow};
pub(crate) use git_sync::{SyncOp, SyncRecord};
pub(crate) use projects::{NewProject, PendingOp, ProjectRow};
pub(crate) use releases::{InstallRow, ReleaseRow, Smoke};

/// One schema step. `rebuilds_tables` migrations drop and recreate tables other tables
/// reference, so the runner switches foreign keys off around them (SQLite's 12-step rebuild).
struct Migration {
    sql: &'static str,
    rebuilds_tables: bool,
}

/// Ordered; entry `i` brings the schema to `user_version = i + 1`.
const MIGRATIONS: &[Migration] = &[
    Migration {
        sql: include_str!("../../migrations/0001_init.sql"),
        rebuilds_tables: false,
    },
    Migration {
        sql: include_str!("../../migrations/0002_agents.sql"),
        rebuilds_tables: false,
    },
    Migration {
        sql: include_str!("../../migrations/0003_preview.sql"),
        rebuilds_tables: true,
    },
    Migration {
        sql: include_str!("../../migrations/0004_builds.sql"),
        rebuilds_tables: true,
    },
    Migration {
        sql: include_str!("../../migrations/0005_git_sync.sql"),
        rebuilds_tables: false,
    },
    Migration {
        sql: include_str!("../../migrations/0006_attachments.sql"),
        rebuilds_tables: false,
    },
];

#[derive(Debug, thiserror::Error)]
pub(crate) enum DbError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database schema version {found} is newer than this Studio understands ({supported})")]
    NewerSchema { found: usize, supported: usize },
    #[error("invalid stored value: {0}")]
    Corrupt(String),
}

pub(crate) struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub(crate) fn open(path: &Path) -> Result<Self, DbError> {
        let conn = Connection::open(path)?;
        // The pragma returns the resulting mode as a row.
        conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub(crate) fn open_in_memory() -> Result<Self, DbError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self, DbError> {
        conn.pragma_update(None, "foreign_keys", true)?;
        migrate(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        // A panic mid-statement leaves SQLite itself consistent, so recover from poison.
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn get_setting(&self, key: &str) -> Result<Option<serde_json::Value>, DbError> {
        let text: Option<String> = self
            .conn()
            .query_row(
                "SELECT value FROM app_settings WHERE key = ?1",
                [key],
                |r| r.get(0),
            )
            .optional()?;
        text.map(|t| serde_json::from_str(&t).map_err(|e| DbError::Corrupt(format!("{key}: {e}"))))
            .transpose()
    }

    pub(crate) fn set_setting(&self, key: &str, value: &serde_json::Value) -> Result<(), DbError> {
        self.conn().execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value.to_string()],
        )?;
        Ok(())
    }
}

fn migrate(conn: &mut Connection) -> Result<(), DbError> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if current as usize > MIGRATIONS.len() {
        return Err(DbError::NewerSchema {
            found: current as usize,
            supported: MIGRATIONS.len(),
        });
    }
    for (i, m) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        // PRAGMA foreign_keys is a no-op inside a transaction, so it is switched here.
        if m.rebuilds_tables {
            conn.pragma_update(None, "foreign_keys", false)?;
        }
        let applied = apply(conn, i, m);
        if m.rebuilds_tables {
            // Back on even when the migration failed.
            conn.pragma_update(None, "foreign_keys", true)?;
        }
        applied?;
    }
    Ok(())
}

fn apply(conn: &mut Connection, i: usize, m: &Migration) -> Result<(), DbError> {
    let tx = conn.transaction()?;
    tx.execute_batch(m.sql)?;
    if m.rebuilds_tables {
        let violations: Vec<String> = tx
            .prepare("PRAGMA foreign_key_check")?
            .query_map([], |r| {
                Ok(format!(
                    "{} row {:?} -> {}",
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, String>(2)?
                ))
            })?
            .collect::<Result<_, _>>()?;
        if !violations.is_empty() {
            // Dropping `tx` rolls the whole migration back.
            return Err(DbError::Corrupt(format!(
                "migration {} left broken references: {}",
                i + 1,
                violations.join("; ")
            )));
        }
    }
    tx.pragma_update(None, "user_version", i as i64 + 1)?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_version(db: &Db) -> i64 {
        db.conn()
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn fresh_db_is_migrated_with_fk_on() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(user_version(&db), MIGRATIONS.len() as i64);
        let fk: bool = db
            .conn()
            .pragma_query_value(None, "foreign_keys", |r| r.get(0))
            .unwrap();
        assert!(fk);
        let tables: Vec<String> = db
            .conn()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            tables,
            [
                "agent_profiles",
                "agent_sessions",
                "app_settings",
                "git_sync",
                "installs",
                "jobs",
                "projects",
                "releases",
                "run_attachments"
            ]
        );
    }

    #[test]
    fn reopen_keeps_data_and_does_not_rerun_migrations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        Db::open(&path)
            .unwrap()
            .set_setting("k", &serde_json::json!({"a": 1}))
            .unwrap();
        // Re-running 0001 would fail with "table already exists" or wipe the row.
        let db = Db::open(&path).unwrap();
        assert_eq!(user_version(&db), MIGRATIONS.len() as i64);
        assert_eq!(
            db.get_setting("k").unwrap(),
            Some(serde_json::json!({"a": 1}))
        );
        let mode: String = db
            .conn()
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn v1_database_upgrades_keeping_projects() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0].sql).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            for (id, slug) in [("1", "a"), ("2", "b")] {
                conn.execute(
                    "INSERT INTO projects (id, name, slug, template_id, app_id, workspace_path,
                                           created_at, updated_at)
                     VALUES (?1, ?2, ?2, 'tauri-vue', 'com.x', '/p/' || ?2, 't', 't')",
                    [id, slug],
                )
                .unwrap();
            }
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(user_version(&db), MIGRATIONS.len() as i64);
        let ids: Vec<_> = db
            .list_projects()
            .unwrap()
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(ids, ["1", "2"]);
        db.insert_job("j", "1", crate::persistence::JobState::Queued)
            .unwrap();
    }

    /// A populated v2 database (jobs + agent sessions) must survive the jobs rebuild.
    #[test]
    fn v2_database_with_runs_survives_the_jobs_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "foreign_keys", true).unwrap();
            conn.execute_batch(MIGRATIONS[0].sql).unwrap();
            conn.execute_batch(MIGRATIONS[1].sql).unwrap();
            conn.pragma_update(None, "user_version", 2).unwrap();
            conn.execute_batch(
                "INSERT INTO projects (id, name, slug, template_id, app_id, workspace_path,
                                       created_at, updated_at)
                 VALUES ('p', 'p', 'p', 'tauri-vue', 'com.x', '/p', 't', 't');
                 INSERT INTO jobs (id, project_id, state, started_at, completed_at, exit_code,
                                   error_kind, error_message)
                 VALUES ('j1', 'p', 'succeeded', 's1', 'c1', 0, NULL, NULL),
                        ('j2', 'p', 'failed', 's2', 'c2', 1, 'failed', 'boom'),
                        ('j3', 'p', 'running', 's3', NULL, NULL, NULL, NULL);
                 INSERT INTO agent_sessions (id, job_id, project_id, mode, prompt, log_path,
                                             before_ref, created_at)
                 VALUES ('s1', 'j1', 'p', 'changes', 'a', '/l1', 'b1', 't'),
                        ('s2', 'j2', 'p', 'plan', 'b', '/l2', NULL, 't');",
            )
            .unwrap();
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(user_version(&db), MIGRATIONS.len() as i64);

        let jobs = db.list_jobs_by_project("p").unwrap();
        let ids: Vec<_> = jobs.iter().map(|j| j.id.as_str()).collect();
        assert_eq!(ids, ["j1", "j2", "j3"]);
        assert!(jobs.iter().all(|j| j.kind == "agent"));
        let j2 = db.get_job("j2").unwrap().unwrap();
        assert_eq!(
            (
                j2.exit_code,
                j2.error_message.as_deref(),
                j2.completed_at.as_deref()
            ),
            (Some(1), Some("boom"), Some("c2"))
        );
        let sessions = db.list_sessions_by_project("p").unwrap();
        assert_eq!(sessions.len(), 2, "sessions must not cascade away");
        assert_eq!(sessions[0].job_id, "j1");
        assert_eq!(sessions[0].before_ref.as_deref(), Some("b1"));
        assert!(sessions[0].checkpoint_ref.is_none());

        let conn = db.conn();
        // agent_sessions still points at `jobs`, not at a renamed leftover.
        let sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'agent_sessions'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            sql.contains("REFERENCES jobs(id)") || sql.contains("REFERENCES \"jobs\"(id)"),
            "{sql}"
        );
        assert!(
            !sql.contains("jobs_new") && !sql.contains("jobs_old"),
            "{sql}"
        );
        let violations: i64 = conn
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(violations, 0);
        let fk: bool = conn
            .pragma_query_value(None, "foreign_keys", |r| r.get(0))
            .unwrap();
        assert!(fk, "foreign keys are back on after the rebuild");
        let index: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'jobs_project_started'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(index, 1);
        drop(conn);

        // New sessions link to the rebuilt table; previews are accepted; cascade still works.
        db.insert_job("j4", "p", JobState::Queued).unwrap();
        db.insert_session(&SessionRow {
            id: "s4".into(),
            job_id: "j4".into(),
            project_id: "p".into(),
            profile_id: None,
            mode: SessionMode::Plan,
            prompt: "x".into(),
            provider_session_id: None,
            resumed_from: None,
            log_path: "/l".into(),
            before_ref: None,
            after_ref: None,
            checkpoint_ref: None,
            created_at: String::new(),
        })
        .unwrap();
        db.insert_job_of_kind("pv", "p", "preview", JobState::Running)
            .unwrap();
        assert!(
            db.insert_job_of_kind("bad", "p", "other", JobState::Queued)
                .is_err()
        );
        db.conn()
            .execute("DELETE FROM jobs WHERE id = 'j1'", [])
            .unwrap();
        assert!(db.get_session("s1").unwrap().is_none());
    }

    /// A populated v3 database must survive the jobs rebuild of 0004 with every job and
    /// session intact (dropping `jobs` with foreign keys on would cascade-delete sessions).
    #[test]
    fn v3_database_with_runs_and_previews_survives_the_builds_migration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "foreign_keys", true).unwrap();
            for m in &MIGRATIONS[..3] {
                conn.execute_batch(m.sql).unwrap();
            }
            conn.pragma_update(None, "user_version", 3).unwrap();
            conn.execute_batch(
                "INSERT INTO projects (id, name, slug, template_id, app_id, workspace_path,
                                       created_at, updated_at)
                 VALUES ('p', 'p', 'p', 'tauri-vue', 'com.x', '/p', 't', 't');
                 INSERT INTO jobs (id, project_id, kind, state, started_at, completed_at,
                                   exit_code, error_kind, error_message)
                 VALUES ('j1', 'p', 'agent', 'succeeded', 's1', 'c1', 0, NULL, NULL),
                        ('j2', 'p', 'agent', 'failed', 's2', 'c2', 1, 'failed', 'boom'),
                        ('pv1', 'p', 'preview', 'running', 's3', NULL, NULL, NULL, NULL),
                        ('pv2', 'p', 'preview', 'cancelled', 's4', 'c4', NULL, NULL, NULL);
                 INSERT INTO agent_sessions (id, job_id, project_id, mode, prompt, log_path,
                                             before_ref, checkpoint_ref, created_at)
                 VALUES ('s1', 'j1', 'p', 'changes', 'a', '/l1', 'b1', 'refs/c1', 't'),
                        ('s2', 'j2', 'p', 'plan', 'b', '/l2', NULL, NULL, 't');",
            )
            .unwrap();
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(user_version(&db), MIGRATIONS.len() as i64);

        let jobs = db.list_jobs_by_project("p").unwrap();
        assert_eq!(jobs.len(), 4);
        assert_eq!(db.list_jobs_of_kind("p", "agent").unwrap().len(), 2);
        assert_eq!(db.list_jobs_of_kind("p", "preview").unwrap().len(), 2);
        let j2 = db.get_job("j2").unwrap().unwrap();
        assert_eq!(
            (j2.exit_code, j2.error_message.as_deref()),
            (Some(1), Some("boom"))
        );
        let sessions = db.list_sessions_by_project("p").unwrap();
        assert_eq!(sessions.len(), 2, "sessions must not cascade away");
        assert_eq!(sessions[0].checkpoint_ref.as_deref(), Some("refs/c1"));

        let conn = db.conn();
        let violations: i64 = conn
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(violations, 0);
        let fk: bool = conn
            .pragma_query_value(None, "foreign_keys", |r| r.get(0))
            .unwrap();
        assert!(fk);
        let sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'agent_sessions'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!sql.contains("jobs_new"), "{sql}");
        for name in ["jobs_project_started", "releases_project_version_kept"] {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name = ?1",
                    [name],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "{name}");
        }
        drop(conn);

        // Builds are accepted now; cascade from jobs to sessions still works.
        db.insert_job_of_kind("b1", "p", "build", JobState::Running)
            .unwrap();
        db.conn()
            .execute("DELETE FROM jobs WHERE id = 'j1'", [])
            .unwrap();
        assert!(db.get_session("s1").unwrap().is_none());
    }

    /// A populated v4 database upgrades with every row intact and empty git_sync and run_attachments.
    #[test]
    fn v4_database_upgrades_with_all_rows_intact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "foreign_keys", true).unwrap();
            for m in &MIGRATIONS[..4] {
                conn.execute_batch(m.sql).unwrap();
            }
            conn.pragma_update(None, "user_version", 4).unwrap();
            conn.execute_batch(
                "INSERT INTO projects (id, name, slug, template_id, app_id, workspace_path,
                                       created_at, updated_at)
                 VALUES ('p', 'p', 'p', 'tauri-vue', 'com.x', '/p', 't', 't');
                 INSERT INTO jobs (id, project_id, kind, state, started_at, completed_at,
                                   exit_code, error_kind, error_message)
                 VALUES ('j1', 'p', 'agent', 'succeeded', 's1', 'c1', 0, NULL, NULL),
                        ('b1', 'p', 'build', 'failed', 's2', 'c2', 1, 'failed', 'boom');
                 INSERT INTO agent_sessions (id, job_id, project_id, mode, prompt, log_path,
                                             before_ref, checkpoint_ref, created_at)
                 VALUES ('s1', 'j1', 'p', 'changes', 'a', '/l1', 'b1', 'refs/c1', 't');
                 INSERT INTO releases (id, project_id, job_id, version, sha256, size_bytes,
                                       architecture, product_name, main_binary_name,
                                       dirty_source, smoke, created_at)
                 VALUES ('r1', 'p', 'b1', '0.1.0', 'abc', 10, 'x86_64', 'P', 'p', 0,
                         'passed', 't');
                 INSERT INTO installs (project_id, desktop_file, installed_at)
                 VALUES ('p', '/d', 't');",
            )
            .unwrap();
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(user_version(&db), MIGRATIONS.len() as i64);
        assert_eq!(db.list_jobs_by_project("p").unwrap().len(), 2);
        assert_eq!(db.list_sessions_by_project("p").unwrap().len(), 1);
        let count = |t: &str| -> i64 {
            db.conn()
                .query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(
            [
                count("projects"),
                count("releases"),
                count("installs"),
                count("git_sync"),
                count("run_attachments")
            ],
            [1, 1, 1, 0, 0]
        );
    }

    #[test]
    fn rebuild_migration_with_violations_is_rolled_back() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", false).unwrap();
        conn.execute_batch(
            "CREATE TABLE a (id INTEGER PRIMARY KEY);
             CREATE TABLE b (a INTEGER REFERENCES a(id));
             INSERT INTO b VALUES (7);",
        )
        .unwrap();
        let m = Migration {
            sql: "CREATE TABLE marker (x);",
            rebuilds_tables: true,
        };
        let err = apply(&mut conn, 0, &m).unwrap_err();
        assert!(err.to_string().contains("broken references"), "{err}");
        let marker: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='marker'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(marker, 0, "rolled back");
    }

    #[test]
    fn newer_db_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        Connection::open(&path)
            .unwrap()
            .pragma_update(None, "user_version", 99)
            .unwrap();
        assert!(matches!(
            Db::open(&path),
            Err(DbError::NewerSchema { found: 99, .. })
        ));
    }

    #[test]
    fn settings_roundtrip_and_overwrite() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.get_setting("missing").unwrap(), None);
        db.set_setting("k", &serde_json::json!("a")).unwrap();
        db.set_setting("k", &serde_json::json!(["b", 2])).unwrap();
        assert_eq!(
            db.get_setting("k").unwrap(),
            Some(serde_json::json!(["b", 2]))
        );
    }
}
