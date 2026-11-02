//! OS integration: paths, graphics quirks, AppImage environment (PRD 6.2).
// Consumers (commands, lib.rs wiring) land after this module; drop once used.
#![allow(dead_code)]

pub mod appimage;
pub mod graphics;
pub mod paths;

/// The process environment as owned UTF-8 pairs (non-UTF-8 entries are skipped).
pub fn process_env() -> Vec<(String, String)> {
    std::env::vars_os()
        .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
        .collect()
}
