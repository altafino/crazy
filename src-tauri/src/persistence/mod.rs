//! SQLite persistence (rusqlite, bundled). Migrations tracked with PRAGMA user_version.

mod agents;
mod projects;

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use rusqlite::{Connection, OptionalExtension, params};

#[allow(unused_imports)] // used by M2 lane A / commands
pub(crate) use agents::{JobRow, JobState, JobUpdate, ProfileRow, SessionMode, SessionRow};
pub(crate) use projects::{NewProject, PendingOp, ProjectRow};

/// Ordered; entry `i` brings the schema to `user_version = i + 1`.
const MIGRATIONS: &[&str] = &[
    include_str!("../../migrations/0001_init.sql"),
    include_str!("../../migrations/0002_agents.sql"),
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
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i as i64 + 1)?;
        tx.commit()?;
    }
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
                "jobs",
                "projects"
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
    fn v1_database_upgrades_to_v2_keeping_projects() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
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
        assert_eq!(user_version(&db), 2);
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
