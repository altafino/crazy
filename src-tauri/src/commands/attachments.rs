//! Chat attachment commands: stage files for a draft prompt, remove chips, list what earlier
//! runs carried, and the limits. Staged files move into the conversation folder when the run
//! starts (`start_run`'s `draft_id` + `attachment_ids`).

use std::path::PathBuf;

use serde::Serialize;
use specta::Type;
use tauri::State;

use super::{AppState, CommandError, blocking};
use crate::persistence::AttachmentKind;
use crate::services::attachments::{
    AttachError, AttachmentTypeInfo, Pandoc, StagedAttachment, conversation_id, supported_types,
};
use crate::services::preview::Toolchain;
use crate::services::settings::{self, AttachmentLimits};

/// One file's outcome: exactly one of `staged` / `error` is set, so the UI shows a chip per file.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttachResult {
    pub name: String,
    pub staged: Option<StagedAttachment>,
    pub error: Option<AttachError>,
}

impl AttachResult {
    fn new(name: String, r: Result<StagedAttachment, AttachError>) -> Self {
        match r {
            Ok(s) => Self {
                name,
                staged: Some(s),
                error: None,
            },
            Err(e) => Self {
                name,
                staged: None,
                error: Some(e),
            },
        }
    }
}

/// An attachment a run carried, with absolute paths for `convertFileSrc` and the opener.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunAttachment {
    pub run_id: String,
    pub ordinal: u32,
    pub name: String,
    pub kind: AttachmentKind,
    pub size_bytes: u32,
    /// The stored original.
    pub path: String,
    /// True once pruning deleted the files ("File removed").
    pub removed: bool,
}

fn pandoc(st: &AppState) -> Pandoc {
    Pandoc {
        path: st.toolchain.resolve_binary("pandoc"),
        env: st.toolchain.spawn_env(),
    }
}

/// The picker filter and the drop check come from this one list.
#[tauri::command]
#[specta::specta]
pub(crate) async fn attachment_types() -> Vec<AttachmentTypeInfo> {
    supported_types()
}

/// Stages each file for the draft. Never fails as a whole; see `AttachResult`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn attach_files(
    state: State<'_, AppState>,
    draft_id: String,
    paths: Vec<String>,
) -> Result<Vec<AttachResult>, CommandError> {
    let st = state.inner().clone();
    let store = st.jobs().attachments();
    let limits = settings::load_attachment_limits(&st.db);
    let pandoc = pandoc(&st);
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let path = PathBuf::from(&p);
        let name = path
            .file_name()
            .map_or_else(|| p.clone(), |n| n.to_string_lossy().into_owned());
        let r = store.stage_path(&draft_id, &path, &limits, &pandoc).await;
        out.push(AttachResult::new(name, r));
    }
    Ok(out)
}

/// Stages pasted bytes (clipboard images).
#[tauri::command]
#[specta::specta]
pub(crate) async fn attach_pasted(
    state: State<'_, AppState>,
    draft_id: String,
    name: String,
    bytes: Vec<u8>,
) -> Result<AttachResult, CommandError> {
    let st = state.inner().clone();
    let limits = settings::load_attachment_limits(&st.db);
    let r = st
        .jobs()
        .attachments()
        .stage_bytes(&draft_id, &name, &bytes, &limits, &pandoc(&st))
        .await;
    Ok(AttachResult::new(name, r))
}

/// Files copied in a file manager (Ctrl+C in Files), as local paths. WebKitGTK's paste event hides
/// their `text/uri-list` and its Clipboard API returns only the first one, so ask GTK directly.
/// Empty when the clipboard holds no files.
#[tauri::command]
#[specta::specta]
pub(crate) async fn clipboard_file_paths(app: tauri::AppHandle) -> Vec<String> {
    #[cfg(target_os = "linux")]
    {
        use gtk::{gdk, glib};
        let (tx, rx) = tokio::sync::oneshot::channel::<Vec<String>>();
        let tx = std::sync::Mutex::new(Some(tx));
        let asked = app.run_on_main_thread(move || {
            gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD).request_uris(move |_, uris| {
                let paths = uris
                    .iter()
                    .filter_map(|u| glib::filename_from_uri(u).ok())
                    .map(|(p, _)| p.to_string_lossy().into_owned())
                    .collect();
                if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
                    let _ = tx.send(paths);
                }
            });
        });
        if asked.is_err() {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = app;
        Vec::new()
    }
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn remove_attachment(
    state: State<'_, AppState>,
    draft_id: String,
    item_id: String,
) -> Result<(), CommandError> {
    let st = state.inner().clone();
    blocking(move || {
        st.jobs().attachments().remove_staged(&draft_id, &item_id);
        Ok(())
    })
    .await
}

/// The attachments of these runs (session ids), by run then chip order.
#[tauri::command]
#[specta::specta]
pub(crate) async fn list_run_attachments(
    state: State<'_, AppState>,
    run_ids: Vec<String>,
) -> Result<Vec<RunAttachment>, CommandError> {
    let st = state.inner().clone();
    blocking(move || {
        let store = st.jobs().attachments();
        let mut out = Vec::new();
        for r in st.db.list_attachments(&run_ids)? {
            let conv = conversation_id(&st.db, &r.session_id)?;
            out.push(RunAttachment {
                path: store
                    .dir_for(&conv)
                    .join(&r.stored_path)
                    .display()
                    .to_string(),
                run_id: r.session_id,
                ordinal: r.ordinal,
                name: r.name,
                kind: r.kind,
                size_bytes: r.size_bytes,
                removed: r.removed,
            });
        }
        Ok(out)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn get_attachment_limits(
    state: State<'_, AppState>,
) -> Result<AttachmentLimits, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(settings::load_attachment_limits(&st.db))).await
}

/// Validates against the backend bounds and stores. Applies to files attached afterwards.
#[tauri::command]
#[specta::specta]
pub(crate) async fn set_attachment_limits(
    state: State<'_, AppState>,
    limits: AttachmentLimits,
) -> Result<AttachmentLimits, CommandError> {
    let st = state.inner().clone();
    blocking(move || {
        settings::save_attachment_limits(&st.db, limits)
            .map_err(|e| CommandError::msg(e.to_string()))
    })
    .await
}
