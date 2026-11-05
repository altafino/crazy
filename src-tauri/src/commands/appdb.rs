//! App database commands (Database tab): read-only overview and rows of a project's Preview or
//! Installed data, backups and the file actions, the running-app state, the rollback check, and
//! the query time limit setting. Nothing here writes to an app's database.

use tauri::State;

use super::{AppState, CommandError, blocking};
use crate::persistence::DataSet;
use crate::services::appdb::{AppDbOverview, BackupView, DataGuard, RollbackCheck, RowsPage};
use crate::services::settings::{self, DatabaseLimits};

/// Settings > General "Database tab" card.
#[tauri::command]
#[specta::specta]
pub(crate) async fn get_database_limits(
    state: State<'_, AppState>,
) -> Result<DatabaseLimits, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(settings::load_database_limits(&st.db))).await
}

/// Validates (0-60 s, 0 = no limit) and stores. Applies to the next query.
#[tauri::command]
#[specta::specta]
pub(crate) async fn set_database_limits(
    state: State<'_, AppState>,
    limits: DatabaseLimits,
) -> Result<DatabaseLimits, CommandError> {
    let st = state.inner().clone();
    blocking(move || {
        settings::save_database_limits(&st.db, limits).map_err(|e| CommandError::msg(e.to_string()))
    })
    .await
}

/// The state of the project's Preview or Installed data (DR-6), its tables and row counts.
/// A file that cannot be read is `state: "damaged"` with the raw error, not a failure.
#[tauri::command]
#[specta::specta]
pub(crate) async fn appdb_overview(
    state: State<'_, AppState>,
    project_id: String,
    which: DataSet,
) -> Result<AppDbOverview, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.appdb().overview(&project_id, which)?)).await
}

/// One page of 50 rows. `filter` keeps rows where any column contains the text. When the page
/// hits the query time limit, `timedOut` is true and `rows` is empty.
#[tauri::command]
#[specta::specta]
pub(crate) async fn appdb_rows(
    state: State<'_, AppState>,
    project_id: String,
    which: DataSet,
    table: String,
    offset: u32,
    filter: Option<String>,
) -> Result<RowsPage, CommandError> {
    let st = state.inner().clone();
    blocking(move || {
        Ok(st
            .appdb()
            .rows(&project_id, which, &table, offset, filter.as_deref())?)
    })
    .await
}

/// Whether Preview or the installed app runs, for disabling Restore, Copy and Reset
/// ("Stop Preview first", "Quit <app> first").
#[tauri::command]
#[specta::specta]
pub(crate) async fn appdb_guard(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<DataGuard, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.appdb().guard(&project_id)?)).await
}

/// Backups of the project, newest first; `which` narrows to Preview or Installed.
#[tauri::command]
#[specta::specta]
pub(crate) async fn appdb_list_backups(
    state: State<'_, AppState>,
    project_id: String,
    which: Option<DataSet>,
) -> Result<Vec<BackupView>, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.appdb().list_backups(&project_id, which)?)).await
}

/// "Back up now": a manual backup of Preview or Installed data.
#[tauri::command]
#[specta::specta]
pub(crate) async fn appdb_backup(
    state: State<'_, AppState>,
    project_id: String,
    which: DataSet,
) -> Result<BackupView, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.appdb().backup(&project_id, which)?)).await
}

/// Deletes a manual or "before restore" backup. Automatic "before <version>" backups are
/// refused: they go with their release.
#[tauri::command]
#[specta::specta]
pub(crate) async fn appdb_delete_backup(
    state: State<'_, AppState>,
    project_id: String,
    backup_id: String,
) -> Result<(), CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.appdb().delete_backup(&project_id, &backup_id)?)).await
}

/// Replaces the backup's data with the backup, after saving the current data as a "before
/// restore" backup (returned; `null` when there was no data). Refused while the app that uses
/// the file runs.
#[tauri::command]
#[specta::specta]
pub(crate) async fn appdb_restore_backup(
    state: State<'_, AppState>,
    project_id: String,
    backup_id: String,
) -> Result<Option<BackupView>, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.appdb().restore(&project_id, &backup_id)?)).await
}

/// Replaces Preview data with a copy of the installed app's data. Refused while Preview runs.
#[tauri::command]
#[specta::specta]
pub(crate) async fn appdb_copy_installed_to_preview(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<(), CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.appdb().copy_installed_to_preview(&project_id)?)).await
}

/// Deletes Preview data. Refused while Preview runs. Installed data is never touched.
#[tauri::command]
#[specta::specta]
pub(crate) async fn appdb_reset_preview(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<(), CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.appdb().reset_preview(&project_id)?)).await
}

/// For the confirm before Install, Activate or Roll back to `release_id` (DR-7): the data's
/// schema version against the release's, and the backup "Restore data from before ..." uses.
#[tauri::command]
#[specta::specta]
pub(crate) async fn appdb_rollback_check(
    state: State<'_, AppState>,
    project_id: String,
    release_id: String,
) -> Result<RollbackCheck, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.appdb().rollback_check(&project_id, &release_id)?)).await
}
