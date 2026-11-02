//! Typed access to the `projects` table.

use rusqlite::types::Type as SqlType;
use rusqlite::{OptionalExtension, Row, params};

use super::{Db, DbError};
use crate::domain::TemplateId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PendingOp {
    Create,
    Copy,
}

impl PendingOp {
    fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Copy => "copy",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProjectRow {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub template_id: TemplateId,
    pub app_id: String,
    pub workspace_path: String,
    pub archived: bool,
    pub pending_op: Option<PendingOp>,
    pub default_agent_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_opened_at: Option<String>,
}

/// Fields the caller chooses on insert; timestamps are set by the database.
pub(crate) struct NewProject {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub template_id: TemplateId,
    pub app_id: String,
    pub workspace_path: String,
    pub pending_op: Option<PendingOp>,
}

const COLS: &str = "id, name, slug, template_id, app_id, workspace_path, archived, pending_op, \
                    default_agent_id, created_at, updated_at, last_opened_at";
const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ','now')";

fn bad(col: usize, what: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(col, SqlType::Text, what.into())
}

fn map_row(r: &Row<'_>) -> rusqlite::Result<ProjectRow> {
    let template: String = r.get(3)?;
    let pending: Option<String> = r.get(7)?;
    Ok(ProjectRow {
        id: r.get(0)?,
        name: r.get(1)?,
        slug: r.get(2)?,
        template_id: TemplateId::parse(&template)
            .ok_or_else(|| bad(3, format!("unknown template {template}")))?,
        app_id: r.get(4)?,
        workspace_path: r.get(5)?,
        archived: r.get(6)?,
        pending_op: match pending.as_deref() {
            None => None,
            Some("create") => Some(PendingOp::Create),
            Some("copy") => Some(PendingOp::Copy),
            Some(other) => return Err(bad(7, format!("unknown pending_op {other}"))),
        },
        default_agent_id: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
        last_opened_at: r.get(11)?,
    })
}

impl Db {
    pub(crate) fn insert_project(&self, p: &NewProject) -> Result<(), DbError> {
        self.conn().execute(
            &format!(
                "INSERT INTO projects (id, name, slug, template_id, app_id, workspace_path,
                                       pending_op, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, {NOW}, {NOW})"
            ),
            params![
                p.id,
                p.name,
                p.slug,
                p.template_id.as_str(),
                p.app_id,
                p.workspace_path,
                p.pending_op.map(PendingOp::as_str)
            ],
        )?;
        Ok(())
    }

    /// Updates the mutable fields of an existing row (not `pending_op`; see `set_pending`).
    pub(crate) fn update_project(&self, p: &ProjectRow) -> Result<(), DbError> {
        self.conn().execute(
            &format!(
                "UPDATE projects SET name=?2, slug=?3, template_id=?4, app_id=?5,
                    workspace_path=?6, archived=?7, default_agent_id=?8,
                    last_opened_at=?9, updated_at={NOW}
                 WHERE id=?1"
            ),
            params![
                p.id,
                p.name,
                p.slug,
                p.template_id.as_str(),
                p.app_id,
                p.workspace_path,
                p.archived,
                p.default_agent_id,
                p.last_opened_at
            ],
        )?;
        Ok(())
    }

    pub(crate) fn set_pending(&self, id: &str, op: Option<PendingOp>) -> Result<(), DbError> {
        self.conn().execute(
            &format!("UPDATE projects SET pending_op=?2, updated_at={NOW} WHERE id=?1"),
            params![id, op.map(PendingOp::as_str)],
        )?;
        Ok(())
    }

    pub(crate) fn delete_project(&self, id: &str) -> Result<(), DbError> {
        self.conn()
            .execute("DELETE FROM projects WHERE id=?1", [id])?;
        Ok(())
    }

    pub(crate) fn get_project(&self, id: &str) -> Result<Option<ProjectRow>, DbError> {
        self.get_by("id", id)
    }

    pub(crate) fn get_project_by_slug(&self, slug: &str) -> Result<Option<ProjectRow>, DbError> {
        self.get_by("slug", slug)
    }

    pub(crate) fn get_project_by_path(&self, path: &str) -> Result<Option<ProjectRow>, DbError> {
        self.get_by("workspace_path", path)
    }

    /// All rows, including ones with a pending operation, oldest first.
    pub(crate) fn list_projects(&self) -> Result<Vec<ProjectRow>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM projects ORDER BY created_at, rowid"
        ))?;
        Ok(stmt.query_map([], map_row)?.collect::<Result<_, _>>()?)
    }

    fn get_by(&self, col: &str, value: &str) -> Result<Option<ProjectRow>, DbError> {
        // `col` is only ever passed from the literals above.
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {COLS} FROM projects WHERE {col} = ?1"),
                [value],
                map_row,
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new(id: &str, slug: &str, path: &str) -> NewProject {
        NewProject {
            id: id.into(),
            name: slug.into(),
            slug: slug.into(),
            template_id: TemplateId::TauriVue,
            app_id: format!("com.x.{slug}"),
            workspace_path: path.into(),
            pending_op: None,
        }
    }

    #[test]
    fn insert_get_update_delete() {
        let db = Db::open_in_memory().unwrap();
        db.insert_project(&new("1", "a", "/p/a")).unwrap();
        let mut row = db.get_project_by_slug("a").unwrap().unwrap();
        assert_eq!(db.get_project_by_path("/p/a").unwrap().unwrap().id, "1");
        assert!(row.created_at.ends_with('Z') && row.last_opened_at.is_none());
        assert!(!row.archived && row.default_agent_id.is_none());

        row.archived = true;
        row.workspace_path = "/p/b".into();
        db.update_project(&row).unwrap();
        let got = db.get_project("1").unwrap().unwrap();
        assert!(got.archived);
        assert_eq!(got.workspace_path, "/p/b");

        db.delete_project("1").unwrap();
        assert!(db.get_project("1").unwrap().is_none());
    }

    #[test]
    fn slug_and_path_are_unique() {
        let db = Db::open_in_memory().unwrap();
        db.insert_project(&new("1", "a", "/p/a")).unwrap();
        assert!(db.insert_project(&new("2", "a", "/p/other")).is_err());
        assert!(db.insert_project(&new("3", "c", "/p/a")).is_err());
    }

    #[test]
    fn pending_set_and_clear() {
        let db = Db::open_in_memory().unwrap();
        let mut p = new("1", "a", "/p/a");
        p.pending_op = Some(PendingOp::Create);
        db.insert_project(&p).unwrap();
        assert_eq!(
            db.get_project("1").unwrap().unwrap().pending_op,
            Some(PendingOp::Create)
        );
        db.set_pending("1", Some(PendingOp::Copy)).unwrap();
        assert_eq!(
            db.list_projects().unwrap()[0].pending_op,
            Some(PendingOp::Copy)
        );
        db.set_pending("1", None).unwrap();
        assert_eq!(db.get_project("1").unwrap().unwrap().pending_op, None);
    }

    #[test]
    fn check_constraint_rejects_bad_pending_op() {
        let db = Db::open_in_memory().unwrap();
        db.insert_project(&new("1", "a", "/p/a")).unwrap();
        let r = db
            .conn()
            .execute("UPDATE projects SET pending_op='bogus'", []);
        assert!(r.is_err());
    }
}
