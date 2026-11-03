//! Installing generated apps into GNOME (PRD M4 step 5). `Installer` owns policy (what may be
//! activated, uninstalled, repaired) and the `releases`/`installs` rows; a `LauncherAdapter`
//! owns the freedesktop details.
//!
//! The `current.AppImage` symlink is the only record of the active release (M4-D5).
#![allow(dead_code, unused_imports)] // wired up by the commands lane (M4 step 6)

mod launcher;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use crate::persistence::{Db, DbError, InstallRow, ReleaseRow, Smoke};
use crate::platform::paths::{AppLayout, StudioPaths};

pub(crate) use launcher::{EntrySpec, EntryState, LauncherAdapter, XdgLauncher, desktop_entry};

#[derive(Debug, thiserror::Error)]
pub(crate) enum InstallError {
    /// "Could not <step> <path>: <io error>".
    #[error("Could not {step} {}: {source}", path.display())]
    Io {
        step: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    #[error("That release does not exist for this app.")]
    ReleaseNotFound,
    #[error("Version {version} failed its launch test and cannot be installed.")]
    SmokeFailed { version: String },
    #[error("The files of version {version} were deleted; build it again to install it.")]
    FilesRemoved { version: String },
    #[error(
        "Another app installed by Studio already uses the program name \"{binary}\"; GNOME \
         could not tell the two apart."
    )]
    BinaryClash { binary: String },
    #[error(
        "{} exists and was not created by AI App Studio; it was left untouched.",
        .0.display()
    )]
    ForeignFile(PathBuf),
    #[error("The stored file {} does not match the checksum recorded at build time.", .0.display())]
    ChecksumMismatch(PathBuf),
    #[error("This app is not installed.")]
    NotInstalled,
    #[error("The active version is not a release Studio knows, so the launcher cannot be rebuilt.")]
    UnknownRelease,
    #[error("{0}")]
    Launch(String),
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("Internal error: {0}")]
    Internal(String),
}

/// `map_err` helper: `fs::x(..).map_err(io_err("write the launcher entry", &path))`.
pub(super) fn io_err(step: &'static str, path: &Path) -> impl FnOnce(io::Error) -> InstallError {
    let path = path.to_owned();
    move |source| InstallError::Io { step, path, source }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InstallState {
    NotInstalled,
    /// Installed and the symlink matches a kept release.
    Installed {
        release_id: String,
        version: String,
    },
    /// The symlink points at a version without a kept release row.
    UnknownRelease {
        version: Option<String>,
    },
    /// An install row exists but our desktop entry is gone (or replaced): Repair.
    LauncherMissing {
        release_id: Option<String>,
        version: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Activation {
    pub state: InstallState,
    /// What was active before; the UI's Undo activates this release again.
    pub previous_release_id: Option<String>,
    pub previous_version: Option<String>,
}

/// Everything the blocking operations need.
struct Core {
    paths: StudioPaths,
    db: Arc<Db>,
    launcher: Arc<dyn LauncherAdapter>,
}

pub(crate) struct Installer {
    core: Arc<Core>,
    // ponytail: the map only grows (one entry per project ever touched); fine for a desktop app.
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl Installer {
    pub(crate) fn new(paths: StudioPaths, db: Arc<Db>, launcher: Arc<dyn LauncherAdapter>) -> Self {
        Self {
            core: Arc::new(Core {
                paths,
                db,
                launcher,
            }),
            locks: Mutex::default(),
        }
    }

    /// Runs `f` on the blocking pool while holding this project's lock.
    async fn run<T: Send + 'static>(
        &self,
        project_id: &str,
        f: impl FnOnce(&Core) -> Result<T, InstallError> + Send + 'static,
    ) -> Result<T, InstallError> {
        let lock = self
            .locks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(project_id.to_owned())
            .or_default()
            .clone();
        let _guard = lock.lock().await;
        let core = self.core.clone();
        tokio::task::spawn_blocking(move || f(&core))
            .await
            .map_err(|e| InstallError::Internal(e.to_string()))?
    }

    /// Installs a release (first time) or switches to it (activate, rollback: same call).
    pub(crate) async fn install(
        &self,
        project_id: &str,
        release_id: &str,
    ) -> Result<Activation, InstallError> {
        self.activate(project_id, release_id).await
    }

    pub(crate) async fn activate(
        &self,
        project_id: &str,
        release_id: &str,
    ) -> Result<Activation, InstallError> {
        let (p, r) = (project_id.to_owned(), release_id.to_owned());
        self.run(project_id, move |c| c.activate(&p, &r)).await
    }

    pub(crate) async fn state(&self, project_id: &str) -> Result<InstallState, InstallError> {
        let p = project_id.to_owned();
        self.run(project_id, move |c| c.state(&p)).await
    }

    /// Returns the ids of the releases whose files were removed.
    pub(crate) async fn uninstall(&self, project_id: &str) -> Result<Vec<String>, InstallError> {
        let p = project_id.to_owned();
        self.run(project_id, move |c| c.uninstall(&p)).await
    }

    /// `env` is the host spawn env (`Toolchain::spawn_env`, AppImage variables stripped).
    pub(crate) async fn launch(
        &self,
        project_id: &str,
        env: Vec<(String, String)>,
    ) -> Result<(), InstallError> {
        let p = project_id.to_owned();
        self.run(project_id, move |c| c.launch(&p, &env)).await
    }

    pub(crate) async fn repair(&self, project_id: &str) -> Result<InstallState, InstallError> {
        let p = project_id.to_owned();
        self.run(project_id, move |c| c.repair(&p)).await
    }
}

impl Core {
    fn layout(&self, project_id: &str) -> AppLayout {
        AppLayout::new(&self.paths, project_id)
    }

    fn kept_release(&self, project_id: &str, version: &str) -> Result<Option<ReleaseRow>, DbError> {
        Ok(self
            .db
            .list_releases(project_id, false)?
            .into_iter()
            .find(|r| r.version == version))
    }

    fn activate(&self, project_id: &str, release_id: &str) -> Result<Activation, InstallError> {
        let layout = self.layout(project_id);
        let rel = self
            .db
            .get_release(release_id)?
            .filter(|r| r.project_id == project_id)
            .ok_or(InstallError::ReleaseNotFound)?;
        if rel.smoke == Smoke::Failed {
            return Err(InstallError::SmokeFailed {
                version: rel.version,
            });
        }
        if rel.files_removed_at.is_some() {
            return Err(InstallError::FilesRemoved {
                version: rel.version,
            });
        }
        self.check_binary_clash(project_id, &rel)?;
        let entry = self.launcher.entry_state(&layout, project_id)?;
        if entry == EntryState::Foreign {
            return Err(InstallError::ForeignFile(layout.desktop_file().to_owned()));
        }

        let file = layout.release_appimage(&rel.version);
        if sha256_file(&file).map_err(io_err("read the release file", &file))? != rel.sha256 {
            return Err(InstallError::ChecksumMismatch(file));
        }

        let previous_version = layout.active_version().filter(|v| *v != rel.version);
        let previous_release_id = match &previous_version {
            Some(v) => self.kept_release(project_id, v)?.map(|r| r.id),
            None => None,
        };

        self.launcher.place_icon(&layout, &rel.version)?;
        self.launcher.switch(&layout, &rel.version)?;
        // First install: the entry goes last, so GNOME never sees a launcher without a target.
        if self.db.get_install(project_id)?.is_none() {
            self.write_entry(&layout, project_id, &rel)?;
            self.db.upsert_install(&InstallRow {
                project_id: project_id.to_owned(),
                desktop_file: layout.desktop_file().to_string_lossy().into_owned(),
                installed_at: now_rfc3339(),
            })?;
        }
        self.db.set_release_activated(&rel.id)?;
        Ok(Activation {
            state: self.state(project_id)?,
            previous_release_id,
            previous_version,
        })
    }

    fn write_entry(
        &self,
        layout: &AppLayout,
        project_id: &str,
        rel: &ReleaseRow,
    ) -> Result<(), InstallError> {
        let icon = layout.icon();
        self.launcher.write_entry(
            layout,
            &EntrySpec {
                project_id,
                product_name: &rel.product_name,
                main_binary_name: &rel.main_binary_name,
                icon: icon.exists().then_some(icon.as_path()),
            },
        )
    }

    /// M4-D4: GNOME matches windows to launchers by the binary name, so two installed apps
    /// must not share one.
    fn check_binary_clash(&self, project_id: &str, rel: &ReleaseRow) -> Result<(), InstallError> {
        for other in self.db.list_installs()? {
            if other.project_id == project_id {
                continue;
            }
            let Some(v) = self.layout(&other.project_id).active_version() else {
                continue;
            };
            let clash = self
                .kept_release(&other.project_id, &v)?
                .is_some_and(|o| o.main_binary_name == rel.main_binary_name);
            if clash {
                return Err(InstallError::BinaryClash {
                    binary: rel.main_binary_name.clone(),
                });
            }
        }
        Ok(())
    }

    fn state(&self, project_id: &str) -> Result<InstallState, InstallError> {
        if self.db.get_install(project_id)?.is_none() {
            return Ok(InstallState::NotInstalled);
        }
        let layout = self.layout(project_id);
        let version = layout.active_version();
        let release = match &version {
            Some(v) => self.kept_release(project_id, v)?,
            None => None,
        };
        if self.launcher.entry_state(&layout, project_id)? != EntryState::Ours {
            return Ok(InstallState::LauncherMissing {
                release_id: release.map(|r| r.id),
                version,
            });
        }
        Ok(match (version, release) {
            (Some(version), Some(r)) => InstallState::Installed {
                release_id: r.id,
                version,
            },
            (version, _) => InstallState::UnknownRelease { version },
        })
    }

    fn uninstall(&self, project_id: &str) -> Result<Vec<String>, InstallError> {
        let layout = self.layout(project_id);
        // Entry first: GNOME must not keep a launcher whose files are gone. A foreign file
        // refuses the whole operation before anything is deleted.
        self.launcher.remove_entry(&layout, project_id)?;
        match fs::remove_dir_all(layout.root()) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                return Err(io_err("remove the app files", layout.root())(e));
            }
            _ => {}
        }
        let ids: Vec<String> = self
            .db
            .list_releases(project_id, false)?
            .into_iter()
            .map(|r| r.id)
            .collect();
        for id in &ids {
            self.db.mark_release_files_removed(id)?;
        }
        self.db.delete_install(project_id)?;
        Ok(ids)
    }

    fn launch(&self, project_id: &str, env: &[(String, String)]) -> Result<(), InstallError> {
        if self.db.get_install(project_id)?.is_none() {
            return Err(InstallError::NotInstalled);
        }
        self.launcher.launch(&self.layout(project_id), env)
    }

    fn repair(&self, project_id: &str) -> Result<InstallState, InstallError> {
        if self.db.get_install(project_id)?.is_none() {
            return Err(InstallError::NotInstalled);
        }
        let layout = self.layout(project_id);
        match self.launcher.entry_state(&layout, project_id)? {
            EntryState::Foreign => {
                return Err(InstallError::ForeignFile(layout.desktop_file().to_owned()));
            }
            EntryState::Ours => return self.state(project_id),
            EntryState::Missing => {}
        }
        let rel = layout
            .active_version()
            .map(|v| self.kept_release(project_id, &v))
            .transpose()?
            .flatten()
            .ok_or(InstallError::UnknownRelease)?;
        if !layout.icon().exists() {
            self.launcher.place_icon(&layout, &rel.version)?;
        }
        self.write_entry(&layout, project_id, &rel)?;
        self.state(project_id)
    }
}

fn sha256_file(path: &Path) -> io::Result<String> {
    let mut f = fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// UTC `YYYY-MM-DDTHH:MM:SSZ` from the system clock (civil-from-days, Hinnant).
fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rem) = (i64::try_from(secs / 86400).unwrap_or(0), secs % 86400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}
