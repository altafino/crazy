//! Build and release commands and events (PRD M4 step 6, build part): start, stop, the
//! project's latest build with its phase, the output tail, the suggested version, the release
//! table and deleting a release.
//!
//! Like preview: a view subscribes to `BuildStatusEvent` / `BuildLogEvent`, then loads
//! `get_build_status` and `get_build_log` and drops log events below the tail's `nextSeq`.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;
use tokio::sync::broadcast::{Receiver, error::RecvError};

use super::preview::project;
use super::{AppState, CommandError, blocking};
use crate::persistence::{ReleaseRow, Smoke};
use crate::platform::paths::AppLayout;
use crate::services::builds::{BuildError, BuildEvent, BuildLogTail, BuildStatus};

impl From<BuildError> for CommandError {
    fn from(e: BuildError) -> Self {
        Self::msg(e.to_string())
    }
}

// ------------------------------------------------------------------ events

/// The build's state or phase changed (also sent once when it starts and once when it ends).
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BuildStatusEvent {
    pub status: BuildStatus,
}

/// New output lines, batched every 100 ms.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BuildLogEvent {
    pub project_id: String,
    pub job_id: String,
    /// Sequence number of the first line; see `BuildLogTail.nextSeq`.
    pub from_seq: u32,
    pub lines: Vec<String>,
}

pub(crate) fn forward_build_events(app: tauri::AppHandle, mut rx: Receiver<BuildEvent>) {
    tauri::async_runtime::spawn(async move {
        loop {
            let sent = match rx.recv().await {
                Ok(BuildEvent::Status(status)) => BuildStatusEvent { status: *status }.emit(&app),
                Ok(BuildEvent::Log {
                    project_id,
                    job_id,
                    from_seq,
                    lines,
                }) => BuildLogEvent {
                    project_id,
                    job_id,
                    from_seq,
                    lines,
                }
                .emit(&app),
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            };
            if let Err(e) = sent {
                eprintln!("could not emit build event: {e}");
            }
        }
    });
}

// ------------------------------------------------------------------- views

/// One row of the release table.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReleaseView {
    pub id: String,
    pub version: String,
    pub created_at: String,
    /// Bytes; a float because TypeScript has no 64-bit integer.
    pub size_bytes: f64,
    pub source_commit: Option<String>,
    pub dirty_source: bool,
    pub smoke: Smoke,
    pub smoke_detail: Option<String>,
    pub notes: Option<String>,
    pub last_activated_at: Option<String>,
    pub files_removed: bool,
    /// Highest migration number in the built source ("Database changes: schema 3 -> 4");
    /// `null` when the app had no migrations.
    pub schema_version: Option<u32>,
    /// The stored AppImage, for "Show in Files".
    pub appimage_path: String,
}

fn release_view(layout: &AppLayout, r: ReleaseRow) -> ReleaseView {
    ReleaseView {
        appimage_path: layout.release_appimage(&r.version).display().to_string(),
        id: r.id,
        version: r.version,
        created_at: r.created_at,
        size_bytes: r.size_bytes as f64,
        source_commit: r.source_commit,
        dirty_source: r.dirty_source,
        smoke: r.smoke,
        smoke_detail: r.smoke_detail,
        notes: r.notes,
        last_activated_at: r.last_activated_at,
        files_removed: r.files_removed_at.is_some(),
        schema_version: r.schema_version.and_then(|v| u32::try_from(v).ok()),
    }
}

// ---------------------------------------------------------------- commands

/// Starts a release build and returns at once; progress arrives as events.
#[tauri::command]
#[specta::specta]
pub(crate) async fn start_build(
    state: State<'_, AppState>,
    project_id: String,
    version: String,
    notes: Option<String>,
) -> Result<BuildStatus, CommandError> {
    let st = state.inner().clone();
    let p = project(&st, project_id).await?;
    Ok(st.builds().start(&p, version, notes)?)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn stop_build(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<(), CommandError> {
    state.builds().stop(&project_id).await;
    Ok(())
}

/// The project's latest build (live, ended this session, or the last stored one); `null`
/// when it never had one.
#[tauri::command]
#[specta::specta]
pub(crate) async fn get_build_status(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Option<BuildStatus>, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.builds().status(&project_id)?)).await
}

/// The output tail of a build (default: the project's latest).
#[tauri::command]
#[specta::specta]
pub(crate) async fn get_build_log(
    state: State<'_, AppState>,
    project_id: String,
    job_id: Option<String>,
) -> Result<Option<BuildLogTail>, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.builds().log_tail(&project_id, job_id.as_deref())?)).await
}

/// The version the build dialog pre-fills.
#[tauri::command]
#[specta::specta]
pub(crate) async fn next_build_version(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<String, CommandError> {
    let st = state.inner().clone();
    let p = project(&st, project_id).await?;
    blocking(move || Ok(st.builds().next_version(&p)?)).await
}

/// The project's releases, newest first; `includeRemoved` adds those whose files are gone.
#[tauri::command]
#[specta::specta]
pub(crate) async fn list_releases(
    state: State<'_, AppState>,
    project_id: String,
    include_removed: bool,
) -> Result<Vec<ReleaseView>, CommandError> {
    let st = state.inner().clone();
    blocking(move || {
        let layout = AppLayout::new(&st.paths, &project_id);
        Ok(st
            .db
            .list_releases(&project_id, include_removed)
            .map_err(|e| CommandError::msg(e.to_string()))?
            .into_iter()
            .map(|r| release_view(&layout, r))
            .collect())
    })
    .await
}

/// Deletes a release's files (refused for the active one); the row stays as "removed".
#[tauri::command]
#[specta::specta]
pub(crate) async fn delete_release(
    state: State<'_, AppState>,
    project_id: String,
    release_id: String,
) -> Result<(), CommandError> {
    Ok(state
        .builds()
        .delete_release(&project_id, &release_id)
        .await?)
}
