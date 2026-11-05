//! Typed access to `app_data_backups` (app databases): the backups Studio keeps of a generated
//! app's database files.

use rusqlite::types::Type as SqlType;
use rusqlite::{OptionalExtension, Row, params};

use super::{Db, DbError};

/// Which of an app's two database files (the convention's `dev/app.db` or `app.db`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DataSet {
    Preview,
    Installed,
}

impl DataSet {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Installed => "installed",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "preview" => Self::Preview,
            "installed" => Self::Installed,
            _ => return None,
        })
    }
}

/// Why a backup exists. `Release` ones are automatic and go with their release (DR-8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum BackupReason {
    Manual,
    Release,
    Restore,
}

impl BackupReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Release => "release",
            Self::Restore => "restore",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "manual" => Self::Manual,
            "release" => Self::Release,
            "restore" => Self::Restore,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BackupRow {
    pub id: String,
    pub project_id: String,
    pub which: DataSet,
    pub reason: BackupReason,
    /// For `Release` backups: the release whose data this holds; the backup goes with it.
    pub release_id: Option<String>,
    /// Shown in the list, for example "before 1.1.0".
    pub label: Option<String>,
    pub path: String,
    pub size_bytes: i64,
    /// `PRAGMA user_version` of the backed-up data.
    pub schema_version: Option<i64>,
    pub created_at: String,
}

const COLS: &str = "id, project_id, which, reason, release_id, label, path, size_bytes, schema_version, created_at";

fn bad(col: usize, what: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(col, SqlType::Text, what.into())
}

fn backup_row(r: &Row<'_>) -> rusqlite::Result<BackupRow> {
    let which: String = r.get(2)?;
    let reason: String = r.get(3)?;
    Ok(BackupRow {
        id: r.get(0)?,
        project_id: r.get(1)?,
        which: DataSet::parse(&which).ok_or_else(|| bad(2, format!("unknown which {which:?}")))?,
        reason: BackupReason::parse(&reason)
            .ok_or_else(|| bad(3, format!("unknown reason {reason:?}")))?,
        release_id: r.get(4)?,
        label: r.get(5)?,
        path: r.get(6)?,
        size_bytes: r.get(7)?,
        schema_version: r.get(8)?,
        created_at: r.get(9)?,
    })
}

impl Db {
    pub(crate) fn insert_backup(&self, b: &BackupRow) -> Result<(), DbError> {
        self.conn().execute(
            &format!(
                "INSERT INTO app_data_backups ({COLS})
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
            ),
            params![
                b.id,
                b.project_id,
                b.which.as_str(),
                b.reason.as_str(),
                b.release_id,
                b.label,
                b.path,
                b.size_bytes,
                b.schema_version,
                b.created_at,
            ],
        )?;
        Ok(())
    }

    pub(crate) fn get_backup(&self, id: &str) -> Result<Option<BackupRow>, DbError> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {COLS} FROM app_data_backups WHERE id = ?1"),
                [id],
                backup_row,
            )
            .optional()?)
    }

    /// A project's backups, newest first; `which` narrows to one data set.
    pub(crate) fn list_backups(
        &self,
        project_id: &str,
        which: Option<DataSet>,
    ) -> Result<Vec<BackupRow>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM app_data_backups
             WHERE project_id = ?1 AND (?2 IS NULL OR which = ?2)
             ORDER BY created_at DESC, rowid DESC"
        ))?;
        Ok(stmt
            .query_map(params![project_id, which.map(DataSet::as_str)], backup_row)?
            .collect::<Result<_, _>>()?)
    }

    /// The automatic backups that belong to a release, newest first.
    pub(crate) fn list_release_backups(&self, release_id: &str) -> Result<Vec<BackupRow>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM app_data_backups
             WHERE release_id = ?1 AND reason = 'release'
             ORDER BY created_at DESC, rowid DESC"
        ))?;
        Ok(stmt
            .query_map([release_id], backup_row)?
            .collect::<Result<_, _>>()?)
    }

    pub(crate) fn delete_backup(&self, id: &str) -> Result<(), DbError> {
        self.conn()
            .execute("DELETE FROM app_data_backups WHERE id = ?1", [id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::TemplateId;
    use crate::persistence::NewProject;

    fn row(id: &str, reason: BackupReason, release: Option<&str>, at: &str) -> BackupRow {
        BackupRow {
            id: id.into(),
            project_id: "p1".into(),
            which: DataSet::Installed,
            reason,
            release_id: release.map(Into::into),
            label: Some("before 1.1.0".into()),
            path: format!("/b/{id}.db"),
            size_bytes: 10,
            schema_version: Some(3),
            created_at: at.into(),
        }
    }

    #[test]
    fn backups_round_trip_list_newest_first_and_delete() {
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
        let a = row("a", BackupReason::Manual, None, "2026-01-01T00:00:01Z");
        let mut b = row("b", BackupReason::Restore, None, "2026-01-01T00:00:02Z");
        b.which = DataSet::Preview;
        db.insert_backup(&a).unwrap();
        db.insert_backup(&b).unwrap();
        assert_eq!(db.get_backup("a").unwrap(), Some(a.clone()));
        let all = db.list_backups("p1", None).unwrap();
        assert_eq!(
            all.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["b", "a"]
        );
        assert_eq!(
            db.list_backups("p1", Some(DataSet::Installed)).unwrap(),
            vec![a]
        );
        db.delete_backup("a").unwrap();
        assert_eq!(db.get_backup("a").unwrap(), None);
    }
}
