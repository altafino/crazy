//! What the preview needs to know about a project's files: its package manager (M3-D8,
//! M3-D23) and whether `tauri dev` may be told a different port (M3-D7, M3-D32).

use std::path::Path;

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackageManager {
    Pnpm,
    Npm,
    Yarn,
    Bun,
}

impl PackageManager {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Pnpm => "pnpm",
            Self::Npm => "npm",
            Self::Yarn => "yarn",
            Self::Bun => "bun",
        }
    }

    /// Arguments of the install; pnpm keeps `--prefer-offline`.
    pub(crate) fn install_args(self) -> &'static [&'static str] {
        match self {
            Self::Pnpm => &["install", "--prefer-offline"],
            _ => &["install"],
        }
    }

    /// Arguments that run the project's Tauri CLI with `dev`.
    pub(crate) fn tauri_dev_args(self) -> &'static [&'static str] {
        match self {
            Self::Pnpm => &["exec", "tauri", "dev"],
            Self::Npm => &["exec", "--", "tauri", "dev"],
            Self::Yarn => &["tauri", "dev"],
            Self::Bun => &["x", "tauri", "dev"],
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pnpm" => Self::Pnpm,
            "npm" => Self::Npm,
            "yarn" => Self::Yarn,
            "bun" => Self::Bun,
            _ => return None,
        })
    }
}

/// Lockfile first, then package.json's `packageManager`, then pnpm (template projects).
pub(crate) fn detect_package_manager(dir: &Path) -> PackageManager {
    let has = |f: &str| dir.join(f).exists();
    if has("pnpm-lock.yaml") {
        return PackageManager::Pnpm;
    }
    if has("package-lock.json") {
        return PackageManager::Npm;
    }
    if has("yarn.lock") {
        return PackageManager::Yarn;
    }
    if has("bun.lock") || has("bun.lockb") {
        return PackageManager::Bun;
    }
    read_json(&dir.join("package.json"))
        .and_then(|v| {
            let field = v.get("packageManager")?.as_str()?;
            PackageManager::parse(field.split('@').next()?)
        })
        .unwrap_or(PackageManager::Pnpm)
}

fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

/// RFC 7396 merge patch, which is how Tauri layers `tauri.linux.conf.json` on the base config.
fn merge(base: &mut Value, patch: Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                if v.is_null() {
                    b.remove(&k);
                } else {
                    merge(b.entry(k).or_insert(Value::Null), v);
                }
            }
        }
        (b, p) => *b = p,
    }
}

/// The effective `build.beforeDevCommand` script: `tauri.conf.json` plus
/// `tauri.linux.conf.json`, string or `{script}` form.
pub(crate) fn before_dev_command(dir: &Path) -> Option<String> {
    let tauri = dir.join("src-tauri");
    let mut conf = read_json(&tauri.join("tauri.conf.json"))?;
    if let Some(linux) = read_json(&tauri.join("tauri.linux.conf.json")) {
        merge(&mut conf, linux);
    }
    match conf.pointer("/build/beforeDevCommand")? {
        Value::String(s) => Some(s.clone()),
        Value::Object(o) => o.get("script")?.as_str().map(str::to_owned),
        _ => None,
    }
}

/// The `--config` value that moves the preview to `port`, or `None` when the project's dev
/// command is anything but the template's `pnpm dev` (then `tauri dev` runs unmodified).
pub(crate) fn port_override(dir: &Path, port: u16) -> Option<String> {
    (before_dev_command(dir)?.trim() == "pnpm dev").then(|| {
        json!({"build": {
            "devUrl": format!("http://localhost:{port}"),
            "beforeDevCommand": format!("pnpm dev --port {port} --strictPort"),
        }})
        .to_string()
    })
}

/// A free localhost port, found by binding port 0. Racy by nature; the caller retries once.
pub(crate) fn free_port() -> std::io::Result<u16> {
    Ok(std::net::TcpListener::bind(("127.0.0.1", 0))?
        .local_addr()?
        .port())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn dir_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for (name, body) in files {
            let p = d.path().join(name);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, body).unwrap();
        }
        d
    }

    #[test]
    fn manager_comes_from_the_lockfile_then_package_manager_then_pnpm() {
        let m = |files: &[(&str, &str)]| detect_package_manager(dir_with(files).path());
        assert_eq!(m(&[("pnpm-lock.yaml", "")]), PackageManager::Pnpm);
        assert_eq!(m(&[("package-lock.json", "{}")]), PackageManager::Npm);
        assert_eq!(m(&[("yarn.lock", "")]), PackageManager::Yarn);
        assert_eq!(m(&[("bun.lock", "")]), PackageManager::Bun);
        assert_eq!(m(&[("bun.lockb", "")]), PackageManager::Bun);
        // The lockfile beats a conflicting packageManager field.
        assert_eq!(
            m(&[
                ("yarn.lock", ""),
                ("package.json", r#"{"packageManager":"pnpm@9"}"#)
            ]),
            PackageManager::Yarn
        );
        assert_eq!(
            m(&[(
                "package.json",
                r#"{"packageManager":"yarn@4.1.0+sha1.abc"}"#
            )]),
            PackageManager::Yarn
        );
        assert_eq!(
            m(&[("package.json", r#"{"packageManager":"weird@1"}"#)]),
            PackageManager::Pnpm
        );
        assert_eq!(m(&[("package.json", "not json")]), PackageManager::Pnpm);
        assert_eq!(m(&[]), PackageManager::Pnpm);
    }

    #[test]
    fn install_commands_per_manager() {
        assert_eq!(
            PackageManager::Pnpm.install_args(),
            ["install", "--prefer-offline"]
        );
        assert_eq!(PackageManager::Npm.install_args(), ["install"]);
        assert_eq!(PackageManager::Yarn.install_args(), ["install"]);
        assert_eq!(PackageManager::Bun.install_args(), ["install"]);
    }

    fn conf(before: &str) -> String {
        format!(r#"{{"build":{{"beforeDevCommand":{before},"devUrl":"http://localhost:1420"}}}}"#)
    }

    #[test]
    fn port_override_only_for_the_templates_pnpm_dev() {
        let port = |files: &[(&str, &str)]| port_override(dir_with(files).path(), 4321);
        let want: Value = json!({"build": {
            "devUrl": "http://localhost:4321",
            "beforeDevCommand": "pnpm dev --port 4321 --strictPort"}});
        let got = |s: Option<String>| serde_json::from_str::<Value>(&s.unwrap()).unwrap();

        // String and {script} forms.
        let c = conf(r#""pnpm dev""#);
        assert_eq!(got(port(&[("src-tauri/tauri.conf.json", &c)])), want);
        let c = conf(r#"{"script":"pnpm dev","cwd":".."}"#);
        assert_eq!(got(port(&[("src-tauri/tauri.conf.json", &c)])), want);
        // Not the template's command: unmodified.
        for other in [
            r#""npm run dev""#,
            r#""pnpm dev --host""#,
            r#""vite""#,
            r#"{"script":"yarn dev"}"#,
        ] {
            let c = conf(other);
            assert_eq!(port(&[("src-tauri/tauri.conf.json", &c)]), None, "{other}");
        }
        // No command, no config, broken config.
        assert_eq!(
            port(&[("src-tauri/tauri.conf.json", r#"{"build":{}}"#)]),
            None
        );
        assert_eq!(port(&[]), None);
        assert_eq!(port(&[("src-tauri/tauri.conf.json", "{")]), None);
    }

    #[test]
    fn linux_config_is_merged_before_matching() {
        let base = conf(r#""pnpm dev""#);
        // The linux file replaces the command: not the template's any more.
        let linux = r#"{"build":{"beforeDevCommand":"npm run dev"}}"#;
        let d = dir_with(&[
            ("src-tauri/tauri.conf.json", &base),
            ("src-tauri/tauri.linux.conf.json", linux),
        ]);
        assert_eq!(before_dev_command(d.path()).as_deref(), Some("npm run dev"));
        assert_eq!(port_override(d.path(), 1), None);
        // ...and the other way round: the base is custom, linux restores `pnpm dev`.
        let base = conf(r#""npm run dev""#);
        let linux = r#"{"build":{"beforeDevCommand":{"script":"pnpm dev"}}}"#;
        let d = dir_with(&[
            ("src-tauri/tauri.conf.json", &base),
            ("src-tauri/tauri.linux.conf.json", linux),
        ]);
        assert!(port_override(d.path(), 1).is_some());
        // A linux file without the key leaves the base alone.
        let d = dir_with(&[
            ("src-tauri/tauri.conf.json", &conf(r#""pnpm dev""#)),
            ("src-tauri/tauri.linux.conf.json", r#"{"app":{}}"#),
        ]);
        assert!(port_override(d.path(), 1).is_some());
    }

    #[test]
    fn free_port_is_usable() {
        let p = free_port().unwrap();
        assert!(std::net::TcpListener::bind(("127.0.0.1", p)).is_ok());
    }
}
