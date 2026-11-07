//! Export commands (PRD F22): readiness, start, the exports of a project's releases, the
//! export caches and the Microsoft license answer. Stop is `stop_build` (shared Build slot,
//! D8); progress arrives as `BuildStatusEvent` / `BuildLogEvent` with `status.export` set.

use tauri::State;

use super::preview::project;
use super::{AppState, CommandError, blocking};
use crate::services::builds::BuildStatus;
use crate::services::builds::export::{
    ExportCacheSize, ExportReadiness, ExportTarget, ReleaseExports,
};

/// Docker state, image presence, the license answer and the remembered targets (one probe,
/// at most a few seconds).
#[tauri::command]
#[specta::specta]
pub(crate) async fn export_readiness(
    state: State<'_, AppState>,
) -> Result<ExportReadiness, CommandError> {
    Ok(state.builds().export_readiness().await)
}

/// Starts an export of a kept release and returns at once. `accept_license` records the
/// Microsoft license acceptance (required once before the first Windows export, D10).
/// The targets are remembered for the next dialog.
#[tauri::command]
#[specta::specta]
pub(crate) async fn start_export(
    state: State<'_, AppState>,
    project_id: String,
    release_id: String,
    targets: Vec<ExportTarget>,
    accept_license: bool,
) -> Result<BuildStatus, CommandError> {
    let st = state.inner().clone();
    let p = project(&st, project_id).await?;
    Ok(st
        .builds()
        .start_export(&p, &release_id, targets, accept_license)
        .await?)
}

/// The exports of the project's kept releases (files with SHA-256, latest failure per target).
#[tauri::command]
#[specta::specta]
pub(crate) async fn list_release_exports(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<ReleaseExports>, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.builds().list_release_exports(&project_id)?)).await
}

/// Space used by the export image and download caches (`docker system df -v`, 30 s timeout).
#[tauri::command]
#[specta::specta]
pub(crate) async fn export_cache_size(
    state: State<'_, AppState>,
) -> Result<ExportCacheSize, CommandError> {
    Ok(state.builds().export_cache_size().await?)
}

/// Removes the export download caches and image. Refused while any export runs.
#[tauri::command]
#[specta::specta]
pub(crate) async fn clear_export_caches(state: State<'_, AppState>) -> Result<(), CommandError> {
    Ok(state.builds().clear_export_caches().await?)
}

/// Forgets the Microsoft license acceptance; the next Windows export asks again.
#[tauri::command]
#[specta::specta]
pub(crate) async fn withdraw_export_license(
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    Ok(state.builds().withdraw_export_license()?)
}
