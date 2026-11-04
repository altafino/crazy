//! Typed access to `run_attachments`: the files each run's prompt carried (chat attachments).
//! Only the attachments service (`services/attachments`) writes these rows.

use rusqlite::{Row, params, params_from_iter};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{Db, DbError};

const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ','now')";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AttachmentKind {
    Image,
    Pdf,
    Text,
    Docx,
}

impl AttachmentKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Pdf => "pdf",
            Self::Text => "text",
            Self::Docx => "docx",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "image" => Self::Image,
            "pdf" => Self::Pdf,
            "text" => Self::Text,
            "docx" => Self::Docx,
            _ => return None,
        })
    }
}

/// One attachment of one run. Paths are relative to the conversation folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttachmentRow {
    pub session_id: String,
    pub ordinal: u32,
    /// The name the user saw (not sanitized).
    pub name: String,
    pub stored_path: String,
    pub kind: AttachmentKind,
    pub size_bytes: u32,
    /// For DOCX: the converted Markdown Claude reads.
    pub converted_path: Option<String>,
    /// True once pruning deleted the files ("File removed").
    pub removed: bool,
    /// Set by the database on insert.
    pub created_at: String,
}

const COLS: &str =
    "session_id, ordinal, name, stored_path, kind, size_bytes, converted_path, removed, created_at";

fn row(r: &Row<'_>) -> rusqlite::Result<AttachmentRow> {
    let kind: String = r.get(4)?;
    Ok(AttachmentRow {
        session_id: r.get(0)?,
        ordinal: r.get(1)?,
        name: r.get(2)?,
        stored_path: r.get(3)?,
        kind: AttachmentKind::parse(&kind).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                format!("unknown attachment kind {kind}").into(),
            )
        })?,
        size_bytes: r.get(5)?,
        converted_path: r.get(6)?,
        removed: r.get(7)?,
        created_at: r.get(8)?,
    })
}

impl Db {
    /// Inserts all rows or none.
    pub(crate) fn insert_attachments(&self, rows: &[AttachmentRow]) -> Result<(), DbError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        for a in rows {
            tx.execute(
                &format!(
                    "INSERT INTO run_attachments (session_id, ordinal, name, stored_path, kind,
                            size_bytes, converted_path, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, {NOW})"
                ),
                params![
                    a.session_id,
                    a.ordinal,
                    a.name,
                    a.stored_path,
                    a.kind.as_str(),
                    a.size_bytes,
                    a.converted_path
                ],
            )?;
        }
        Ok(tx.commit()?)
    }

    /// The attachments of all `session_ids` in one query (timeline), by run then ordinal.
    pub(crate) fn list_attachments(
        &self,
        session_ids: &[String],
    ) -> Result<Vec<AttachmentRow>, DbError> {
        if session_ids.is_empty() {
            return Ok(Vec::new());
        }
        let marks = vec!["?"; session_ids.len()].join(",");
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLS} FROM run_attachments WHERE session_id IN ({marks})
             ORDER BY session_id, ordinal"
        ))?;
        Ok(stmt
            .query_map(params_from_iter(session_ids), row)?
            .collect::<Result<_, _>>()?)
    }

    /// Flags the attachments of these runs as deleted from disk.
    pub(crate) fn mark_attachments_removed(&self, session_ids: &[String]) -> Result<(), DbError> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare("UPDATE run_attachments SET removed = 1 WHERE session_id = ?1")?;
        for id in session_ids {
            stmt.execute([id])?;
        }
        Ok(())
    }
}
