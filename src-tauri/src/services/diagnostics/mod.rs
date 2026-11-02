//! DiagnosticsService: environment doctor (tool resolution, probes, graphics).

mod shell;
mod spawn;
mod tools;

use crate::platform::{appimage, graphics::GraphicsInfo, process_env};
use serde::Serialize;
use specta::Type;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Mutex as StdMutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{sync::Mutex, task::JoinSet};

const SHELL_TIMEOUT: Duration = Duration::from_secs(10);
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const NVIDIA_WAYLAND_NOTE: &str =
    "NVIDIA on Wayland: WebKitGTK may render a blank window; see PRD Section 11 step 6";

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "state", rename_all = "camelCase")]
pub(crate) enum ShellStatus {
    Ok,
    TimedOut,
    Failed { reason: String },
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AliasInfo {
    pub tool: String,
    pub definition: String,
    pub env_assignments: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShellEnv {
    /// PATH reported by the login shell; `None` unless `status` is `ok`.
    pub path: Option<String>,
    pub aliases: Vec<AliasInfo>,
    pub status: ShellStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ToolSource {
    Override,
    ShellPath,
    GuiPath,
    /// Not found through PATH (file checks such as libfuse2).
    System,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "state", rename_all = "camelCase")]
pub(crate) enum ToolState {
    Installed {
        version: String,
        path: String,
        source: ToolSource,
    },
    Missing {
        hint: String,
    },
    Misconfigured {
        path: Option<String>,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolStatus {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub state: ToolState,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GraphicsRow {
    pub info: GraphicsInfo,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DoctorReport {
    pub shell: ShellEnv,
    pub tools: Vec<ToolStatus>,
    pub graphics: GraphicsRow,
    pub warnings: Vec<String>,
    /// Seconds since the Unix epoch.
    pub checked_at: u32,
}

/// Everything environment-dependent, so tests can inject a fake shell and short timeouts.
struct Config {
    shell: PathBuf,
    env: Vec<(String, String)>,
    shell_timeout: Duration,
    probe_timeout: Duration,
}

pub(crate) struct DiagnosticsService {
    cfg: Config,
    cache: Mutex<Option<DoctorReport>>,
    resolved_path: StdMutex<Option<String>>,
}

impl DiagnosticsService {
    pub(crate) fn new() -> Self {
        let env = process_env();
        let shell = env
            .iter()
            .find(|(k, _)| k == "SHELL")
            .map_or_else(|| "/bin/sh".into(), |(_, v)| v.into());
        Self::with_config(Config {
            shell,
            env,
            shell_timeout: SHELL_TIMEOUT,
            probe_timeout: PROBE_TIMEOUT,
        })
    }

    fn with_config(cfg: Config) -> Self {
        Self {
            cfg,
            cache: Mutex::new(None),
            resolved_path: StdMutex::new(None),
        }
    }

    /// Returns the cached report unless `force` (or nothing is cached yet).
    /// `overrides` maps tool id to a user-chosen binary path.
    pub(crate) async fn run(
        &self,
        overrides: &HashMap<String, String>,
        force: bool,
    ) -> DoctorReport {
        let mut cache = self.cache.lock().await;
        if let (false, Some(report)) = (force, cache.as_ref()) {
            return report.clone();
        }
        let report = self.compute(overrides).await;
        *cache = Some(report.clone());
        report
    }

    /// PATH to hand to spawned tools: the login-shell PATH after a `run`, the GUI PATH before.
    pub(crate) fn resolved_path(&self) -> String {
        let resolved = self
            .resolved_path
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        resolved.unwrap_or_else(|| self.gui_path())
    }

    /// Path of a host binary: the user override when given, else a lookup on the resolved PATH.
    pub(crate) fn resolve_binary(
        &self,
        binary: &str,
        override_path: Option<&str>,
    ) -> Option<PathBuf> {
        match override_path {
            Some(p) => Some(PathBuf::from(p)),
            None => tools::find_in_path(binary, &self.resolved_path()),
        }
    }

    /// Environment for host tools (AppImage variables stripped, resolved PATH set).
    pub(crate) fn spawn_env(&self) -> Vec<(String, String)> {
        spawn_env(&self.cfg.env, &self.resolved_path())
    }

    fn gui_path(&self) -> String {
        self.cfg
            .env
            .iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }

    async fn compute(&self, overrides: &HashMap<String, String>) -> DoctorReport {
        let host = appimage::host_env(&self.cfg.env);
        let shell = shell::capture(&self.cfg.shell, &host, self.cfg.shell_timeout).await;
        let gui_path = self.gui_path();
        let path = shell.path.clone().unwrap_or_else(|| gui_path.clone());
        *self.resolved_path.lock().unwrap_or_else(|e| e.into_inner()) = Some(path.clone());
        let env = spawn_env(&self.cfg.env, &path);

        let mut probes = JoinSet::new();
        for (i, spec) in tools::TOOLS.iter().enumerate() {
            let (o, sp, gp, env, t) = (
                overrides.get(spec.id).cloned(),
                shell.path.clone(),
                gui_path.clone(),
                env.clone(),
                self.cfg.probe_timeout,
            );
            probes.spawn(async move {
                (
                    i,
                    tools::check(spec, o.as_deref(), sp.as_deref(), &gp, &env, t).await,
                )
            });
        }
        let mut statuses = Vec::new();
        while let Some(done) = probes.join_next().await {
            statuses.extend(done); // a panicked probe simply drops its row
        }
        statuses.sort_by_key(|(i, _)| *i);

        let info = GraphicsInfo::detect();
        let note = (info.nvidia && info.wayland).then(|| NVIDIA_WAYLAND_NOTE.to_string());
        let warnings = warnings(&shell, note.as_deref());
        DoctorReport {
            shell,
            tools: statuses.into_iter().map(|(_, s)| s).collect(),
            graphics: GraphicsRow { info, note },
            warnings,
            checked_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX)),
        }
    }
}

fn spawn_env(base: &[(String, String)], path: &str) -> Vec<(String, String)> {
    let mut env = appimage::host_env(base);
    env.retain(|(k, _)| k != "PATH");
    env.push(("PATH".into(), path.into()));
    env
}

fn warnings(shell: &ShellEnv, graphics_note: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    match &shell.status {
        ShellStatus::Ok => {}
        ShellStatus::TimedOut => out.push(
            "Reading your login shell timed out; using the desktop session PATH, so tools added by your shell profile may show as missing".into(),
        ),
        ShellStatus::Failed { reason } => out.push(format!(
            "Could not read your login shell ({reason}); using the desktop session PATH"
        )),
    }
    for a in shell
        .aliases
        .iter()
        .filter(|a| !a.env_assignments.is_empty())
    {
        let vars: Vec<&str> = a
            .env_assignments
            .iter()
            .filter_map(|w| w.split_once('=').map(|(n, _)| n))
            .collect();
        let tail = if a.tool == "claude" {
            " until agent profiles (Milestone 2)"
        } else {
            ""
        };
        out.push(format!(
            "Your shell alias for {} sets {}; Studio runs {} without it{tail}",
            a.tool,
            vars.join(", "),
            a.tool
        ));
    }
    out.extend(graphics_note.map(str::to_string));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use spawn::{OUTPUT_CAP, RunError, run_capped};
    use std::{fs, os::unix::fs::PermissionsExt, path::Path, time::Instant};

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn service(
        shell: PathBuf,
        gui_path: &str,
        probe: Duration,
        shell_t: Duration,
    ) -> DiagnosticsService {
        DiagnosticsService::with_config(Config {
            shell,
            env: vec![("PATH".into(), gui_path.into())],
            shell_timeout: shell_t,
            probe_timeout: probe,
        })
    }

    fn state<'a>(r: &'a DoctorReport, id: &str) -> &'a ToolState {
        &r.tools.iter().find(|t| t.id == id).unwrap().state
    }

    const MARKERS: &str = "printf 'noise\\n__AAS_PATH_BEGIN__%s__AAS_PATH_END__\\n' \"$FAKEPATH\"";

    #[tokio::test]
    async fn shell_capture_parses_noise_aliases_and_warns() {
        let d = tempfile::tempdir().unwrap();
        let sh = script(
            d.path(),
            "zsh",
            &format!(
                "FAKEPATH=/fake/bin\n{MARKERS}\necho \"claude='CLAUDE_CONFIG_DIR=~/.claude-a claude'\"\necho \"alias git='git --no-pager'\"\necho __AAS_ALIAS_END__\necho trailing p10k noise"
            ),
        );
        let svc = service(sh, "/usr/bin", PROBE_TIMEOUT, SHELL_TIMEOUT);
        let r = svc.run(&HashMap::new(), true).await;
        assert!(matches!(r.shell.status, ShellStatus::Ok));
        assert_eq!(r.shell.path.as_deref(), Some("/fake/bin"));
        assert_eq!(r.shell.aliases.len(), 2);
        assert_eq!(
            r.shell.aliases[0].env_assignments,
            ["CLAUDE_CONFIG_DIR=~/.claude-a"]
        );
        assert!(r.warnings.iter().any(|w| w
            == "Your shell alias for claude sets CLAUDE_CONFIG_DIR; Studio runs claude without it until agent profiles (Milestone 2)"));
        assert_eq!(svc.resolved_path(), "/fake/bin");
    }

    #[tokio::test]
    async fn hanging_shell_is_killed_and_falls_back() {
        let d = tempfile::tempdir().unwrap();
        let pidfile = d.path().join("pid");
        let sh = script(
            d.path(),
            "sh",
            &format!("echo $$ > {}\n/bin/sleep 30", pidfile.display()),
        );
        let svc = service(sh, "/gui/bin", PROBE_TIMEOUT, Duration::from_millis(500));
        let t = Instant::now();
        let r = svc.run(&HashMap::new(), true).await;
        assert!(t.elapsed() < Duration::from_secs(10));
        assert!(matches!(r.shell.status, ShellStatus::TimedOut));
        assert_eq!(svc.resolved_path(), "/gui/bin");
        let pid: i32 = fs::read_to_string(pidfile).unwrap().trim().parse().unwrap();
        // SAFETY: signal 0 only checks existence.
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    }

    #[tokio::test]
    async fn failing_shell_reports_failed_and_falls_back() {
        let d = tempfile::tempdir().unwrap();
        let sh = script(d.path(), "sh", "echo boom >&2\nexit 3");
        let svc = service(sh, "/gui/bin", PROBE_TIMEOUT, SHELL_TIMEOUT);
        let r = svc.run(&HashMap::new(), true).await;
        assert!(matches!(r.shell.status, ShellStatus::Failed { .. }));
        assert_eq!(svc.resolved_path(), "/gui/bin");
        assert!(!r.warnings.is_empty());
    }

    #[tokio::test]
    async fn override_beats_shell_path_and_missing_override_is_misconfigured() {
        let d = tempfile::tempdir().unwrap();
        let (shell_bin, over_bin) = (d.path().join("shellbin"), d.path().join("over"));
        fs::create_dir_all(&shell_bin).unwrap();
        fs::create_dir_all(&over_bin).unwrap();
        script(&shell_bin, "git", "echo git version SHELL");
        script(&shell_bin, "node", "echo v1.2.3");
        let over = script(&over_bin, "git", "echo git version OVERRIDE");
        let sh = script(
            d.path(),
            "sh",
            &format!(
                "FAKEPATH={}\n{MARKERS}\necho __AAS_ALIAS_END__",
                shell_bin.display()
            ),
        );
        let svc = service(sh, "/nonexistent", PROBE_TIMEOUT, SHELL_TIMEOUT);

        let mut o = HashMap::from([("git".to_string(), over.display().to_string())]);
        o.insert("pnpm".into(), "/nonexistent/pnpm".into());
        let r = svc.run(&o, true).await;
        match state(&r, "git") {
            ToolState::Installed {
                version, source, ..
            } => {
                assert_eq!(version, "git version OVERRIDE");
                assert_eq!(*source, ToolSource::Override);
            }
            s => panic!("{s:?}"),
        }
        match state(&r, "node") {
            ToolState::Installed { source, .. } => assert_eq!(*source, ToolSource::ShellPath),
            s => panic!("{s:?}"),
        }
        assert!(matches!(state(&r, "pnpm"), ToolState::Misconfigured { .. }));
        assert!(matches!(state(&r, "gh"), ToolState::Missing { .. }));
    }

    #[tokio::test]
    async fn hanging_tool_is_misconfigured_by_timeout() {
        let d = tempfile::tempdir().unwrap();
        script(d.path(), "git", "/bin/sleep 30");
        let sh = script(d.path(), "sh", "exit 0");
        let svc = service(
            sh,
            &d.path().display().to_string(),
            Duration::from_millis(300),
            SHELL_TIMEOUT,
        );
        let t = Instant::now();
        let r = svc.run(&HashMap::new(), true).await;
        assert!(t.elapsed() < Duration::from_secs(10));
        match state(&r, "git") {
            ToolState::Misconfigured { reason, .. } => assert!(reason.contains("timed out")),
            s => panic!("{s:?}"),
        }
    }

    #[tokio::test]
    async fn report_is_cached_until_forced() {
        let d = tempfile::tempdir().unwrap();
        let sh = script(d.path(), "sh", "exit 0");
        let svc = service(sh, "/x", PROBE_TIMEOUT, SHELL_TIMEOUT);
        let a = svc.run(&HashMap::new(), false).await;
        script(d.path(), "git", "echo v");
        let b = svc.run(&HashMap::new(), false).await;
        assert_eq!(a.checked_at, b.checked_at);
        assert_eq!(a.tools.len(), tools::TOOLS.len());
    }

    #[tokio::test]
    async fn output_is_capped() {
        let out = run_capped(
            "/bin/sh",
            &["-c", "head -c 300000 /dev/zero"],
            &[],
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(out.stdout.len(), OUTPUT_CAP);
        assert!(matches!(
            run_capped(
                "/bin/sh",
                &["-c", "sleep 30"],
                &[],
                Duration::from_millis(100)
            )
            .await,
            Err(RunError::TimedOut(_))
        ));
    }
}
