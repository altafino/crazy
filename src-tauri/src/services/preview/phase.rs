//! The live phase of a preview, derived from known `tauri dev` / cargo / Vite lines
//! (M3-D14, M3-D29). Pure: no IO, so recorded logs can drive it in tests. Unknown output
//! keeps the last phase. Revisit the patterns after each Tauri or cargo update.

use regex::Regex;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "phase",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum PreviewPhase {
    /// Checking tools, before anything runs.
    Starting,
    Installing,
    Compiling,
    /// Another cargo process holds the build directory or package cache.
    WaitingForRustBuild,
    /// The app process is up (cargo's "Running `target/...`").
    AppRunning,
    /// "Build failed, waiting for changes", with the first error line.
    BuildFailed {
        first_error: Option<String>,
    },
}

static ANSI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").expect("valid regex"));
static COMPILING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(Compiling|Building|Downloading|Downloaded|Updating)\b").expect("valid regex")
});
static RUNNING_APP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*Running `[^`]*target/").expect("valid regex"));
static ERROR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^error(\[E\d+\])?: ").expect("valid regex"));
static LOCK_WAIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*Blocking waiting for file lock on").expect("valid regex"));
static PORT_IN_USE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Port (\d+) is already in use").expect("valid regex"));

/// Removes ANSI escapes and carriage-return progress redraws, leaving the final text.
pub(crate) fn clean_line(raw: &str) -> String {
    let stripped = ANSI.replace_all(raw, "");
    // Cargo redraws progress with \r; the last segment is what the terminal would show.
    stripped
        .rsplit('\r')
        .find(|s| !s.trim().is_empty())
        .unwrap_or("")
        .trim_end()
        .to_owned()
}

pub(crate) struct PhaseParser {
    phase: PreviewPhase,
    /// The first `error...` line of the current build, kept while the phase is `BuildFailed`.
    first_error: Option<String>,
    port_in_use: Option<u16>,
}

impl PhaseParser {
    pub(crate) fn new(start: PreviewPhase) -> Self {
        Self {
            phase: start,
            first_error: None,
            port_in_use: None,
        }
    }

    /// Set by the service for phases the output does not announce (Installing, Compiling).
    pub(crate) fn set(&mut self, phase: PreviewPhase) {
        self.phase = phase;
    }

    /// The port Vite said was taken, if any line said so.
    pub(crate) fn port_in_use(&self) -> Option<u16> {
        self.port_in_use
    }

    /// The first error line of the last failed build.
    pub(crate) fn first_error(&self) -> Option<&str> {
        self.first_error.as_deref()
    }

    /// Feeds one cleaned line; returns the new phase when it changed.
    pub(crate) fn feed(&mut self, line: &str) -> Option<&PreviewPhase> {
        if let Some(c) = PORT_IN_USE.captures(line) {
            self.port_in_use = c[1].parse().ok();
        }
        let before = self.phase.clone();
        if ERROR.is_match(line) {
            let generic = line.starts_with("error: could not compile");
            // The specific error that precedes cargo's summary line wins.
            if self.first_error.is_none() || (self.first_error_is_generic() && !generic) {
                self.first_error = Some(line.to_owned());
            }
            self.phase = PreviewPhase::BuildFailed {
                first_error: self.first_error.clone(),
            };
        } else if RUNNING_APP.is_match(line) {
            self.first_error = None;
            self.phase = PreviewPhase::AppRunning;
        } else if LOCK_WAIT.is_match(line) {
            self.phase = PreviewPhase::WaitingForRustBuild;
        } else if COMPILING.is_match(line) {
            // A new build starts: forget the previous failure.
            self.first_error = None;
            self.phase = PreviewPhase::Compiling;
        }
        (self.phase != before).then_some(&self.phase)
    }

    fn first_error_is_generic(&self) -> bool {
        self.first_error
            .as_deref()
            .is_some_and(|e| e.starts_with("error: could not compile"))
    }
}

#[cfg(test)]
impl PhaseParser {
    fn phase_for_test(&self) -> &PreviewPhase {
        &self.phase
    }
}

#[cfg(test)]
#[path = "phase_tests.rs"]
mod tests;
