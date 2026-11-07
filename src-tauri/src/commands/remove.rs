//! Remove a project from Studio (PRD F01: separate from deleting its source and from
//! uninstalling its app). Studio forgets the project and deletes what it stored for it:
//! database rows (cascade), snapshots, attachments, release and export files, data backups,
//! job logs and the build refs in the project repo. The project folder stays unless the user
//! ticked "move to Trash"; the app's own data under `~/.local/share/<identifier>` is never
//! touched.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tauri::State;

use super::{AppState, CommandError, blocking};
use crate::persistence::ProjectRow;
use crate::platform::paths::AppLayout;
use crate::security;
use crate::services::jobs::JobKind;
use crate::services::preview::Toolchain;

/// Moves a folder to the Trash; the error is a plain sentence.
type Trash = dyn Fn(&Path) -> Result<(), String> + Send + Sync;

/// Forgets the project and deletes Studio's data for it. With `trash_folder`, the project
/// folder goes to the Trash first, and nothing is removed when that fails. Refused while a
/// run, build, export or GitHub sync runs, and while the app is installed in GNOME. A
/// running preview is stopped.
#[tauri::command]
#[specta::specta]
pub(crate) async fn remove_project(
    state: State<'_, AppState>,
    id: String,
    trash_folder: bool,
) -> Result<(), CommandError> {
    let st = state.inner().clone();
    let gio = st
        .toolchain
        .resolve_binary("gio")
        .unwrap_or_else(|| "gio".into());
    let env = st.doctor.spawn_env();
    let trash = move |p: &Path| gio_trash(&gio, &env, p);
    st.remove_project(id, trash_folder.then_some(Box::new(trash) as Box<Trash>))
        .await
}

fn gio_trash(gio: &Path, env: &[(String, String)], path: &Path) -> Result<(), String> {
    let out = Command::new(gio)
        .arg("trash")
        .arg("--")
        .arg(path)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not run {}: {e}", gio.display()))?;
    if out.status.success() {
        return Ok(());
    }
    let why = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    Err(if why.is_empty() {
        out.status.to_string()
    } else {
        why
    })
}

impl AppState {
    pub(super) async fn remove_project(
        &self,
        id: String,
        trash: Option<Box<Trash>>,
    ) -> Result<(), CommandError> {
        let st = self.clone();
        let pid = id.clone();
        let row = blocking(move || st.removable(&pid)).await?;
        self.preview.stop(&id).await;
        let st = self.clone();
        blocking(move || st.remove_blocking(&row, trash.as_deref())).await
    }

    /// The project row, or why it can't be removed now.
    fn removable(&self, id: &str) -> Result<ProjectRow, CommandError> {
        let row = self
            .db
            .get_project(id)?
            .ok_or_else(|| CommandError::msg("project not found"))?;
        let slots = self.jobs.slots();
        if slots.job_id(id, JobKind::Agent).is_some() {
            return Err(CommandError::msg(
                "An agent run is in progress. Stop it, then remove the project.",
            ));
        }
        if slots.job_id(id, JobKind::Build).is_some() {
            return Err(CommandError::msg(
                "A build or export is running. Stop it, then remove the project.",
            ));
        }
        if self.syncs.is_running(id) {
            return Err(CommandError::msg(
                "A GitHub sync is running. Wait for it or stop it, then remove the project.",
            ));
        }
        if self.db.get_install(id)?.is_some() {
            return Err(CommandError::msg(format!(
                "{} is installed in GNOME. Uninstall it first, then remove the project.",
                row.name
            )));
        }
        Ok(row)
    }

    fn remove_blocking(&self, row: &ProjectRow, trash: Option<&Trash>) -> Result<(), CommandError> {
        let id = row.id.as_str();
        let folder = PathBuf::from(&row.workspace_path);
        let trashed = match trash {
            Some(trash) if folder.exists() => {
                let others: Vec<PathBuf> = self
                    .db
                    .list_projects()?
                    .into_iter()
                    .filter(|p| p.id != id)
                    .map(|p| p.workspace_path.into())
                    .collect();
                security::validate_trash(
                    &folder,
                    &self.paths.data_dir,
                    self.home.as_deref(),
                    &others,
                )
                .map_err(|e| {
                    CommandError::msg(format!("The folder was not moved to the Trash: {}.", e.0))
                })?;
                trash(&folder).map_err(|e| {
                    CommandError::msg(format!("The folder could not be moved to the Trash: {e}"))
                })?;
                true
            }
            _ => false,
        };

        // The kept folder must not carry refs/studio/builds/* that point at nothing.
        if !trashed && folder.is_dir() {
            let ids: Vec<String> = self
                .db
                .list_releases(id, true)?
                .into_iter()
                .map(|r| r.id)
                .collect();
            self.builds.forget_refs(row, &ids);
        }
        self.jobs.attachments().remove_project(&self.db, id)?;
        let logs = self.log_files(id)?;

        // Best effort from here: the row decides whether Studio still knows the project.
        let warn = |what: &str, r: std::io::Result<()>| match r {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                eprintln!("remove project {id}: {what}: {e}");
            }
            _ => {}
        };
        if let Err(e) = self.snapshots.delete_all(id) {
            eprintln!("remove project {id}: snapshots: {e}");
        }
        warn(
            "release files",
            std::fs::remove_dir_all(AppLayout::new(&self.paths, id).root()),
        );
        warn(
            "data backups",
            std::fs::remove_dir_all(self.appdb.backups_dir(id)),
        );
        for log in logs {
            warn("log", std::fs::remove_file(log));
        }
        self.db.delete_project(id)?;
        Ok(())
    }

    /// The project's preview, build and agent logs, all inside Studio's data dir.
    fn log_files(&self, id: &str) -> Result<Vec<PathBuf>, CommandError> {
        let data = &self.paths.data_dir;
        let mut logs: Vec<PathBuf> = self
            .db
            .list_jobs_by_project(id)?
            .into_iter()
            .filter_map(|j| {
                let dir = match j.kind.as_str() {
                    "preview" => "previews",
                    "build" => "builds",
                    _ => return None,
                };
                Some(crate::services::jobs::output::log_path(data, dir, &j.id))
            })
            .collect();
        let sessions = data.join("sessions");
        logs.extend(
            self.db
                .list_sessions_by_project(id)?
                .into_iter()
                .map(|s| PathBuf::from(s.log_path))
                .filter(|p| p.starts_with(&sessions)),
        );
        Ok(logs)
    }
}

#[cfg(test)]
#[path = "remove_tests.rs"]
mod tests;
