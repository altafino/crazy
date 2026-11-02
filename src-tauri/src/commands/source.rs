//! Source tab commands (PRD M3 step 8): thin wrappers over `RepoService`. Errors are the
//! typed `SourceError` so the UI can map each case to its own state.

use std::path::PathBuf;

use tauri::State;

use super::AppState;
use crate::services::git::FileDiff;
use crate::services::git::repo::{
    BranchList, CannotUndo, CommitInfo, CommitOutcome, Identity, IdentityInfo, InitOutcome,
    InitRequest, RepoService, RepoStatus, RestoreReport, SecretFinding, SourceError,
};

fn repo(st: &AppState) -> RepoService {
    let program = st
        .doctor
        .resolve_binary(
            "git",
            st.settings().tool_overrides.get("git").map(String::as_str),
        )
        .unwrap_or_else(|| "git".into());
    RepoService::new(
        program,
        st.doctor.spawn_env(),
        st.db.clone(),
        st.snapshots.clone(),
    )
}

/// Folder of a library project; a missing row is a plain `Invalid`.
fn dir_of(st: &AppState, project_id: &str) -> Result<PathBuf, SourceError> {
    let row = st
        .db
        .get_project(project_id)
        .map_err(|e| SourceError::Git {
            message: e.to_string(),
        })?
        .ok_or_else(|| SourceError::Invalid {
            message: "Project not found.".into(),
        })?;
    Ok(PathBuf::from(row.workspace_path))
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn source_status(
    state: State<'_, AppState>,
    project_id: String,
    ticked: Vec<String>,
) -> Result<RepoStatus, SourceError> {
    let dir = dir_of(&state, &project_id)?;
    repo(&state).status(&project_id, &dir, &ticked).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn source_file_diff(
    state: State<'_, AppState>,
    project_id: String,
    path: String,
    old_path: Option<String>,
    full: bool,
) -> Result<FileDiff, SourceError> {
    let dir = dir_of(&state, &project_id)?;
    repo(&state)
        .file_diff(&dir, &path, old_path.as_deref(), full)
        .await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn source_commit(
    state: State<'_, AppState>,
    project_id: String,
    message: String,
    paths: Vec<String>,
) -> Result<CommitOutcome, SourceError> {
    let dir = dir_of(&state, &project_id)?;
    repo(&state).commit(&dir, &message, &paths).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn source_get_identity(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<IdentityInfo, SourceError> {
    let dir = dir_of(&state, &project_id)?;
    repo(&state).identity(&dir).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn source_set_identity(
    state: State<'_, AppState>,
    project_id: String,
    identity: Identity,
) -> Result<(), SourceError> {
    let dir = dir_of(&state, &project_id)?;
    repo(&state).set_identity(&dir, &identity).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn source_secret_check(
    state: State<'_, AppState>,
    project_id: String,
    paths: Vec<String>,
) -> Result<Vec<SecretFinding>, SourceError> {
    let dir = dir_of(&state, &project_id)?;
    repo(&state).secret_check(&dir, &paths).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn source_list_branches(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<BranchList, SourceError> {
    let dir = dir_of(&state, &project_id)?;
    repo(&state).branches(&dir).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn source_create_branch(
    state: State<'_, AppState>,
    project_id: String,
    name: String,
) -> Result<BranchList, SourceError> {
    let dir = dir_of(&state, &project_id)?;
    repo(&state).create_branch(&dir, &name).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn source_switch_branch(
    state: State<'_, AppState>,
    project_id: String,
    name: String,
) -> Result<BranchList, SourceError> {
    let dir = dir_of(&state, &project_id)?;
    repo(&state).switch_branch(&project_id, &dir, &name).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn source_restore_preview(
    state: State<'_, AppState>,
    project_id: String,
    paths: Vec<String>,
) -> Result<Vec<CannotUndo>, SourceError> {
    let dir = dir_of(&state, &project_id)?;
    repo(&state).restore_preview(&dir, &paths).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn source_restore(
    state: State<'_, AppState>,
    project_id: String,
    paths: Vec<String>,
) -> Result<RestoreReport, SourceError> {
    let dir = dir_of(&state, &project_id)?;
    repo(&state).restore(&project_id, &dir, &paths).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn source_undo_restore(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<(), SourceError> {
    let dir = dir_of(&state, &project_id)?;
    repo(&state).undo_restore(&project_id, &dir).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn source_history(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<CommitInfo>, SourceError> {
    let dir = dir_of(&state, &project_id)?;
    repo(&state).history(&dir).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn source_init_git(
    state: State<'_, AppState>,
    project_id: String,
    request: InitRequest,
) -> Result<InitOutcome, SourceError> {
    let dir = dir_of(&state, &project_id)?;
    repo(&state).init_repo(&project_id, &dir, &request).await
}
