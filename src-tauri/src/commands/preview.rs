//! Preview commands and events (PRD M3 step 8, preview part): start, stop, restart, the
//! project's latest preview with its phase, the output tail, and the output limits.
//!
//! A view subscribes to `PreviewStatusEvent` / `PreviewLogEvent`, then loads
//! `get_preview_status` and `get_preview_log` and drops log events whose `fromSeq` is below
//! the tail's `nextSeq`.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;
use tokio::sync::broadcast::{Receiver, error::RecvError};

use super::{AppState, CommandError, blocking};
use crate::persistence::{Db, ProjectRow};
use crate::services::diagnostics::{DiagnosticsService, DoctorReport};
use crate::services::preview::{
    PreviewError, PreviewEvent, PreviewLogTail, PreviewStatus, Toolchain,
};
use crate::services::settings::{self, PreviewLimits};

impl From<PreviewError> for CommandError {
    fn from(e: PreviewError) -> Self {
        Self::msg(e.to_string())
    }
}

/// The real doctor behind the preview's `Toolchain`.
pub(crate) struct AppToolchain {
    pub doctor: Arc<DiagnosticsService>,
    pub db: Arc<Db>,
    pub default_projects_dir: PathBuf,
}

impl AppToolchain {
    fn overrides(&self) -> std::collections::HashMap<String, String> {
        settings::load(&self.db, &self.default_projects_dir)
            .map(|s| s.tool_overrides.into_iter().collect())
            .unwrap_or_default()
    }
}

impl Toolchain for AppToolchain {
    fn doctor_report(
        &self,
        force: bool,
    ) -> Pin<Box<dyn Future<Output = DoctorReport> + Send + '_>> {
        Box::pin(async move { self.doctor.run(&self.overrides(), force).await })
    }

    fn spawn_env(&self) -> Vec<(String, String)> {
        self.doctor.spawn_env()
    }

    fn resolve_binary(&self, tool: &str) -> Option<PathBuf> {
        let overrides = self.overrides();
        self.doctor
            .resolve_binary(tool, overrides.get(tool).map(String::as_str))
    }
}

// ------------------------------------------------------------------ events

/// The preview's state or phase changed (also sent once when it starts and once when it ends).
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewStatusEvent {
    pub status: PreviewStatus,
}

/// New output lines, batched every 100 ms.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewLogEvent {
    pub project_id: String,
    pub job_id: String,
    /// Sequence number of the first line; see `PreviewLogTail.nextSeq`.
    pub from_seq: u32,
    pub lines: Vec<String>,
}

pub(crate) fn forward_preview_events(app: tauri::AppHandle, mut rx: Receiver<PreviewEvent>) {
    tauri::async_runtime::spawn(async move {
        loop {
            let sent = match rx.recv().await {
                Ok(PreviewEvent::Status(status)) => PreviewStatusEvent { status }.emit(&app),
                Ok(PreviewEvent::Log {
                    project_id,
                    job_id,
                    from_seq,
                    lines,
                }) => PreviewLogEvent {
                    project_id,
                    job_id,
                    from_seq,
                    lines,
                }
                .emit(&app),
                // The view reloads status and the tail to fill any gap.
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            };
            if let Err(e) = sent {
                eprintln!("could not emit preview event: {e}");
            }
        }
    });
}

// ---------------------------------------------------------------- commands

pub(super) async fn project(st: &AppState, project_id: String) -> Result<ProjectRow, CommandError> {
    let db = st.db.clone();
    blocking(move || {
        db.get_project(&project_id)?
            .ok_or_else(|| CommandError::msg("Project not found."))
    })
    .await
}

/// Starts the project's preview and returns at once; progress arrives as events.
#[tauri::command]
#[specta::specta]
pub(crate) async fn start_preview(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<PreviewStatus, CommandError> {
    let st = state.inner().clone();
    let p = project(&st, project_id).await?;
    Ok(st.preview().start(&p)?)
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn stop_preview(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<(), CommandError> {
    state.preview().stop(&project_id).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn restart_preview(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<PreviewStatus, CommandError> {
    let st = state.inner().clone();
    let p = project(&st, project_id).await?;
    Ok(st.preview().restart(&p).await?)
}

/// The project's latest preview: live (with its phase) or ended this session, else its last
/// stored job ("Last preview ... View log"). `null` when it never had one.
#[tauri::command]
#[specta::specta]
pub(crate) async fn get_preview_status(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Option<PreviewStatus>, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.preview().status(&project_id)?)).await
}

/// The output tail of a preview (default: the project's latest). `null` when there is no
/// preview or its log file was pruned.
#[tauri::command]
#[specta::specta]
pub(crate) async fn get_preview_log(
    state: State<'_, AppState>,
    project_id: String,
    job_id: Option<String>,
) -> Result<Option<PreviewLogTail>, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.preview().log_tail(&project_id, job_id.as_deref())?)).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn get_preview_settings(
    state: State<'_, AppState>,
) -> Result<PreviewLimits, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(settings::load_preview_limits(&st.db))).await
}

/// Validates (lines 1,000-50,000; MB 1-100; kept 1-50) and stores. Applies to previews
/// started afterwards.
#[tauri::command]
#[specta::specta]
pub(crate) async fn save_preview_settings(
    state: State<'_, AppState>,
    limits: PreviewLimits,
) -> Result<PreviewLimits, CommandError> {
    let st = state.inner().clone();
    blocking(move || {
        settings::save_preview_limits(&st.db, limits).map_err(|e| CommandError::msg(e.to_string()))
    })
    .await
}
