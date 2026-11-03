//! The Linux launcher: freedesktop entry text, symlink switch, icon placement and `gio launch`.
//! Everything here is synchronous; the installer calls it from `spawn_blocking`.
use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::platform::paths::AppLayout;

use super::{InstallError, io_err};

/// What Studio knows about the launcher entry path (A13: only our own file is ever touched).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryState {
    Missing,
    /// Exists and carries `X-AI-App-Studio-Project=<this project>`.
    Ours,
    Foreign,
}

/// The facts a launcher entry is made of.
#[derive(Debug, Clone)]
pub(crate) struct EntrySpec<'a> {
    pub project_id: &'a str,
    pub product_name: &'a str,
    pub main_binary_name: &'a str,
    pub icon: Option<&'a Path>,
}

/// Desktop integration for one app. Owns format, escaping, the symlink switch and icons.
pub(crate) trait LauncherAdapter: Send + Sync {
    fn entry_state(&self, layout: &AppLayout, project_id: &str)
    -> Result<EntryState, InstallError>;
    /// Writes (or replaces) the entry. Callers have checked it is not foreign.
    fn write_entry(&self, layout: &AppLayout, spec: &EntrySpec<'_>) -> Result<(), InstallError>;
    /// Removes our entry; refuses a foreign file; a missing one is fine.
    fn remove_entry(&self, layout: &AppLayout, project_id: &str) -> Result<(), InstallError>;
    /// Copies the release's icon to the app's icon path. `false` when the release has none.
    fn place_icon(&self, layout: &AppLayout, version: &str) -> Result<bool, InstallError>;
    /// Atomically points `current.AppImage` at the release.
    fn switch(&self, layout: &AppLayout, version: &str) -> Result<(), InstallError>;
    fn launch(&self, layout: &AppLayout, env: &[(String, String)]) -> Result<(), InstallError>;
}

pub(crate) struct XdgLauncher {
    gio: PathBuf,
}

impl XdgLauncher {
    /// `gio` is the resolved program (tests inject `/bin/false`).
    pub(crate) fn new(gio: PathBuf) -> Self {
        Self { gio }
    }
}

/// Atomic file write: temp sibling, then rename.
fn write_atomic(path: &Path, bytes: &[u8], step: &'static str) -> Result<(), InstallError> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(io_err(step, dir))?;
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
    ));
    fs::write(&tmp, bytes).map_err(io_err(step, &tmp))?;
    fs::rename(&tmp, path).map_err(io_err(step, path))
}

impl LauncherAdapter for XdgLauncher {
    fn entry_state(
        &self,
        layout: &AppLayout,
        project_id: &str,
    ) -> Result<EntryState, InstallError> {
        let path = layout.desktop_file();
        match fs::read(path) {
            Ok(bytes) => {
                let marker = format!("X-AI-App-Studio-Project={project_id}");
                Ok(
                    if String::from_utf8_lossy(&bytes).lines().any(|l| l == marker) {
                        EntryState::Ours
                    } else {
                        EntryState::Foreign
                    },
                )
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(EntryState::Missing),
            Err(e) => Err(io_err("read the launcher entry", path)(e)),
        }
    }

    fn write_entry(&self, layout: &AppLayout, spec: &EntrySpec<'_>) -> Result<(), InstallError> {
        let text = desktop_entry(&layout.current_link(), spec);
        write_atomic(
            layout.desktop_file(),
            text.as_bytes(),
            "write the launcher entry",
        )
    }

    fn remove_entry(&self, layout: &AppLayout, project_id: &str) -> Result<(), InstallError> {
        let path = layout.desktop_file();
        match self.entry_state(layout, project_id)? {
            EntryState::Missing => Ok(()),
            EntryState::Foreign => Err(InstallError::ForeignFile(path.to_owned())),
            EntryState::Ours => match fs::remove_file(path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => {
                    Err(io_err("remove the launcher entry", path)(e))
                }
                _ => Ok(()),
            },
        }
    }

    fn place_icon(&self, layout: &AppLayout, version: &str) -> Result<bool, InstallError> {
        let src = layout.release_icon(version);
        let bytes = match fs::read(&src) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(io_err("read the release icon", &src)(e)),
        };
        write_atomic(&layout.icon(), &bytes, "write the app icon")?;
        Ok(true)
    }

    fn switch(&self, layout: &AppLayout, version: &str) -> Result<(), InstallError> {
        let link = layout.current_link();
        let tmp = layout.root().join(".current.AppImage.tmp");
        fs::create_dir_all(layout.root())
            .map_err(io_err("create the app folder", layout.root()))?;
        // A temp link left by a crash is stale: replace it. `current.AppImage` itself is never
        // removed before the rename, so it is always a valid link.
        if tmp.symlink_metadata().is_ok() {
            fs::remove_file(&tmp).map_err(io_err("remove a stale temporary link", &tmp))?;
        }
        symlink(AppLayout::current_target(version), &tmp)
            .map_err(io_err("create the temporary link", &tmp))?;
        fs::rename(&tmp, &link).map_err(io_err("switch the active release", &link))
    }

    fn launch(&self, layout: &AppLayout, env: &[(String, String)]) -> Result<(), InstallError> {
        // stderr goes to a file, not a pipe: the launched app may inherit it and keep a pipe
        // open forever.
        let err_path = layout.root().join("launch.stderr");
        let err_file =
            fs::File::create(&err_path).map_err(io_err("create the launch log", &err_path))?;
        let status = Command::new(&self.gio)
            .arg("launch")
            .arg(layout.desktop_file())
            .env_clear()
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(err_file)
            .process_group(0) // own group: not tied to Studio's lifetime
            .status()
            .map_err(|e| {
                InstallError::Launch(format!("Could not run {}: {e}", self.gio.display()))
            })?;
        if status.success() {
            return Ok(());
        }
        let stderr = fs::read_to_string(&err_path).unwrap_or_default();
        let stderr = stderr.trim();
        Err(InstallError::Launch(if stderr.is_empty() {
            format!("{} launch failed ({status})", self.gio.display())
        } else {
            stderr.to_owned()
        }))
    }
}

/// Desktop Entry string-value escaping: `\` doubles, `\n \t \r` become escapes, other
/// control characters are dropped.
fn escape_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// `Exec=` value for one program path: quoted per the spec (inside quotes `"`, `` ` ``, `$`
/// and `\` get a backslash, `%` doubles), then string-escaped.
fn exec_value(program: &str) -> String {
    let mut quoted = String::from("\"");
    for c in program.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    escape_value(&format!("env APPIMAGELAUNCHER_DISABLE=1 {quoted}"))
}

/// The whole `.desktop` text (A14, M4-D4, M4-D12, S2). Pure.
pub(crate) fn desktop_entry(current_appimage: &Path, spec: &EntrySpec<'_>) -> String {
    // ponytail: non-UTF-8 paths are converted lossily; Studio builds these paths from a
    // UTF-8 HOME in practice. Upgrade to byte-exact escaping if that ever matters.
    let app = current_appimage.to_string_lossy();
    let name: String = spec
        .product_name
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let name = if name.trim().is_empty() {
        spec.main_binary_name
    } else {
        &name
    };
    let mut out = String::from("[Desktop Entry]\nType=Application\n");
    out.push_str(&format!("Name={}\n", escape_value(name)));
    out.push_str(&format!("Exec={}\n", exec_value(&app)));
    out.push_str(&format!("TryExec={}\n", escape_value(&app)));
    if let Some(icon) = spec.icon {
        out.push_str(&format!("Icon={}\n", escape_value(&icon.to_string_lossy())));
    }
    out.push_str("Terminal=false\nCategories=Utility;\n");
    out.push_str(&format!(
        "StartupWMClass={}\n",
        escape_value(spec.main_binary_name)
    ));
    out.push_str(&format!("X-AI-App-Studio-Project={}\n", spec.project_id));
    out
}
