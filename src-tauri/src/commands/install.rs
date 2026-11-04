//! Install commands (PRD M4 step 6, install part): the app's GNOME state, install / activate /
//! roll back (one call), uninstall, launch, repair, and the Installed Apps list.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use super::{AppState, CommandError, blocking};
use crate::platform::paths::AppLayout;
use crate::services::install::{Activation, InstallError, InstallState};

impl From<InstallError> for CommandError {
    fn from(e: InstallError) -> Self {
        Self::msg(e.to_string())
    }
}

/// One row of the Installed Apps view.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InstalledApp {
    pub project_id: String,
    /// `null` when the project was removed from Studio.
    pub project_name: Option<String>,
    pub state: InstallState,
    pub installed_at: String,
    /// Releases whose files are kept.
    pub releases_kept: u32,
    /// The active release's test result is Unverified.
    pub unverified: bool,
    /// The app folder, for "Show in Files".
    pub folder: String,
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn get_install_state(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<InstallState, CommandError> {
    Ok(state.installer().state(&project_id).await?)
}

/// Installs the release, or makes it the active one when the app is installed (rollback is
/// the same call). The result names what was active before, for Undo.
#[tauri::command]
#[specta::specta]
pub(crate) async fn activate_release(
    state: State<'_, AppState>,
    project_id: String,
    release_id: String,
) -> Result<Activation, CommandError> {
    Ok(state.installer().activate(&project_id, &release_id).await?)
}

/// Removes the launcher, the icon and every release file; the project's source stays.
#[tauri::command]
#[specta::specta]
pub(crate) async fn uninstall_app(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<(), CommandError> {
    let st = state.inner().clone();
    let removed = st.installer().uninstall(&project_id).await?;
    let db = st.db.clone();
    blocking(move || {
        // Best effort: the build refs only pin snapshots of releases that are gone now.
        if let Ok(Some(p)) = db.get_project(&project_id) {
            st.builds().forget_refs(&p, &removed);
        }
        Ok(())
    })
    .await
}

/// Starts the installed app through GNOME (`gio launch`), as Activities would.
#[tauri::command]
#[specta::specta]
pub(crate) async fn launch_installed(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<(), CommandError> {
    let env = state.toolchain_env();
    Ok(state.installer().launch(&project_id, env).await?)
}

/// Writes the launcher entry and icon again when they went missing outside Studio.
#[tauri::command]
#[specta::specta]
pub(crate) async fn repair_launcher(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<InstallState, CommandError> {
    Ok(state.installer().repair(&project_id).await?)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_installed_apps(
    state: State<'_, AppState>,
) -> Result<Vec<InstalledApp>, CommandError> {
    let st = state.inner().clone();
    let db = st.db.clone();
    let installs = blocking(move || {
        db.list_installs()
            .map_err(|e| CommandError::msg(e.to_string()))
    })
    .await?;
    let mut out = Vec::with_capacity(installs.len());
    for i in installs {
        let app_state = st.installer().state(&i.project_id).await?;
        let (db, paths, pid) = (st.db.clone(), st.paths.clone(), i.project_id.clone());
        let active = match &app_state {
            InstallState::Installed { release_id, .. } => Some(release_id.clone()),
            _ => None,
        };
        let (name, kept, unverified, folder) = blocking(move || {
            let err = |e: crate::persistence::DbError| CommandError::msg(e.to_string());
            let name = db.get_project(&pid).map_err(err)?.map(|p| p.name);
            let releases = db.list_releases(&pid, false).map_err(err)?;
            let unverified = releases.iter().any(|r| {
                Some(&r.id) == active.as_ref() && r.smoke == crate::persistence::Smoke::Unverified
            });
            let folder = AppLayout::new(&paths, &pid).root().display().to_string();
            Ok((name, releases.len() as u32, unverified, folder))
        })
        .await?;
        out.push(InstalledApp {
            project_id: i.project_id,
            project_name: name,
            state: app_state,
            installed_at: i.installed_at,
            releases_kept: kept,
            unverified,
            folder,
        });
    }
    Ok(out)
}
