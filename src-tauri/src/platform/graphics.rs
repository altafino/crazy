//! NVIDIA + Wayland detection and the optional WebKitGTK workaround (PRD 11 step 6, D10).
use serde::Serialize;
use specta::Type;
use std::path::Path;

const NVIDIA_VERSION_FILE: &str = "/proc/driver/nvidia/version";
const WORKAROUND_VAR: &str = "__NV_DISABLE_EXPLICIT_SYNC";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GraphicsInfo {
    pub nvidia: bool,
    pub wayland: bool,
    pub driver_version: Option<String>,
}

impl GraphicsInfo {
    pub fn detect() -> Self {
        Self::detect_with(|k| std::env::var(k).ok(), Path::new(NVIDIA_VERSION_FILE))
    }

    pub fn detect_with(get: impl Fn(&str) -> Option<String>, nvidia_version_file: &Path) -> Self {
        let version = std::fs::read_to_string(nvidia_version_file).ok();
        let wayland = get("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
            || get("XDG_SESSION_TYPE").as_deref() == Some("wayland");
        Self {
            nvidia: version.is_some(),
            wayland,
            driver_version: version.and_then(|v| v.lines().next().map(|l| l.trim().to_string())),
        }
    }

    fn needs_workaround(&self) -> bool {
        self.nvidia && self.wayland
    }
}

/// Sets `__NV_DISABLE_EXPLICIT_SYNC=1` on NVIDIA + Wayland unless the user already set it.
/// Returns whether it was set.
///
/// # Safety
/// Mutates the process environment: call from `main()` before any thread is spawned.
pub unsafe fn apply_nvidia_workaround() -> bool {
    let info = GraphicsInfo::detect();
    if !info.needs_workaround() || std::env::var_os(WORKAROUND_VAR).is_some() {
        return false;
    }
    // SAFETY: upheld by the caller (single-threaded at this point).
    unsafe { std::env::set_var(WORKAROUND_VAR, "1") };
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(env: &[(&str, &str)], file: &Path) -> GraphicsInfo {
        GraphicsInfo::detect_with(
            |k| {
                env.iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| (*v).to_string())
            },
            file,
        )
    }

    #[test]
    fn nvidia_wayland_reads_first_line() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("version");
        std::fs::write(
            &f,
            "NVRM version: NVIDIA UNIX x86_64 Kernel Module  580.82\nGCC: x\n",
        )
        .unwrap();
        let i = detect(&[("XDG_SESSION_TYPE", "wayland")], &f);
        assert!(i.nvidia && i.wayland && i.needs_workaround());
        assert_eq!(
            i.driver_version.as_deref(),
            Some("NVRM version: NVIDIA UNIX x86_64 Kernel Module  580.82")
        );
    }

    #[test]
    fn x11_without_nvidia() {
        let i = detect(
            &[("XDG_SESSION_TYPE", "x11")],
            Path::new("/nonexistent/aas"),
        );
        assert!(!i.nvidia && !i.wayland && !i.needs_workaround());
        assert_eq!(i.driver_version, None);
    }

    #[test]
    fn wayland_display_counts() {
        assert!(
            detect(
                &[("WAYLAND_DISPLAY", "wayland-0")],
                Path::new("/nonexistent/aas")
            )
            .wayland
        );
    }
}
