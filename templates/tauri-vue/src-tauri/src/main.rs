// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Enabled after the M1 spike: Wayland Error 71 on NVIDIA without it (PRD Section 11 step 6).
const APPLY_NVIDIA_WORKAROUND: bool = true;

/// Must run before any thread starts (before Tauri).
#[cfg(target_os = "linux")]
fn apply_graphics_workaround() {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v == "wayland");
    if std::path::Path::new("/proc/driver/nvidia/version").exists()
        && wayland
        && std::env::var_os("__NV_DISABLE_EXPLICIT_SYNC").is_none()
    {
        // SAFETY: called first thing in main, while the process is single-threaded.
        unsafe { std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1") };
    }
}

#[cfg(not(target_os = "linux"))]
fn apply_graphics_workaround() {}

fn main() {
    if APPLY_NVIDIA_WORKAROUND {
        apply_graphics_workaround();
    }
    app_lib::run()
}
