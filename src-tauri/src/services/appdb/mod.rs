//! App databases (Phase 1, SQLite): Studio's view of the database a generated app keeps in
//! `<XDG_DATA_HOME>/<identifier>/` (`dev/app.db` for Preview, `app.db` for the installed app).
//!
//! One owner (`AppDb`) with two internal halves: `inspect` (read-only overview and rows, under
//! a query time limit) and `files` (backups, restore, copy, reset, the running-app guards and
//! the release hooks). Nothing here ever writes to an app's database; file actions replace or
//! delete whole files, and only while the app that uses the file is not running (DR-15).
//! The convention (file names, migrations folder) lives in `convention` and nowhere else.

pub(crate) mod convention;
mod files;
mod inspect;
#[cfg(test)]
mod tests;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::ProjectMeta;
use crate::persistence::{Db, DbError, ProjectRow};
use crate::platform::paths::{AppLayout, StudioPaths};
use crate::services::jobs::{JobKind, Slots};
use crate::services::projects::tauri::read_tauri_config;
use crate::services::settings;

#[cfg(test)]
use crate::persistence::BackupReason;
pub(crate) use crate::persistence::DataSet;
pub(crate) use files::{BackupView, DataGuard, RollbackCheck};
pub(crate) use inspect::{AppDbOverview, RowsPage};
#[cfg(test)]
use inspect::{Cell, DbState};

/// Rows per page in the Database tab.
pub(crate) const PAGE_SIZE: u32 = 50;

#[derive(Debug, thiserror::Error)]
pub(crate) enum AppDbError {
    #[error("That project does not exist.")]
    ProjectNotFound,
    #[error("This app does not store data in a database yet.")]
    NoDatabase,
    #[error("Couldn't read the app identifier from src-tauri/tauri.conf.json.")]
    NoIdentifier,
    #[error("The app identifier {0:?} is not valid, so no data folder was touched.")]
    BadIdentifier(String),
    #[error("There is no data file yet.")]
    NoFile,
    #[error("There is no table {0:?}.")]
    NoSuchTable(String),
    #[error("That release does not exist for this app.")]
    NoSuchRelease,
    #[error("That backup does not exist.")]
    NoSuchBackup,
    #[error("The backup file is missing: {}", .0.display())]
    BackupFileMissing(PathBuf),
    #[error("Automatic backups are deleted together with their release.")]
    AutomaticBackup,
    #[error("Quit {app} first.")]
    InstalledRunning { app: String },
    #[error("Stop Preview first.")]
    PreviewRunning,
    #[error("Could not {step} {}: {source}", path.display())]
    Io {
        step: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    #[error("{0}")]
    Sqlite(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

impl AppDbError {
    fn io(step: &'static str, path: &Path) -> impl FnOnce(io::Error) -> Self {
        let path = path.to_owned();
        move |source| Self::Io { step, path, source }
    }
}

impl From<rusqlite::Error> for AppDbError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e.to_string())
    }
}

/// Finds running apps. A trait so tests can say what runs.
pub(crate) trait ProcessScan: Send + Sync {
    /// The `APPIMAGE` value of every live process that has one (the AppImage runtime sets it
    /// for the app and everything the app starts).
    fn appimage_vars(&self) -> Vec<PathBuf>;
}

/// Reads `<root>/<pid>/environ` of every process Studio may look at (its own user's).
pub(crate) struct Procfs {
    root: PathBuf,
}

impl Procfs {
    pub(crate) fn new() -> Self {
        Self::at("/proc")
    }

    pub(crate) fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl ProcessScan for Procfs {
    fn appimage_vars(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return vec![];
        };
        let mut out = Vec::new();
        for e in entries.flatten() {
            // Processes of other users and kernel threads fail to read; that is fine.
            let Ok(env) = std::fs::read(e.path().join("environ")) else {
                continue;
            };
            out.extend(
                env.split(|b| *b == 0)
                    .filter_map(|kv| kv.strip_prefix(b"APPIMAGE="))
                    .filter_map(|v| std::str::from_utf8(v).ok())
                    .map(PathBuf::from),
            );
        }
        out
    }
}

/// A project that has a database: its row and the checked app identifier.
pub(crate) struct Ctx {
    pub project: ProjectRow,
    pub identifier: String,
}

pub(crate) struct AppDb {
    db: Arc<Db>,
    paths: StudioPaths,
    /// `<XDG_DATA_HOME>`: parent of Studio's own data folder, where every app's folder lives.
    data_home: PathBuf,
    slots: Arc<Slots>,
    procs: Arc<dyn ProcessScan>,
}

impl AppDb {
    /// With no live jobs and the real `/proc`; `with_slots` and `with_procs` replace them.
    pub(crate) fn new(db: Arc<Db>, paths: &StudioPaths) -> Self {
        Self {
            db,
            data_home: paths
                .data_dir
                .parent()
                .map_or_else(|| paths.data_dir.clone(), Path::to_path_buf),
            paths: paths.clone(),
            slots: Arc::default(),
            procs: Arc::new(Procfs::new()),
        }
    }

    /// The live-job table, to know whether Preview runs.
    pub(crate) fn with_slots(mut self, slots: Arc<Slots>) -> Self {
        self.slots = slots;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_procs(mut self, procs: Arc<dyn ProcessScan>) -> Self {
        self.procs = procs;
        self
    }

    /// The project's database context, `None` when its project.json has no database entry.
    pub(crate) fn context(&self, project_id: &str) -> Result<Option<Ctx>, AppDbError> {
        let project = self
            .db
            .get_project(project_id)?
            .ok_or(AppDbError::ProjectNotFound)?;
        let dir = Path::new(&project.workspace_path);
        // A project.json Studio can't read means no entry it can act on.
        let has_entry = matches!(ProjectMeta::read(dir), Ok(Some(m))
            if m.database.as_ref().is_some_and(|d| d.engine == convention::ENGINE_SQLITE));
        if !has_entry {
            return Ok(None);
        }
        let identifier = read_tauri_config(dir)
            .and_then(|c| c.identifier)
            .ok_or(AppDbError::NoIdentifier)?;
        validate_identifier(&identifier)?;
        Ok(Some(Ctx {
            project,
            identifier,
        }))
    }

    fn require(&self, project_id: &str) -> Result<Ctx, AppDbError> {
        self.context(project_id)?.ok_or(AppDbError::NoDatabase)
    }

    /// `<XDG_DATA_HOME>/<identifier>/<convention file>`. The identifier was validated.
    fn data_file(&self, ctx: &Ctx, which: DataSet) -> PathBuf {
        self.data_file_in(&self.data_home, &ctx.identifier, which)
    }

    fn data_file_in(&self, home: &Path, identifier: &str, which: DataSet) -> PathBuf {
        home.join(identifier).join(match which {
            DataSet::Preview => convention::PREVIEW_DB,
            DataSet::Installed => convention::INSTALLED_DB,
        })
    }

    fn query_limit(&self) -> Option<std::time::Duration> {
        let secs = settings::load_database_limits(&self.db).query_time_limit_secs;
        (secs > 0).then(|| std::time::Duration::from_secs(u64::from(secs)))
    }

    /// The name the user knows the app by: the newest release's product name, else the project's.
    fn app_name(&self, project: &ProjectRow) -> String {
        self.db
            .list_releases(&project.id, true)
            .ok()
            .and_then(|r| r.into_iter().next())
            .map_or_else(|| project.name.clone(), |r| r.product_name)
    }

    fn installed_running(&self, project_id: &str) -> bool {
        let root = AppLayout::new(&self.paths, project_id).root().to_owned();
        self.procs
            .appimage_vars()
            .iter()
            .any(|p| p.starts_with(&root))
    }

    fn preview_running(&self, project_id: &str) -> bool {
        self.slots.job_id(project_id, JobKind::Preview).is_some()
    }

    /// Refuses while the app that uses `which`'s file runs (DR-15).
    fn ensure_not_running(&self, ctx: &Ctx, which: DataSet) -> Result<(), AppDbError> {
        match which {
            DataSet::Preview if self.preview_running(&ctx.project.id) => {
                Err(AppDbError::PreviewRunning)
            }
            DataSet::Installed if self.installed_running(&ctx.project.id) => {
                Err(AppDbError::InstalledRunning {
                    app: self.app_name(&ctx.project),
                })
            }
            _ => Ok(()),
        }
    }
}

/// Reverse-DNS-like: letters, digits, `.`, `-`, `_`; at least two non-empty segments, none
/// starting with `.`. Rules out `/`, `..`, and an empty name before it becomes a path.
pub(crate) fn validate_identifier(id: &str) -> Result<(), AppDbError> {
    let ok = id.len() <= 200
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        && id.split('.').count() >= 2
        && id.split('.').all(|seg| !seg.is_empty());
    if ok {
        Ok(())
    } else {
        Err(AppDbError::BadIdentifier(id.to_owned()))
    }
}
