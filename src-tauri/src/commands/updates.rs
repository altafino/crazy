//! Studio's own updates: check GitHub for a newer release, and install it over the running AppImage.

use super::CommandError;
use crate::services::updates::{self, UpdateError, UpdateStatus};

impl From<UpdateError> for CommandError {
    fn from(e: UpdateError) -> Self {
        Self::msg(e.to_string())
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn check_for_update() -> Result<UpdateStatus, CommandError> {
    Ok(updates::check().await?)
}

/// Downloads `version`, verifies its checksum and replaces the running AppImage. The new
/// version runs after the next start.
#[tauri::command]
#[specta::specta]
pub(crate) async fn install_update(version: String) -> Result<(), CommandError> {
    Ok(updates::install(&version).await?)
}

/// Starts Studio again, from the AppImage it runs from: after an update that is the new version.
#[tauri::command]
#[specta::specta]
pub(crate) fn restart_app(app: tauri::AppHandle) {
    app.restart();
}
