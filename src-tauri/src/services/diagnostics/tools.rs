//! Tracked tools: resolution (override > shell PATH > GUI PATH) and probing.
use super::{
    ToolSource, ToolState, ToolStatus,
    spawn::{RunError, run_capped},
};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

pub(super) enum Probe {
    /// Run `binary args...`; a non-zero exit is a misconfiguration.
    Command {
        binary: &'static str,
        args: &'static [&'static str],
    },
    /// Like `Command`, but a non-zero exit means "not installed" (pkg-config module lookup).
    Query {
        binary: &'static str,
        args: &'static [&'static str],
    },
    /// A shared library must exist in one of the system lib dirs.
    Library { file: &'static str },
}

pub(super) struct ToolSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub hint: &'static str,
    pub probe: Probe,
}

const fn cmd(binary: &'static str) -> Probe {
    Probe::Command {
        binary,
        args: &["--version"],
    }
}

const RUSTUP: &str = "Install Rust with rustup (https://rustup.rs)";

pub(super) const TOOLS: &[ToolSpec] = &[
    ToolSpec {
        id: "git",
        name: "Git",
        hint: "sudo pacman -S git",
        probe: cmd("git"),
    },
    ToolSpec {
        id: "cargo",
        name: "Cargo",
        hint: RUSTUP,
        probe: cmd("cargo"),
    },
    ToolSpec {
        id: "rustc",
        name: "Rust compiler",
        hint: RUSTUP,
        probe: cmd("rustc"),
    },
    ToolSpec {
        id: "node",
        name: "Node.js",
        hint: "sudo pacman -S nodejs",
        probe: cmd("node"),
    },
    ToolSpec {
        id: "pnpm",
        name: "pnpm",
        hint: "sudo pacman -S pnpm",
        probe: cmd("pnpm"),
    },
    ToolSpec {
        id: "cargo-tauri",
        name: "Tauri CLI",
        hint: "cargo install tauri-cli (or sudo pacman -S tauri-cli)",
        probe: cmd("cargo-tauri"),
    },
    ToolSpec {
        id: "claude",
        name: "Claude Code",
        hint: "Install Claude Code (https://claude.com/claude-code)",
        probe: cmd("claude"),
    },
    ToolSpec {
        id: "gh",
        name: "GitHub CLI",
        hint: "sudo pacman -S github-cli",
        probe: cmd("gh"),
    },
    ToolSpec {
        id: "webkit2gtk",
        name: "WebKitGTK 4.1",
        hint: "sudo pacman -S webkit2gtk-4.1",
        probe: Probe::Query {
            binary: "pkg-config",
            args: &["--modversion", "webkit2gtk-4.1"],
        },
    },
    ToolSpec {
        id: "libfuse2",
        name: "libfuse2 (AppImage)",
        hint: "sudo pacman -S fuse2",
        probe: Probe::Library {
            file: "libfuse.so.2",
        },
    },
];

const LIB_DIRS: &[&str] = &["/usr/lib", "/usr/lib64"];

fn is_executable(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

fn find_in_path(name: &str, path: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .filter(|d| !d.as_os_str().is_empty())
        .map(|d| d.join(name))
        .find(|p| is_executable(p))
}

fn status(spec: &ToolSpec, state: ToolState) -> ToolStatus {
    ToolStatus {
        id: spec.id.into(),
        name: spec.name.into(),
        state,
    }
}

fn missing(spec: &ToolSpec) -> ToolStatus {
    status(
        spec,
        ToolState::Missing {
            hint: spec.hint.into(),
        },
    )
}

fn misconfigured(spec: &ToolSpec, path: &Path, reason: String) -> ToolStatus {
    status(
        spec,
        ToolState::Misconfigured {
            path: Some(path.display().to_string()),
            reason,
        },
    )
}

pub(super) async fn check(
    spec: &ToolSpec,
    override_path: Option<&str>,
    shell_path: Option<&str>,
    gui_path: &str,
    env: &[(String, String)],
    timeout: Duration,
) -> ToolStatus {
    let (binary, args, query) = match &spec.probe {
        Probe::Library { file } => {
            let found = LIB_DIRS
                .iter()
                .map(|d| Path::new(d).join(file))
                .find(|p| p.exists());
            return match found {
                Some(p) => status(
                    spec,
                    ToolState::Installed {
                        version: String::new(),
                        path: p.display().to_string(),
                        source: ToolSource::System,
                    },
                ),
                None => missing(spec),
            };
        }
        Probe::Command { binary, args } => (*binary, *args, false),
        Probe::Query { binary, args } => (*binary, *args, true),
    };

    let (path, source) = if let Some(o) = override_path {
        let p = PathBuf::from(o);
        if !is_executable(&p) {
            return misconfigured(
                spec,
                &p,
                "override path is missing or not executable".into(),
            );
        }
        (p, ToolSource::Override)
    } else if let Some(p) = shell_path.and_then(|sp| find_in_path(binary, sp)) {
        (p, ToolSource::ShellPath)
    } else if let Some(p) = find_in_path(binary, gui_path) {
        (p, ToolSource::GuiPath)
    } else {
        return missing(spec);
    };

    match run_capped(&path, args, env, timeout).await {
        Err(RunError::TimedOut(_)) => misconfigured(
            spec,
            &path,
            format!("probe timed out after {}s", timeout.as_secs_f32()),
        ),
        Err(e) => misconfigured(spec, &path, e.to_string()),
        Ok(out) if !out.status.success() && query => missing(spec),
        Ok(out) if !out.status.success() => {
            let detail = first_line(&out.stderr);
            misconfigured(spec, &path, format!("exited with {}: {detail}", out.status))
        }
        Ok(out) => {
            let version = Some(first_line(&out.stdout))
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| first_line(&out.stderr));
            status(
                spec,
                ToolState::Installed {
                    version,
                    path: path.display().to_string(),
                    source,
                },
            )
        }
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}
