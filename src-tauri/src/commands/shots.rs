//! Screenshots of a created app, for the project cards and Installed Apps.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;
use tokio::sync::broadcast::error::RecvError;

use super::{AppState, CommandError, blocking};
use crate::platform::paths::AppLayout;
use crate::services::shots::{self, ProjectShots};

#[tauri::command]
#[specta::specta]
pub(crate) async fn project_screenshots(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<ProjectShots, CommandError> {
    let (data, paths) = (state.paths.data_dir.clone(), state.paths.clone());
    blocking(move || {
        let layout = AppLayout::new(&paths, &project_id);
        Ok(shots::for_project(
            &data,
            &project_id,
            layout.root(),
            layout.active_version().as_deref(),
        ))
    })
    .await
}

/// A project's screenshots changed: a new one was taken.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScreenshotsChangedEvent {
    pub project_id: String,
}

pub(crate) fn forward_screenshot_events(app: tauri::AppHandle) {
    let mut rx = shots::subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(project_id) => {
                    if let Err(e) = (ScreenshotsChangedEvent { project_id }).emit(&app) {
                        eprintln!("could not emit screenshots event: {e}");
                    }
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    });
}
