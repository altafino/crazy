//! Chat attachments (images, PDF, text/code, Word): staging, checks, DOCX conversion, storage,
//! pruning. This service alone writes and deletes attachment files and `run_attachments` rows
//! (eng review R8c).
#![allow(dead_code)] // consumed by the attachment commands (next step); remove then
//!
//! Layout under `<data>/attachments/`:
//! - `.staging/<draft-id>/<item-id>/<safe-name>` while a prompt is being written (D9);
//! - `<conversation-id>/<item-id>/<safe-name>` once the run started (D5). The conversation id is
//!   the first run's session id, found through `resumed_from` ([`conversation_id`]).
//!
//! Files are always copied (never linked) and pasted bytes are written the same way. The service
//! hands out folders only; Claude-specific flags belong to the agent adapter (R8b).

use crate::persistence::{AttachmentKind, AttachmentRow, Db, DbError};
use crate::services::diagnostics::spawn::{RunError, run_capped_in};
use crate::services::git::is_secret;
use crate::services::settings::AttachmentLimits;
use serde::Serialize;
use specta::Type;
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
    time::{Duration, SystemTime},
};

#[cfg(test)]
mod tests;

const MB: u64 = 1024 * 1024;
const STAGING: &str = ".staging";

/// The one list of supported types (R8a): kind, lowercase extensions, mime type. The picker
/// filter comes from [`supported_types`]; [`classify`] reads the same table.
const TYPES: &[(AttachmentKind, &[&str], &str)] = &[
    (AttachmentKind::Image, &["png"], "image/png"),
    (AttachmentKind::Image, &["jpg", "jpeg"], "image/jpeg"),
    (AttachmentKind::Image, &["gif"], "image/gif"),
    (AttachmentKind::Image, &["webp"], "image/webp"),
    (AttachmentKind::Pdf, &["pdf"], "application/pdf"),
    (
        AttachmentKind::Docx,
        &["docx"],
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    (
        AttachmentKind::Text,
        &[
            "txt", "md", "markdown", "log", "csv", "tsv", "json", "jsonl", "yaml", "yml", "toml",
            "ini", "cfg", "conf", "xml", "svg", "html", "htm", "css", "scss", "js", "mjs", "cjs",
            "ts", "tsx", "jsx", "vue", "svelte", "py", "rb", "go", "rs", "java", "kt", "swift",
            "c", "h", "cpp", "hpp", "cs", "php", "sh", "bash", "zsh", "sql", "lua", "dart",
        ],
        "text/plain",
    ),
];

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttachmentTypeInfo {
    pub kind: AttachmentKind,
    pub extensions: Vec<String>,
    pub mime: String,
}

/// What the file picker should offer (returned by the later `attachment_types` command).
pub(crate) fn supported_types() -> Vec<AttachmentTypeInfo> {
    TYPES
        .iter()
        .map(|(kind, exts, mime)| AttachmentTypeInfo {
            kind: *kind,
            extensions: exts.iter().map(|e| (*e).to_owned()).collect(),
            mime: (*mime).to_owned(),
        })
        .collect()
}

fn classify(name: &str) -> Option<(AttachmentKind, String)> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    TYPES
        .iter()
        .find(|(_, exts, _)| exts.contains(&ext.as_str()))
        .map(|(kind, ..)| (*kind, ext))
}

/// Why one file was refused. `reason` is the stable machine key the UI maps to its error chip;
/// the `Display` text is the user-facing message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Type)]
#[serde(tag = "reason", rename_all = "camelCase")]
pub(crate) enum AttachError {
    #[error("This file type isn't supported yet")]
    Unsupported,
    #[error("This file is larger than the {mb} MB limit")]
    TooLarge { mb: u32 },
    #[error("You can attach up to {max} files to one message")]
    TooManyFiles { max: u32 },
    #[error("The files together are larger than the {mb} MB limit")]
    TotalTooLarge { mb: u32 },
    #[error("This file looks like it holds secrets, so it can't be attached")]
    SecretFile,
    #[error("Install pandoc to attach Word files")]
    PandocMissing,
    #[error("Could not convert this Word file: {detail}")]
    ConversionFailed { detail: String },
    #[error("Converting this Word file took too long")]
    ConversionTimedOut,
    #[error("Could not copy this file: {detail}")]
    CopyFailed { detail: String },
    #[error("An attached file is missing. Remove it and try again.")]
    FileMissing,
}

fn copy_failed(e: impl std::fmt::Display) -> AttachError {
    AttachError::CopyFailed {
        detail: e.to_string(),
    }
}

/// How to run pandoc: injectable so tests use a fake script. `path: None` means not installed.
pub(crate) struct Pandoc {
    pub path: Option<PathBuf>,
    pub env: Vec<(String, String)>,
}

/// A file waiting in a draft. Paths are relative to the staging draft folder now and to the
/// conversation folder after [`AttachmentStore::commit`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StagedAttachment {
    /// Pass back to [`AttachmentStore::remove_staged`] / [`AttachmentStore::commit`].
    pub id: String,
    /// The name the user saw.
    pub name: String,
    pub kind: AttachmentKind,
    pub size_bytes: u32,
    /// `<id>/<safe-name>`.
    pub stored_path: String,
    /// DOCX only: `<id>/<name>.md`, what Claude reads.
    pub converted_path: Option<String>,
}

struct Staged {
    draft: String,
    info: StagedAttachment,
}

enum Source<'a> {
    Path(&'a Path),
    Bytes(&'a [u8]),
}

pub(crate) struct AttachmentStore {
    root: PathBuf,
    /// Drafts live only while Studio runs (a restart loses the chips too); the 24 h sweep
    /// removes what a crash left on disk.
    staged: Mutex<HashMap<String, Staged>>,
}

impl AttachmentStore {
    pub(crate) fn new(data_dir: &Path) -> Self {
        Self {
            root: data_dir.join("attachments"),
            staged: Mutex::default(),
        }
    }

    /// The folder holding every file of a conversation; the adapter allows Claude to read it.
    pub(crate) fn dir_for(&self, conversation_id: &str) -> PathBuf {
        self.root.join(conversation_id)
    }

    fn staging(&self) -> PathBuf {
        self.root.join(STAGING)
    }

    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, Staged>> {
        self.staged.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Copies `src` into the draft after checking type, secrets and limits; converts DOCX.
    pub(crate) async fn stage_path(
        &self,
        draft_id: &str,
        src: &Path,
        limits: &AttachmentLimits,
        pandoc: &Pandoc,
    ) -> Result<StagedAttachment, AttachError> {
        let name = src
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(AttachError::Unsupported)?;
        self.stage(draft_id, name, Source::Path(src), limits, pandoc)
            .await
    }

    /// Same as [`Self::stage_path`] for pasted bytes (e.g. a clipboard image).
    pub(crate) async fn stage_bytes(
        &self,
        draft_id: &str,
        name: &str,
        bytes: &[u8],
        limits: &AttachmentLimits,
        pandoc: &Pandoc,
    ) -> Result<StagedAttachment, AttachError> {
        self.stage(draft_id, name, Source::Bytes(bytes), limits, pandoc)
            .await
    }

    async fn stage(
        &self,
        draft_id: &str,
        name: &str,
        src: Source<'_>,
        limits: &AttachmentLimits,
        pandoc: &Pandoc,
    ) -> Result<StagedAttachment, AttachError> {
        if !valid_id(draft_id) {
            return Err(copy_failed("bad draft id"));
        }
        let display = name.rsplit(['/', '\\']).next().unwrap_or(name);
        let (kind, ext) = classify(display).ok_or(AttachError::Unsupported)?;
        if is_secret(display) {
            return Err(AttachError::SecretFile);
        }
        let size = match &src {
            Source::Path(p) => {
                // A symlink must not smuggle a secret in under an innocent name.
                if p.canonicalize()
                    .is_ok_and(|real| is_secret(&real.to_string_lossy()))
                {
                    return Err(AttachError::SecretFile);
                }
                let m = fs::metadata(p).map_err(copy_failed)?; // follows symlinks
                if !m.is_file() {
                    return Err(copy_failed("not a regular file"));
                }
                m.len()
            }
            Source::Bytes(b) => b.len() as u64,
        };
        let kind_mb = match kind {
            AttachmentKind::Image => limits.max_image_mb,
            AttachmentKind::Pdf => limits.max_pdf_mb,
            AttachmentKind::Text => limits.max_text_mb,
            AttachmentKind::Docx => limits.max_docx_mb,
        };
        if size > u64::from(kind_mb) * MB {
            return Err(AttachError::TooLarge { mb: kind_mb });
        }

        let id = uuid::Uuid::new_v4().to_string();
        let safe = safe_name(display);
        let info = StagedAttachment {
            id: id.clone(),
            name: display.to_owned(),
            kind,
            size_bytes: u32::try_from(size).unwrap_or(u32::MAX),
            stored_path: format!("{id}/{safe}"),
            converted_path: None,
        };
        {
            // Check and reserve under one lock so parallel attaches cannot overshoot.
            let mut m = self.map();
            let (count, total) = m
                .values()
                .filter(|s| s.draft == draft_id)
                .fold((0u32, 0u64), |(c, t), s| {
                    (c + 1, t + u64::from(s.info.size_bytes))
                });
            if count >= limits.max_files {
                return Err(AttachError::TooManyFiles {
                    max: limits.max_files,
                });
            }
            if total + size > u64::from(limits.max_total_mb) * MB {
                return Err(AttachError::TotalTooLarge {
                    mb: limits.max_total_mb,
                });
            }
            m.insert(
                id.clone(),
                Staged {
                    draft: draft_id.to_owned(),
                    info: info.clone(),
                },
            );
        }

        let dir = self.staging().join(draft_id).join(&id);
        match fill(&dir, &safe, &ext, kind, kind_mb, &src, limits, pandoc).await {
            Ok(converted) => {
                let converted_path = converted.map(|c| format!("{id}/{c}"));
                if let Some(s) = self.map().get_mut(&id) {
                    s.info.converted_path.clone_from(&converted_path);
                }
                Ok(StagedAttachment {
                    converted_path,
                    ..info
                })
            }
            Err(e) => {
                self.map().remove(&id);
                let _ = fs::remove_dir_all(&dir);
                Err(e)
            }
        }
    }

    /// The user removed a chip: its staged file (and conversion output) is deleted.
    pub(crate) fn remove_staged(&self, draft_id: &str, item_id: &str) {
        let owned = self.map().get(item_id).is_some_and(|s| s.draft == draft_id);
        if owned {
            self.map().remove(item_id);
            let _ = fs::remove_dir_all(self.staging().join(draft_id).join(item_id));
        }
    }

    /// Run was pressed: moves the listed staged files (rename, same filesystem) into the
    /// conversation folder and records one row each for `session_id`. Refuses with
    /// [`AttachError::FileMissing`], moving nothing, if any file is gone.
    pub(crate) fn commit(
        &self,
        db: &Db,
        draft_id: &str,
        conversation_id: &str,
        session_id: &str,
        item_ids: &[String],
    ) -> Result<Vec<AttachmentRow>, AttachError> {
        if !valid_id(draft_id) || !valid_id(conversation_id) {
            return Err(copy_failed("bad id"));
        }
        let items: Vec<StagedAttachment> = {
            let m = self.map();
            item_ids
                .iter()
                .map(|id| {
                    m.get(id)
                        .filter(|s| s.draft == draft_id)
                        .map(|s| s.info.clone())
                        .ok_or(AttachError::FileMissing)
                })
                .collect::<Result<_, _>>()?
        };
        let draft_dir = self.staging().join(draft_id);
        let rel_of = |i: &StagedAttachment| i.converted_path.clone().unwrap_or_default();
        for i in &items {
            let present = draft_dir.join(&i.stored_path).is_file()
                && (i.converted_path.is_none() || draft_dir.join(rel_of(i)).is_file());
            if !present {
                return Err(AttachError::FileMissing);
            }
        }
        let dest = self.dir_for(conversation_id);
        fs::create_dir_all(&dest).map_err(copy_failed)?;
        for i in &items {
            fs::rename(draft_dir.join(&i.id), dest.join(&i.id)).map_err(copy_failed)?;
        }
        let rows: Vec<AttachmentRow> = items
            .iter()
            .zip(0u32..)
            .map(|(i, ordinal)| AttachmentRow {
                session_id: session_id.to_owned(),
                ordinal,
                name: i.name.clone(),
                stored_path: i.stored_path.clone(),
                kind: i.kind,
                size_bytes: i.size_bytes,
                converted_path: i.converted_path.clone(),
                removed: false,
                created_at: String::new(),
            })
            .collect();
        db.insert_attachments(&rows).map_err(copy_failed)?;
        self.map().retain(|id, _| !item_ids.contains(id));
        let _ = fs::remove_dir(&draft_dir); // only if empty
        Ok(rows)
    }

    /// Startup sweep: removes draft folders untouched for `max_age` (24 h) and forgets their
    /// chips. Wire into app startup next to `remove_stale_clone_temps`.
    pub(crate) fn sweep_staging(&self, max_age: Duration) {
        let Ok(entries) = fs::read_dir(self.staging()) else {
            return;
        };
        for e in entries.flatten() {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() > max_age);
            if old && e.file_type().is_ok_and(|t| t.is_dir()) {
                let _ = fs::remove_dir_all(e.path());
            }
        }
        let staging = self.staging();
        self.map()
            .retain(|_, s| staging.join(&s.draft).join(&s.info.id).exists());
    }

    /// Deletes the attachment folders of conversations that have no run among the project's
    /// newest `keep_runs` runs, and flags their rows `removed` ("File removed" chips). Call it
    /// right after snapshot pruning with `KEEP_RUNS`.
    pub(crate) fn prune(&self, db: &Db, project_id: &str, keep_runs: usize) -> Result<(), DbError> {
        let sessions = db.list_sessions_by_project(project_id)?; // oldest first
        let parent: HashMap<&str, &str> = sessions
            .iter()
            .filter_map(|s| Some((s.id.as_str(), s.resumed_from.as_deref()?)))
            .collect();
        let root_of = |id: &str| {
            let mut cur = id;
            for _ in 0..1000 {
                match parent.get(cur) {
                    Some(p) => cur = p,
                    None => break,
                }
            }
            cur.to_owned()
        };
        let recent: HashSet<String> = sessions
            .iter()
            .rev()
            .take(keep_runs)
            .map(|s| root_of(&s.id))
            .collect();
        let mut doomed_roots = HashSet::new();
        let mut doomed_runs = Vec::new();
        for s in &sessions {
            let root = root_of(&s.id);
            if !recent.contains(&root) {
                doomed_runs.push(s.id.clone());
                doomed_roots.insert(root);
            }
        }
        for root in doomed_roots.iter().filter(|r| valid_id(r)) {
            let _ = fs::remove_dir_all(self.dir_for(root));
        }
        db.mark_attachments_removed(&doomed_runs)
    }

    /// Project removal: deletes every conversation folder of the project. Call BEFORE the
    /// project row is deleted (the rows then go with it).
    pub(crate) fn remove_project(&self, db: &Db, project_id: &str) -> Result<(), DbError> {
        self.prune(db, project_id, 0)
    }
}

/// The conversation a run belongs to: the first run's session id, following `resumed_from`.
/// Call after the run's session row exists.
pub(crate) fn conversation_id(db: &Db, session_id: &str) -> Result<String, DbError> {
    let mut cur = session_id.to_owned();
    for _ in 0..1000 {
        match db.get_session(&cur)?.and_then(|s| s.resumed_from) {
            Some(parent) => cur = parent,
            None => break,
        }
    }
    Ok(cur)
}

/// Draft and conversation ids become folder names: plain id characters only.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A file name that is safe on disk and in a prompt: ASCII letters, digits, `.`, `-`, `_`.
fn safe_name(display: &str) -> String {
    let mut s: String = display
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    s = s.trim_start_matches('.').to_owned();
    if s.len() > 100
        && let Some((stem, ext)) = s.rsplit_once('.')
    {
        s = format!("{}.{ext}", &stem[..stem.len().min(80)]);
    }
    if s.is_empty() { "file".into() } else { s }
}

/// Copies/writes the file into `dir`, checks its content and converts DOCX. Returns the name of
/// the converted Markdown, if any.
#[allow(clippy::too_many_arguments)]
async fn fill(
    dir: &Path,
    safe: &str,
    ext: &str,
    kind: AttachmentKind,
    kind_mb: u32,
    src: &Source<'_>,
    limits: &AttachmentLimits,
    pandoc: &Pandoc,
) -> Result<Option<String>, AttachError> {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(dir).map_err(copy_failed)?;
    let part = dir.join(format!("{safe}.part"));
    match src {
        // Copies the bytes of what the path points at: the result is a plain file of its own.
        Source::Path(p) => tokio::fs::copy(p, &part).await.map(|_| ()),
        Source::Bytes(b) => tokio::fs::write(&part, b).await,
    }
    .map_err(copy_failed)?;
    fs::set_permissions(&part, fs::Permissions::from_mode(0o644)).map_err(copy_failed)?;
    // The file may have grown since it was measured.
    if fs::metadata(&part).map_err(copy_failed)?.len() > u64::from(kind_mb) * MB {
        return Err(AttachError::TooLarge { mb: kind_mb });
    }
    if !content_matches(&part, kind, ext) {
        return Err(AttachError::Unsupported);
    }
    let dest = dir.join(safe);
    fs::rename(&part, &dest).map_err(copy_failed)?;
    if kind == AttachmentKind::Docx {
        return convert_docx(dir, safe, limits, pandoc).await.map(Some);
    }
    Ok(None)
}

/// Content check so a renamed file is not taken for what its extension claims.
fn content_matches(path: &Path, kind: AttachmentKind, ext: &str) -> bool {
    if kind == AttachmentKind::Text {
        return fs::read(path).is_ok_and(|b| !b.contains(&0) && std::str::from_utf8(&b).is_ok());
    }
    let mut head = [0u8; 12];
    let Ok(n) = fs::File::open(path).and_then(|mut f| f.read(&mut head)) else {
        return false;
    };
    let head = &head[..n];
    match ext {
        "png" => head.starts_with(&[0x89, b'P', b'N', b'G']),
        "jpg" | "jpeg" => head.starts_with(&[0xFF, 0xD8, 0xFF]),
        "gif" => head.starts_with(b"GIF8"),
        "webp" => head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WEBP"),
        "pdf" => head.starts_with(b"%PDF-"),
        "docx" => head.starts_with(b"PK\x03\x04"),
        _ => false,
    }
}

/// `pandoc -t gfm --extract-media=.` inside the item folder, so the Markdown and its `media/`
/// folder travel together when the item is moved.
async fn convert_docx(
    dir: &Path,
    safe: &str,
    limits: &AttachmentLimits,
    pandoc: &Pandoc,
) -> Result<String, AttachError> {
    let bin = pandoc.path.as_ref().ok_or(AttachError::PandocMissing)?;
    let stem = safe.rsplit_once('.').map_or(safe, |(s, _)| s);
    let out = format!("{stem}.md");
    let args = [
        safe,
        "-f",
        "docx",
        "-t",
        "gfm",
        "--extract-media=.",
        "-o",
        out.as_str(),
    ];
    let timeout = Duration::from_secs(u64::from(limits.pandoc_timeout_secs));
    match run_capped_in(bin, &args, &pandoc.env, Some(dir), timeout).await {
        Err(RunError::Spawn(e)) if e.kind() == ErrorKind::NotFound => {
            Err(AttachError::PandocMissing)
        }
        Err(RunError::Spawn(e)) => Err(AttachError::ConversionFailed {
            detail: e.to_string(),
        }),
        Err(RunError::TimedOut(_)) => Err(AttachError::ConversionTimedOut),
        Ok(o) if !o.status.success() => Err(AttachError::ConversionFailed {
            detail: o
                .stderr
                .lines()
                .next()
                .map_or_else(|| o.status.to_string(), str::to_owned),
        }),
        Ok(_) => {
            let len = fs::metadata(dir.join(&out)).map_err(|_| AttachError::ConversionFailed {
                detail: "pandoc produced no output".into(),
            })?;
            if len.len() > u64::from(limits.max_docx_converted_mb) * MB {
                return Err(AttachError::TooLarge {
                    mb: limits.max_docx_converted_mb,
                });
            }
            Ok(out)
        }
    }
}
