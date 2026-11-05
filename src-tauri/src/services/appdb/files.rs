//! The file side: backups (`VACUUM INTO` on a read-only connection, so a consistent copy while
//! the app runs), restore, copy installed -> preview, reset, the running-app guards, and what
//! the build and install services need (test-launch copy, backup before a release switch,
//! retention). Backups live under Studio's own data folder, never beside the app's data.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

use super::inspect::open_ro;
use super::{AppDb, AppDbError, Ctx};
use crate::persistence::{BackupReason, BackupRow, DataSet};
use crate::platform::time::now_rfc3339;

/// Backups of every project, one folder per project: `<studio data>/app-data-backups/<project-id>/`.
const BACKUPS_DIR: &str = "app-data-backups";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BackupView {
    pub id: String,
    pub which: DataSet,
    pub reason: BackupReason,
    /// The release an automatic backup belongs to.
    pub release_id: Option<String>,
    /// For example "before 1.1.0"; none for manual backups.
    pub label: Option<String>,
    pub size_bytes: f64,
    /// `PRAGMA user_version` of the backed-up data.
    pub schema_version: Option<i32>,
    pub created_at: String,
    /// False when the file was removed outside Studio (restore would fail).
    pub file_exists: bool,
}

impl BackupView {
    fn of(r: &BackupRow) -> Self {
        Self {
            id: r.id.clone(),
            which: r.which,
            reason: r.reason,
            release_id: r.release_id.clone(),
            label: r.label.clone(),
            size_bytes: r.size_bytes as f64,
            schema_version: r.schema_version.and_then(|v| i32::try_from(v).ok()),
            created_at: r.created_at.clone(),
            file_exists: Path::new(&r.path).exists(),
        }
    }
}

/// Which apps use the project's data files right now (DR-15): the UI disables Restore, Copy and
/// Reset with "Stop Preview first" / "Quit <app> first".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataGuard {
    pub preview_running: bool,
    pub installed_running: bool,
    /// The name for "Quit <app> first".
    pub app_name: String,
}

/// What the rollback confirm needs (DR-7) when activating `release_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RollbackCheck {
    /// `PRAGMA user_version` of the installed app's data; none without a data file.
    pub data_schema_version: Option<i32>,
    /// The target release's schema version.
    pub target_schema_version: Option<u32>,
    /// Both are known and the release is older than the data: warn before switching.
    pub older_than_data: bool,
    /// The newest automatic backup of data written by the target release, which is what
    /// "Restore data from before <version>" restores.
    pub backup: Option<BackupView>,
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

fn remove_if_exists(path: &Path) -> Result<(), AppDbError> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(AppDbError::io("remove", path)(e))
        }
        _ => Ok(()),
    }
}

/// The old file's journal must not meet a new main file.
fn remove_journal(db_file: &Path) -> Result<(), AppDbError> {
    remove_if_exists(&sibling(db_file, "-wal"))?;
    remove_if_exists(&sibling(db_file, "-shm"))
}

/// A consistent copy of `src` at `dest` (which must not exist), made on a read-only connection.
pub(crate) fn vacuum_into(src: &Path, dest: &Path) -> Result<(), AppDbError> {
    let conn = open_ro(src)?;
    let r = conn.execute("VACUUM INTO ?1", [dest.to_string_lossy().as_ref()]);
    if let Err(e) = r {
        let _ = std::fs::remove_file(dest);
        return Err(e.into());
    }
    Ok(())
}

fn user_version(path: &Path) -> Option<i64> {
    open_ro(path)
        .ok()?
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .ok()
}

impl AppDb {
    fn backups_dir(&self, project_id: &str) -> PathBuf {
        self.paths.data_dir.join(BACKUPS_DIR).join(project_id)
    }

    /// Copies the data file into a new backup and records it.
    fn backup_of(
        &self,
        ctx: &Ctx,
        which: DataSet,
        reason: BackupReason,
        label: Option<String>,
        release_id: Option<String>,
    ) -> Result<BackupRow, AppDbError> {
        let src = self.data_file(ctx, which);
        if !src.exists() {
            return Err(AppDbError::NoFile);
        }
        let id = uuid::Uuid::new_v4().to_string();
        let dir = self.backups_dir(&ctx.project.id);
        std::fs::create_dir_all(&dir).map_err(AppDbError::io("create", &dir))?;
        let dest = dir.join(format!("{id}.db"));
        vacuum_into(&src, &dest)?;
        let size = std::fs::metadata(&dest)
            .map_err(AppDbError::io("read", &dest))
            .inspect_err(|_| {
                let _ = std::fs::remove_file(&dest);
            })?
            .len();
        let row = BackupRow {
            id,
            project_id: ctx.project.id.clone(),
            which,
            reason,
            release_id,
            label,
            path: dest.to_string_lossy().into_owned(),
            size_bytes: i64::try_from(size).unwrap_or(i64::MAX),
            schema_version: user_version(&dest),
            created_at: now_rfc3339(),
        };
        if let Err(e) = self.db.insert_backup(&row) {
            let _ = std::fs::remove_file(&dest);
            return Err(e.into());
        }
        Ok(row)
    }

    /// "Back up now": a manual backup of Preview or Installed data.
    pub(crate) fn backup(
        &self,
        project_id: &str,
        which: DataSet,
    ) -> Result<BackupView, AppDbError> {
        let ctx = self.require(project_id)?;
        let row = self.backup_of(&ctx, which, BackupReason::Manual, None, None)?;
        Ok(BackupView::of(&row))
    }

    pub(crate) fn list_backups(
        &self,
        project_id: &str,
        which: Option<DataSet>,
    ) -> Result<Vec<BackupView>, AppDbError> {
        Ok(self
            .db
            .list_backups(project_id, which)?
            .iter()
            .map(BackupView::of)
            .collect())
    }

    fn backup_row(&self, project_id: &str, backup_id: &str) -> Result<BackupRow, AppDbError> {
        self.db
            .get_backup(backup_id)?
            .filter(|b| b.project_id == project_id)
            .ok_or(AppDbError::NoSuchBackup)
    }

    /// Deletes a manual or "before restore" backup. Automatic ones go with their release.
    pub(crate) fn delete_backup(
        &self,
        project_id: &str,
        backup_id: &str,
    ) -> Result<(), AppDbError> {
        let row = self.backup_row(project_id, backup_id)?;
        if row.reason == BackupReason::Release {
            return Err(AppDbError::AutomaticBackup);
        }
        remove_if_exists(Path::new(&row.path))?;
        self.db.delete_backup(&row.id)?;
        Ok(())
    }

    /// Replaces the backup's data set with the backup (DR-15). First saves the current data as a
    /// "before restore" backup, which is returned (none when there was no data to save).
    pub(crate) fn restore(
        &self,
        project_id: &str,
        backup_id: &str,
    ) -> Result<Option<BackupView>, AppDbError> {
        let ctx = self.require(project_id)?;
        let row = self.backup_row(project_id, backup_id)?;
        let from = PathBuf::from(&row.path);
        if !from.exists() {
            return Err(AppDbError::BackupFileMissing(from));
        }
        self.ensure_not_running(&ctx, row.which)?;
        let target = self.data_file(&ctx, row.which);
        let before = if target.exists() {
            Some(self.backup_of(
                &ctx,
                row.which,
                BackupReason::Restore,
                Some("before restore".into()),
                None,
            )?)
        } else {
            None
        };
        let parent = target.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).map_err(AppDbError::io("create", parent))?;
        let tmp = sibling(&target, ".restoring");
        std::fs::copy(&from, &tmp).map_err(AppDbError::io("copy", &from))?;
        let swap = remove_journal(&target).and_then(|()| {
            std::fs::rename(&tmp, &target).map_err(AppDbError::io("replace", &target))
        });
        if swap.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        swap?;
        Ok(before.as_ref().map(BackupView::of))
    }

    /// DR-5: replaces Preview data with a copy of the installed app's data. The installed file is
    /// only read.
    pub(crate) fn copy_installed_to_preview(&self, project_id: &str) -> Result<(), AppDbError> {
        let ctx = self.require(project_id)?;
        self.ensure_not_running(&ctx, DataSet::Preview)?;
        let src = self.data_file(&ctx, DataSet::Installed);
        if !src.exists() {
            return Err(AppDbError::NoFile);
        }
        let target = self.data_file(&ctx, DataSet::Preview);
        let parent = target.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).map_err(AppDbError::io("create", parent))?;
        let tmp = sibling(&target, ".copying");
        remove_if_exists(&tmp)?;
        vacuum_into(&src, &tmp)?;
        let swap = remove_journal(&target).and_then(|()| {
            std::fs::rename(&tmp, &target).map_err(AppDbError::io("replace", &target))
        });
        if swap.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        swap
    }

    /// DR-5: deletes Preview data (the file and its WAL files). Installed data is never touched.
    pub(crate) fn reset_preview(&self, project_id: &str) -> Result<(), AppDbError> {
        let ctx = self.require(project_id)?;
        self.ensure_not_running(&ctx, DataSet::Preview)?;
        let target = self.data_file(&ctx, DataSet::Preview);
        remove_journal(&target)?;
        remove_if_exists(&target)
    }

    /// Which apps use the data right now, for disabling the buttons.
    pub(crate) fn guard(&self, project_id: &str) -> Result<DataGuard, AppDbError> {
        let project = self
            .db
            .get_project(project_id)?
            .ok_or(AppDbError::ProjectNotFound)?;
        Ok(DataGuard {
            preview_running: self.preview_running(project_id),
            installed_running: self.installed_running(project_id),
            app_name: self.app_name(&project),
        })
    }

    /// DR-8: deletes the automatic backups of a release whose files were removed. Best effort per
    /// backup; the first error is returned after trying all.
    pub(crate) fn delete_release_backups(&self, release_id: &str) -> Result<(), AppDbError> {
        let mut first = None;
        for b in self.db.list_release_backups(release_id)? {
            let r = remove_if_exists(Path::new(&b.path))
                .and_then(|()| Ok(self.db.delete_backup(&b.id)?));
            if let Err(e) = r {
                first.get_or_insert(e);
            }
        }
        first.map_or(Ok(()), Err)
    }

    /// DR-7: before a release switch, backs up the installed app's data when the project has a
    /// database and the app has created its file. `owner_release_id` is the release whose data
    /// this is (the one being replaced, so the backup lives as long as a rollback to it can
    /// happen), or the new release on a first install. `None`: nothing to back up.
    pub(crate) fn backup_before_activation(
        &self,
        project_id: &str,
        new_version: &str,
        owner_release_id: &str,
    ) -> Result<Option<BackupView>, AppDbError> {
        let Some(ctx) = self.context(project_id)? else {
            return Ok(None);
        };
        if !self.data_file(&ctx, DataSet::Installed).exists() {
            return Ok(None);
        }
        let row = self.backup_of(
            &ctx,
            DataSet::Installed,
            BackupReason::Release,
            Some(format!("before {new_version}")),
            Some(owner_release_id.to_owned()),
        )?;
        Ok(Some(BackupView::of(&row)))
    }

    /// D3: for a build's test launch, puts a `VACUUM INTO` copy of the installed app's data at
    /// `<temp_data_home>/<identifier>/app.db`. `Ok(false)`: nothing to copy (no database entry,
    /// not installed, or the app never created its file). The real file is only read.
    pub(crate) fn copy_installed_for_test(
        &self,
        project_id: &str,
        temp_data_home: &Path,
    ) -> Result<bool, AppDbError> {
        let Some(ctx) = self.context(project_id)? else {
            return Ok(false);
        };
        let src = self.data_file(&ctx, DataSet::Installed);
        if self.db.get_install(project_id)?.is_none() || !src.exists() {
            return Ok(false);
        }
        let dest = self.data_file_in(temp_data_home, &ctx.identifier, DataSet::Installed);
        let parent = dest.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).map_err(AppDbError::io("create", parent))?;
        vacuum_into(&src, &dest)?;
        Ok(true)
    }

    /// DR-7: what the confirm before activating `release_id` needs to know.
    pub(crate) fn rollback_check(
        &self,
        project_id: &str,
        release_id: &str,
    ) -> Result<RollbackCheck, AppDbError> {
        let release = self
            .db
            .get_release(release_id)?
            .filter(|r| r.project_id == project_id)
            .ok_or(AppDbError::NoSuchRelease)?;
        let target = release.schema_version.and_then(|v| u32::try_from(v).ok());
        let mut out = RollbackCheck {
            data_schema_version: None,
            target_schema_version: target,
            older_than_data: false,
            backup: None,
        };
        let Some(ctx) = self.context(project_id)? else {
            return Ok(out);
        };
        let file = self.data_file(&ctx, DataSet::Installed);
        out.data_schema_version = user_version(&file).and_then(|v| i32::try_from(v).ok());
        out.older_than_data = matches!((target, out.data_schema_version),
            (Some(t), Some(d)) if i64::from(t) < i64::from(d));
        out.backup = self
            .db
            .list_release_backups(release_id)?
            .iter()
            .find(|b| b.project_id == project_id && b.which == DataSet::Installed)
            .map(BackupView::of);
        Ok(out)
    }
}
