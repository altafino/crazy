//! Typed access to `releases` and `installs` (M4). There is no "active" column: the
//! `current.AppImage` symlink is the only record of the active release (M4-D5).
// The build and install lanes are the callers.
#![allow(dead_code)]

use rusqlite::types::Type as SqlType;
use rusqlite::{OptionalExtension, Row, params};

use super::{Db, DbError};

const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ','now')";

/// The result of a release's launch check (M4-D10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Smoke {
    Passed,
    Failed,
    Unverified,
}

impl Smoke {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Unverified => "unverified",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "passed" => Self::Passed,
            "failed" => Self::Failed,
            "unverified" => Self::Unverified,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReleaseRow {
    pub id: String,
    pub project_id: String,
    pub job_id: Option<String>,
    pub version: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub architecture: String,
    pub build_profile: String,
    pub product_name: String,
    pub main_binary_name: String,
    pub source_commit: Option<String>,
    pub dirty_source: bool,
    pub snapshot_ref: Option<String>,
    pub smoke: Smoke,
    pub smoke_detail: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub last_activated_at: Option<String>,
    pub files_removed_at: Option<String>,
}

const RELEASE_COLS: &str = "id, project_id, job_id, version, sha256, size_bytes, architecture, \
    build_profile, product_name, main_binary_name, source_commit, dirty_source, snapshot_ref, \
    smoke, smoke_detail, notes, created_at, last_activated_at, files_removed_at";

fn release_row(r: &Row<'_>) -> rusqlite::Result<ReleaseRow> {
    let smoke: String = r.get(13)?;
    Ok(ReleaseRow {
        id: r.get(0)?,
        project_id: r.get(1)?,
        job_id: r.get(2)?,
        version: r.get(3)?,
        sha256: r.get(4)?,
        size_bytes: r.get(5)?,
        architecture: r.get(6)?,
        build_profile: r.get(7)?,
        product_name: r.get(8)?,
        main_binary_name: r.get(9)?,
        source_commit: r.get(10)?,
        dirty_source: r.get(11)?,
        snapshot_ref: r.get(12)?,
        smoke: Smoke::parse(&smoke).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                13,
                SqlType::Text,
                format!("unknown smoke value {smoke:?}").into(),
            )
        })?,
        smoke_detail: r.get(14)?,
        notes: r.get(15)?,
        created_at: r.get(16)?,
        last_activated_at: r.get(17)?,
        files_removed_at: r.get(18)?,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstallRow {
    pub project_id: String,
    pub desktop_file: String,
    pub installed_at: String,
}

impl Db {
    // ---- releases

    /// Fails on the unique index when a kept release of this project already has `version`.
    pub(crate) fn insert_release(&self, r: &ReleaseRow) -> Result<(), DbError> {
        self.conn().execute(
            &format!(
                "INSERT INTO releases ({RELEASE_COLS})
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                         ?17, ?18, ?19)"
            ),
            params![
                r.id,
                r.project_id,
                r.job_id,
                r.version,
                r.sha256,
                r.size_bytes,
                r.architecture,
                r.build_profile,
                r.product_name,
                r.main_binary_name,
                r.source_commit,
                r.dirty_source,
                r.snapshot_ref,
                r.smoke.as_str(),
                r.smoke_detail,
                r.notes,
                r.created_at,
                r.last_activated_at,
                r.files_removed_at,
            ],
        )?;
        Ok(())
    }

    pub(crate) fn get_release(&self, id: &str) -> Result<Option<ReleaseRow>, DbError> {
        Ok(self
            .conn()
            .query_row(
                &format!("SELECT {RELEASE_COLS} FROM releases WHERE id = ?1"),
                [id],
                release_row,
            )
            .optional()?)
    }

    /// A project's releases, newest first. `include_removed` adds those whose files are gone.
    pub(crate) fn list_releases(
        &self,
        project_id: &str,
        include_removed: bool,
    ) -> Result<Vec<ReleaseRow>, DbError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {RELEASE_COLS} FROM releases
             WHERE project_id = ?1 AND (?2 OR files_removed_at IS NULL)
             ORDER BY created_at DESC, rowid DESC"
        ))?;
        Ok(stmt
            .query_map(params![project_id, include_removed], release_row)?
            .collect::<Result<_, _>>()?)
    }

    /// Records that the release's files were deleted; the row stays. Frees its version.
    pub(crate) fn mark_release_files_removed(&self, id: &str) -> Result<(), DbError> {
        self.conn().execute(
            &format!("UPDATE releases SET files_removed_at = {NOW} WHERE id = ?1"),
            [id],
        )?;
        Ok(())
    }

    pub(crate) fn set_release_activated(&self, id: &str) -> Result<(), DbError> {
        self.conn().execute(
            &format!("UPDATE releases SET last_activated_at = {NOW} WHERE id = ?1"),
            [id],
        )?;
        Ok(())
    }

    // ---- installs

    pub(crate) fn get_install(&self, project_id: &str) -> Result<Option<InstallRow>, DbError> {
        Ok(self
            .conn()
            .query_row(
                "SELECT project_id, desktop_file, installed_at FROM installs
                 WHERE project_id = ?1",
                [project_id],
                |r| {
                    Ok(InstallRow {
                        project_id: r.get(0)?,
                        desktop_file: r.get(1)?,
                        installed_at: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    /// One install per project: a second call replaces the first.
    pub(crate) fn upsert_install(&self, i: &InstallRow) -> Result<(), DbError> {
        self.conn().execute(
            "INSERT INTO installs (project_id, desktop_file, installed_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (project_id) DO UPDATE SET
                 desktop_file = excluded.desktop_file, installed_at = excluded.installed_at",
            params![i.project_id, i.desktop_file, i.installed_at],
        )?;
        Ok(())
    }

    pub(crate) fn delete_install(&self, project_id: &str) -> Result<(), DbError> {
        self.conn()
            .execute("DELETE FROM installs WHERE project_id = ?1", [project_id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::TemplateId;
    use crate::persistence::{JobState, NewProject};

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

    fn release(id: &str, version: &str, created: &str) -> ReleaseRow {
        ReleaseRow {
            id: id.into(),
            project_id: "p1".into(),
            job_id: None,
            version: version.into(),
            sha256: "ab".repeat(32),
            size_bytes: 123_456,
            architecture: "x86_64".into(),
            build_profile: "local".into(),
            product_name: "App".into(),
            main_binary_name: "app".into(),
            source_commit: Some("deadbeef".into()),
            dirty_source: true,
            snapshot_ref: Some("refs/studio/builds/r1".into()),
            smoke: Smoke::Passed,
            smoke_detail: None,
            notes: Some("note".into()),
            created_at: created.into(),
            last_activated_at: None,
            files_removed_at: None,
        }
    }

    #[test]
    fn release_roundtrips_and_lists_newest_first() {
        let db = db_with_project();
        let r1 = release("r1", "1.0.0", "2026-01-01T00:00:00Z");
        db.insert_release(&r1).unwrap();
        db.insert_release(&release("r2", "1.1.0", "2026-01-02T00:00:00Z"))
            .unwrap();
        assert_eq!(db.get_release("r1").unwrap().unwrap(), r1);
        assert!(db.get_release("nope").unwrap().is_none());
        let ids: Vec<_> = db
            .list_releases("p1", false)
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, ["r2", "r1"]);
    }

    #[test]
    fn a_kept_version_is_unique_until_its_files_are_removed() {
        let db = db_with_project();
        db.insert_release(&release("r1", "1.0.0", "2026-01-01T00:00:00Z"))
            .unwrap();
        assert!(
            db.insert_release(&release("r2", "1.0.0", "2026-01-02T00:00:00Z"))
                .is_err()
        );
        db.mark_release_files_removed("r1").unwrap();
        db.insert_release(&release("r3", "1.0.0", "2026-01-03T00:00:00Z"))
            .unwrap();
        // The removed row stays, listed only on request.
        assert!(
            db.get_release("r1")
                .unwrap()
                .unwrap()
                .files_removed_at
                .is_some()
        );
        assert_eq!(db.list_releases("p1", false).unwrap().len(), 1);
        assert_eq!(db.list_releases("p1", true).unwrap().len(), 2);
    }

    #[test]
    fn activation_is_stamped_and_deleting_the_project_removes_everything() {
        let db = db_with_project();
        db.insert_release(&release("r1", "1.0.0", "2026-01-01T00:00:00Z"))
            .unwrap();
        db.set_release_activated("r1").unwrap();
        assert!(
            db.get_release("r1")
                .unwrap()
                .unwrap()
                .last_activated_at
                .is_some()
        );
        db.upsert_install(&InstallRow {
            project_id: "p1".into(),
            desktop_file: "/d".into(),
            installed_at: "t".into(),
        })
        .unwrap();
        db.delete_project("p1").unwrap();
        assert!(db.get_release("r1").unwrap().is_none());
        assert!(db.get_install("p1").unwrap().is_none());
    }

    #[test]
    fn installs_upsert_replaces_and_delete_removes() {
        let db = db_with_project();
        assert!(db.get_install("p1").unwrap().is_none());
        let first = InstallRow {
            project_id: "p1".into(),
            desktop_file: "/a.desktop".into(),
            installed_at: "t1".into(),
        };
        db.upsert_install(&first).unwrap();
        assert_eq!(db.get_install("p1").unwrap().unwrap(), first);
        let second = InstallRow {
            desktop_file: "/b.desktop".into(),
            installed_at: "t2".into(),
            ..first
        };
        db.upsert_install(&second).unwrap();
        assert_eq!(db.get_install("p1").unwrap().unwrap(), second);
        db.delete_install("p1").unwrap();
        assert!(db.get_install("p1").unwrap().is_none());
        db.delete_install("p1").unwrap(); // already gone: fine
    }

    #[test]
    fn every_job_kind_inserts_and_a_running_build_is_interrupted_on_startup() {
        use crate::services::jobs::JobKind;
        let db = db_with_project();
        for (i, k) in [JobKind::Agent, JobKind::Preview, JobKind::Build]
            .into_iter()
            .enumerate()
        {
            db.insert_job_of_kind(&format!("j{i}"), "p1", k.as_str(), JobState::Running)
                .unwrap();
        }
        assert_eq!(db.mark_interrupted_on_startup().unwrap(), 3);
        let build = db.list_jobs_of_kind("p1", "build").unwrap();
        assert_eq!(build.len(), 1);
        assert_eq!(build[0].state, JobState::Interrupted);
        assert!(build[0].completed_at.is_some());
    }
}
