// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Enabled after the M1 spike: Wayland Error 71 on NVIDIA without it (PRD Section 11 step 6).
/// The call itself only acts on NVIDIA + Wayland and never overrides a user-set value.
const APPLY_NVIDIA_WORKAROUND: bool = true;

fn main() {
    if APPLY_NVIDIA_WORKAROUND {
        // SAFETY: first statement of main(), no threads exist yet.
        unsafe { ai_app_studio_lib::apply_nvidia_workaround() };
    }
    ai_app_studio_lib::run()
}
