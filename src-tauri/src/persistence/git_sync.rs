//! Typed access to `git_sync`: the last remote-sync result of a project (M5-R4).

use rusqlite::{OptionalExtension, params};

use super::{Db, DbError};
use crate::services::agents::secrets::redact;

/// The remote operation a [`SyncRecord`] describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SyncOp {
    Fetch,
    Pull,
    Merge,
    Push,
    Create,
}

impl SyncOp {
    fn as_str(self) -> &'static str {
        match self {
            Self::Fetch => "fetch",
            Self::Pull => "pull",
            Self::Merge => "merge",
            Self::Push => "push",
            Self::Create => "create",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "fetch" => Self::Fetch,
            "pull" => Self::Pull,
            "merge" => Self::Merge,
            "push" => Self::Push,
            "create" => Self::Create,
            _ => return None,
        })
    }
}

/// The one stored sync result of a project. `message` must already be redacted by the
/// caller; [`Db::upsert_sync`] redacts again as a safety net. `finished_at` is an RFC 3339
/// timestamp chosen by the caller.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncRecord {
    pub project_id: String,
    pub operation: SyncOp,
    pub ok: bool,
    pub message: Option<String>,
    pub finished_at: String,
}

impl Db {
    /// Inserts or replaces the project's sync row.
    pub(crate) fn upsert_sync(&self, r: &SyncRecord) -> Result<(), DbError> {
        let message = r.message.as_deref().map(|m| redact(m).into_owned());
        self.conn().execute(
            "INSERT INTO git_sync (project_id, operation, ok, message, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(project_id) DO UPDATE SET operation = excluded.operation,
                 ok = excluded.ok, message = excluded.message,
                 finished_at = excluded.finished_at",
            params![
                r.project_id,
                r.operation.as_str(),
                r.ok,
                message,
                r.finished_at
            ],
        )?;
        Ok(())
    }

    /// The project's last sync result, if any.
    pub(crate) fn get_sync(&self, project_id: &str) -> Result<Option<SyncRecord>, DbError> {
        let row = self
            .conn()
            .query_row(
                "SELECT operation, ok, message, finished_at FROM git_sync WHERE project_id = ?1",
                [project_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, bool>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(op, ok, message, finished_at)| {
            Ok(SyncRecord {
                project_id: project_id.to_owned(),
                operation: SyncOp::parse(&op)
                    .ok_or_else(|| DbError::Corrupt(format!("git_sync.operation: {op}")))?,
                ok,
                message,
                finished_at,
            })
        })
        .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(op: SyncOp, ok: bool, msg: Option<&str>) -> SyncRecord {
        SyncRecord {
            project_id: "p".into(),
            operation: op,
            ok,
            message: msg.map(Into::into),
            finished_at: "2026-11-03T12:00:00Z".into(),
        }
    }

    fn db_with_project() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO projects (id, name, slug, template_id, app_id, workspace_path,
                                       created_at, updated_at)
                 VALUES ('p', 'p', 'p', 'tauri-vue', 'com.x', '/p', 't', 't')",
                [],
            )
            .unwrap();
        db
    }

    #[test]
    fn upsert_replaces_and_get_reads_back() {
        let db = db_with_project();
        assert_eq!(db.get_sync("p").unwrap(), None);
        db.upsert_sync(&rec(SyncOp::Fetch, true, None)).unwrap();
        assert_eq!(
            db.get_sync("p").unwrap(),
            Some(rec(SyncOp::Fetch, true, None))
        );
        db.upsert_sync(&rec(SyncOp::Push, false, Some("rejected")))
            .unwrap();
        assert_eq!(
            db.get_sync("p").unwrap(),
            Some(rec(SyncOp::Push, false, Some("rejected")))
        );
        let n: i64 = db
            .conn()
            .query_row("SELECT count(*) FROM git_sync", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn message_is_redacted_on_write() {
        let db = db_with_project();
        let msg = format!(
            "fatal: https://me:hunter2@github.com/o/r.git and ghs_{}",
            "a".repeat(36)
        );
        db.upsert_sync(&rec(SyncOp::Pull, false, Some(&msg)))
            .unwrap();
        let got = db.get_sync("p").unwrap().unwrap().message.unwrap();
        assert!(!got.contains("hunter2") && !got.contains("ghs_"), "{got}");
        assert!(got.contains("https://[redacted]@github.com"), "{got}");
    }

    #[test]
    fn row_is_deleted_with_its_project() {
        let db = db_with_project();
        db.upsert_sync(&rec(SyncOp::Merge, true, None)).unwrap();
        db.conn()
            .execute("DELETE FROM projects WHERE id = 'p'", [])
            .unwrap();
        assert_eq!(db.get_sync("p").unwrap(), None);
    }
}
