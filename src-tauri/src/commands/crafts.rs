//! ArtCraft commands (composer popover, design D18-D23): which Crafting Apps are installed and
//! which of them Claude may use in this project. Each change saves at once (design D23).

use std::path::PathBuf;

use tauri::State;

use super::{AppState, CommandError, blocking, preview::project};
use crate::domain::ProjectMeta;
use crate::services::crafts::{self, CraftAppsView, DoctorCrafts};

/// Installed ArtCraft apps and this project's choice.
#[tauri::command]
#[specta::specta]
pub(crate) async fn craft_apps(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<CraftAppsView, CommandError> {
    view(state.inner(), project_id).await
}

/// Saves the apps Claude may use in this project; an empty list turns ArtCraft off.
/// Returns the popover's new state.
#[tauri::command]
#[specta::specta]
pub(crate) async fn set_craft_apps(
    state: State<'_, AppState>,
    project_id: String,
    apps: Vec<String>,
) -> Result<CraftAppsView, CommandError> {
    let st = state.inner();
    let dir = PathBuf::from(project(st, project_id.clone()).await?.workspace_path);
    blocking(move || crafts::save_choice(&dir, &apps).map_err(CommandError::msg)).await?;
    view(st, project_id).await
}

/// The Doctor's "ArtCraft apps" table: what is installed and whether Claude can use it.
#[tauri::command]
#[specta::specta]
pub(crate) async fn craft_doctor(state: State<'_, AppState>) -> Result<DoctorCrafts, CommandError> {
    let st = state.inner();
    let detection = crafts::detect(
        &st.doctor.spawn_env(),
        &st.doctor.resolved_path(),
        st.home.as_deref(),
    )
    .await;
    Ok(crafts::doctor_view(&detection))
}

async fn view(st: &AppState, project_id: String) -> Result<CraftAppsView, CommandError> {
    let dir = PathBuf::from(project(st, project_id).await?.workspace_path);
    let detection = crafts::detect(
        &st.doctor.spawn_env(),
        &st.doctor.resolved_path(),
        st.home.as_deref(),
    )
    .await;
    let chosen = blocking(move || {
        let meta = ProjectMeta::read(&dir).map_err(|e| CommandError::msg(e.to_string()))?;
        Ok(meta.and_then(|m| m.crafts))
    })
    .await?;
    Ok(crafts::apps_view(
        &detection,
        chosen.as_ref(),
        &st.paths.cache_dir.join(crafts::EXTRACT_DIR),
    ))
}
