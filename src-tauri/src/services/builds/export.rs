//! Exports for other systems (PRD F22; eng review 2026-11-06 D2-D14, design review 1-15).
//!
//! A kept release's saved source (`snapshot_ref`, D7) is built in a local Docker container on
//! the Ubuntu 22.04 baseline (D2): portable Linux files (AppImage, deb, rpm, D3) and a Windows
//! NSIS installer through cargo-xwin. Files land in
//! `installed/<project>/releases/<ver>/exports/<target>/` with `SHA256SUMS` (D6). Exports are
//! never installed, activated or rolled back (M4-D5 unchanged).
//!
//! An export is one job in the project's Build slot (D8), marked with `export_release_id` /
//! `export_targets`, with per-target results in `export_results` (D14).

use serde::{Deserialize, Serialize};
use specta::Type;

mod docker;
mod job;
#[cfg(test)]
mod tests;

pub(super) use docker::{DF_TIMEOUT, PROBE_TIMEOUT};
pub(super) use job::remove_leftovers;

/// A system an export builds for.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type,
)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ExportTarget {
    /// AppImage, .deb and .rpm built on the Ubuntu 22.04 base (glibc 2.35).
    Linux,
    /// NSIS `setup.exe` for Windows x86_64 (cargo-xwin, experimental upstream).
    Windows,
}

impl ExportTarget {
    /// The folder name under `exports/` and the value stored in `jobs.export_targets`.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Windows => "windows",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "linux" => Some(Self::Linux),
            "windows" => Some(Self::Windows),
            _ => None,
        }
    }
}

/// Whether Docker can be used for exports (design 5, 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DockerState {
    Ready,
    /// No `docker` binary.
    Missing,
    /// The binary exists but the daemon can't be reached (or did not answer in time).
    NotRunning,
    /// The daemon runs but this user may not use its socket (not in the docker group).
    NoPermission,
}

/// Everything the export dialog, the Doctor's optional-tools row and the first-run caption
/// need, from one probe (re-review RS2).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportReadiness {
    pub docker: DockerState,
    /// `docker version` server version when ready.
    pub docker_version: Option<String>,
    /// The raw reason when not ready (shown under Details).
    pub detail: Option<String>,
    /// The export image exists, so the next export skips "Environment" (design 4).
    pub image_present: bool,
    /// When the user accepted Microsoft's license for the Windows build tools (D10, design 8).
    pub license_accepted_at: Option<String>,
    /// The targets ticked last time; both on first use (design 3).
    pub last_targets: Vec<ExportTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ExportFileKind {
    AppImage,
    Deb,
    Rpm,
    /// The Windows NSIS installer (`*-setup.exe`).
    Nsis,
}

/// One exported file, from the target folder and its `SHA256SUMS`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportFile {
    pub name: String,
    pub kind: ExportFileKind,
    pub path: String,
    /// Bytes; a float because TypeScript has no 64-bit integer.
    pub size_bytes: f64,
    pub sha256: String,
}

/// The last failure of a target whose current files (if any) are older (design 6, D14).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportFailure {
    pub at: String,
    pub message: String,
}

/// One target of one release: its current files and/or its latest failure.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportTargetView {
    pub target: ExportTarget,
    /// When the current files were made; `null` without files.
    pub built_at: Option<String>,
    /// Folder holding the files, for "Show in Files".
    pub dir: String,
    pub files: Vec<ExportFile>,
    /// Set when the newest export of this target failed.
    pub failed: Option<ExportFailure>,
}

/// The exports of one release. Releases without exports or failures are left out.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReleaseExports {
    pub release_id: String,
    /// `exports/` of the release, for "Show in Files".
    pub dir: String,
    pub targets: Vec<ExportTargetView>,
}

/// The per-target outcome of a finished export job (`jobs.export_results`, D14).
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportResult {
    pub target: ExportTarget,
    pub ok: bool,
    /// The first error line when `ok` is false.
    pub message: Option<String>,
}

/// What a `BuildStatus` carries when its job is an export (design 4, 6, 7).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportJob {
    pub release_id: String,
    pub version: String,
    pub targets: Vec<ExportTarget>,
    /// Filled when the job ends.
    pub results: Vec<ExportResult>,
    /// The image had to be built first ("First export" caption).
    pub first_run: bool,
}

/// Settings > General "Exports" card (design 12).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportCacheSize {
    /// Bytes used by the export caches and image; 0 when there are none.
    pub bytes: f64,
}
