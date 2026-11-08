//! Preview runner tests: a fake `pnpm` (and `npm`) script stands in for the package manager
//! and `tauri dev`, replaying recorded `tauri dev` output; the doctor is a canned report.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicU32, Ordering};

use tempfile::TempDir;

use super::*;
use crate::domain::TemplateId;
use crate::persistence::{NewProject, SessionMode, SessionRow};
use crate::platform::graphics::GraphicsInfo;
use crate::services::agents::claude::ClaudeAdapter;
use crate::services::diagnostics::{GraphicsRow, ShellEnv, ShellStatus, ToolSource, ToolStatus};
use crate::services::git::checkpoint::CheckpointStore;
use crate::services::git::{GitRunner, SnapshotStore};
use crate::services::jobs::JobSupervisor;
use crate::services::settings::PreviewLimits;

const SUCCESS_LOG: &str = include_str!("fixtures/tauri_dev_success.log");
const ERROR_LOG: &str = include_str!("fixtures/tauri_dev_build_error.log");

/// Fake package manager. Records every call as one line in $FAKE_LOG; behaviour comes from
/// env vars carried by the toolchain's `spawn_env`.
///   install: $FAKE_INSTALL = fail -> exit 1
///   dev (`exec tauri dev`, `tauri dev`, `x tauri dev`): $FAKE_DEV =
///     ok       print $FAKE_FIXTURE, exit 0 (the app window was closed)
///     hang     print $FAKE_FIXTURE, then stay up (the app is running)
///     fail     print $FAKE_FIXTURE, exit 1
///     portonce first run: Vite's "port in use" error + exit 1; later runs: ok
///     portall  every run: the port error + exit 1
///     flood    print 6000 lines (first is the app-running line), exit 0
const SCRIPT: &str = r##"#!/bin/bash
echo "$0 $*" >> "$FAKE_LOG"
case "$*" in
  install*) [ "$FAKE_INSTALL" = fail ] && { echo "ERR_PNPM_FETCH boom" >&2; exit 1; }; echo "Done in 1s"; exit 0;;
esac
[ -n "$FAKE_PIDFILE" ] && echo $$ > "$FAKE_PIDFILE"
n=$(( $(cat "$FAKE_LOG.count" 2>/dev/null || echo 0) + 1 )); echo $n > "$FAKE_LOG.count"
port=$(echo "$*" | grep -o -- '--port [0-9]*' | head -1 | cut -d' ' -f2)
mode="$FAKE_DEV"
[ "$mode" = portonce ] && { [ $n -eq 1 ] && mode=porterr || mode=ok; }
[ "$mode" = portall ] && mode=porterr
case "$mode" in
  porterr) echo "Error: Port ${port:-1420} is already in use" >&2; exit 1;;
  flood)
    echo '     Running `target/debug/app`'
    for i in $(seq 1 6000); do echo "noise line $i"; done; exit 0;;
  fail) cat "$FAKE_FIXTURE"; exit 1;;
  hang) cat "$FAKE_FIXTURE"; sleep 30 & echo $! > "${FAKE_PIDFILE}.child"; wait;;
  *) cat "$FAKE_FIXTURE"; exit 0;;
esac
"##;

struct FakeToolchain {
    report: Mutex<DoctorReport>,
    forced: AtomicU32,
    env: Vec<(String, String)>,
    bin: PathBuf,
}

fn tool(id: &str, name: &str, state: ToolState) -> ToolStatus {
    ToolStatus {
        id: id.into(),
        name: name.into(),
        state,
    }
}

fn installed() -> ToolState {
    ToolState::Installed {
        version: "1".into(),
        path: "/x".into(),
        source: ToolSource::ShellPath,
    }
}

fn report(checked_at: u32, missing: &[(&str, &str)]) -> DoctorReport {
    let tools = [
        ("node", "Node.js", "sudo pacman -S nodejs"),
        ("pnpm", "pnpm", "sudo pacman -S pnpm"),
        ("cargo", "Cargo", "Install Rust with rustup"),
        ("rustc", "Rust compiler", "Install Rust with rustup"),
        (
            "webkit2gtk",
            "WebKitGTK 4.1",
            "sudo pacman -S webkit2gtk-4.1",
        ),
    ]
    .iter()
    .map(|(id, name, hint)| {
        let state = if missing.iter().any(|(m, _)| m == id) {
            ToolState::Missing {
                hint: (*hint).into(),
            }
        } else {
            installed()
        };
        tool(id, name, state)
    })
    .collect();
    DoctorReport {
        shell: ShellEnv {
            path: None,
            aliases: vec![],
            status: ShellStatus::Ok,
        },
        tools,
        graphics: GraphicsRow {
            info: GraphicsInfo {
                nvidia: false,
                wayland: false,
                driver_version: None,
            },
            note: None,
        },
        warnings: vec![],
        checked_at,
    }
}

impl Toolchain for FakeToolchain {
    fn doctor_report(
        &self,
        force: bool,
    ) -> Pin<Box<dyn Future<Output = DoctorReport> + Send + '_>> {
        if force {
            self.forced.fetch_add(1, Ordering::SeqCst);
            // A refresh yields a current report.
            self.report.lock().unwrap().checked_at = now_secs();
        }
        Box::pin(async move { self.report.lock().unwrap().clone() })
    }

    fn spawn_env(&self) -> Vec<(String, String)> {
        self.env.clone()
    }

    fn resolve_binary(&self, tool: &str) -> Option<PathBuf> {
        let p = self.bin.join(tool);
        p.exists().then_some(p)
    }
}

struct Fx {
    tmp: TempDir,
    db: Arc<Db>,
    svc: PreviewService,
    sup: JobSupervisor<ClaudeAdapter>,
    tc: Arc<FakeToolchain>,
    project: ProjectRow,
}

fn write_script(dir: &Path, name: &str) {
    let p = dir.join(name);
    fs::write(&p, SCRIPT).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
}

fn tauri_conf(before: &str) -> String {
    format!(r#"{{"build":{{"beforeDevCommand":{before},"devUrl":"http://localhost:1420"}}}}"#)
}

/// `env` is merged over the base environment of the fake toolchain.
fn fx_with(
    template: TemplateId,
    files: &[(&str, &str)],
    managers: &[&str],
    env: &[(&str, &str)],
) -> Fx {
    let tmp = TempDir::new().unwrap();
    let bin = tmp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    for m in managers {
        write_script(&bin, m);
    }
    let dir = tmp.path().join("project");
    fs::create_dir(&dir).unwrap();
    for (name, body) in files {
        let p = dir.join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }
    let fixture = tmp.path().join("dev.log");
    fs::write(&fixture, SUCCESS_LOG).unwrap();
    let mut full_env = vec![
        ("PATH".into(), format!("{}:/usr/bin:/bin", bin.display())),
        (
            "FAKE_LOG".into(),
            tmp.path().join("calls.log").display().to_string(),
        ),
        ("FAKE_FIXTURE".into(), fixture.display().to_string()),
        (
            "FAKE_PIDFILE".into(),
            tmp.path().join("pid").display().to_string(),
        ),
    ];
    full_env.extend(env.iter().map(|(k, v)| ((*k).into(), (*v).into())));

    let db = Arc::new(Db::open_in_memory().unwrap());
    db.insert_project(&NewProject {
        id: "proj1".into(),
        name: "a".into(),
        slug: "a".into(),
        template_id: template,
        app_id: "com.x.a".into(),
        workspace_path: dir.display().to_string(),
        pending_op: None,
    })
    .unwrap();
    let project = db.get_project("proj1").unwrap().unwrap();

    let data = tmp.path().join("data");
    let git_env = vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())];
    let sup = JobSupervisor::new(
        db.clone(),
        Arc::new(SnapshotStore::new(
            &data,
            GitRunner {
                program: "git".into(),
                env: git_env.clone(),
            },
        )),
        Arc::new(CheckpointStore {
            program: "git".into(),
            env: git_env,
        }),
        ClaudeAdapter::default(),
        data.clone(),
    );
    let tc = Arc::new(FakeToolchain {
        report: Mutex::new(report(now_secs(), &[])),
        forced: AtomicU32::new(0),
        env: full_env,
        bin,
    });
    let svc = PreviewService::new(db.clone(), sup.slots(), tc.clone(), data);
    Fx {
        tmp,
        db,
        svc,
        sup,
        tc,
        project,
    }
}

/// A template-like project: pnpm lockfile, `pnpm dev` as the dev command.
fn fx(env: &[(&str, &str)]) -> Fx {
    let conf = tauri_conf(r#""pnpm dev""#);
    fx_with(
        TemplateId::TauriVue,
        &[("pnpm-lock.yaml", ""), ("src-tauri/tauri.conf.json", &conf)],
        &["pnpm"],
        env,
    )
}

impl Fx {
    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.tmp.path().join("calls.log"))
            .unwrap_or_default()
            .lines()
            .map(|l| l.split_once(' ').map_or(l, |(_, rest)| rest).to_owned())
            .collect()
    }

    fn dev_calls(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|c| c.contains("tauri"))
            .collect()
    }

    fn status(&self) -> PreviewStatus {
        self.svc.status("proj1").unwrap().unwrap()
    }

    async fn wait(&self, what: &str, pred: impl Fn(&PreviewStatus) -> bool) -> PreviewStatus {
        for _ in 0..500 {
            if let Ok(Some(s)) = self.svc.status("proj1")
                && pred(&s)
            {
                return s;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("never reached: {what}; last {:?}", self.svc.status("proj1"));
    }

    async fn finished(&self) -> PreviewStatus {
        self.wait("preview ended", |s| !s.live).await
    }

    async fn running_app(&self) -> PreviewStatus {
        self.wait("app running", |s| s.phase == Some(PreviewPhase::AppRunning))
            .await
    }

    fn log(&self) -> Vec<String> {
        self.svc.log_tail("proj1", None).unwrap().unwrap().lines
    }

    fn log_has(&self, needle: &str) -> bool {
        self.log().iter().any(|l| l.contains(needle))
    }
}

fn phases(events: &mut broadcast::Receiver<PreviewEvent>) -> Vec<PreviewPhase> {
    let mut out: Vec<PreviewPhase> = Vec::new();
    while let Ok(e) = events.try_recv() {
        if let PreviewEvent::Status(s) = e
            && let Some(p) = s.phase
            && out.last() != Some(&p)
        {
            out.push(p);
        }
    }
    out
}

// ------------------------------------------------------------ lifecycle

/// `tauri dev` exits 0 even when the app aborts, so a fatal line in the output decides.
#[tokio::test]
async fn app_crash_with_exit_zero_is_failed_with_the_crash_line() {
    let crash = TempDir::new().unwrap();
    let log = crash.path().join("crash.log");
    let line = "** (test1:3799809): ERROR **: 02:43:49.985: Unable to spawn a new child process";
    fs::write(&log, format!("{SUCCESS_LOG}\n{line}\n")).unwrap();
    let path = log.display().to_string();
    let fx = fx(&[("FAKE_DEV", "ok"), ("FAKE_FIXTURE", &path)]);
    fx.svc.start(&fx.project).unwrap();
    let done = fx.finished().await;
    assert_eq!(done.state, JobState::Failed);
    assert_eq!(done.error_message.as_deref(), Some(line));
}

#[tokio::test]
async fn window_closed_with_exit_zero_succeeds_and_phases_run_in_order() {
    let fx = fx(&[("FAKE_DEV", "ok")]);
    let mut events = fx.svc.subscribe();
    let started = fx.svc.start(&fx.project).unwrap();
    assert_eq!(started.state, JobState::Running);
    assert!(started.live);
    let done = fx.finished().await;
    assert_eq!(done.state, JobState::Succeeded);
    assert_eq!(done.error_message, None);
    assert!(done.phase.is_none() && done.completed_at.is_some() && done.has_log);
    assert_eq!(
        phases(&mut events),
        [
            PreviewPhase::Starting,
            PreviewPhase::Installing,
            PreviewPhase::Compiling,
            PreviewPhase::AppRunning
        ]
    );
    // Install ran first, with pnpm's --prefer-offline, then tauri dev.
    let calls = fx.calls();
    assert_eq!(calls[0], "install --prefer-offline");
    assert!(
        calls[1].starts_with("exec tauri dev --config "),
        "{calls:?}"
    );
    let job = fx.db.get_job(&started.job_id).unwrap().unwrap();
    assert_eq!(
        (job.kind.as_str(), job.state, job.exit_code),
        ("preview", JobState::Succeeded, Some(0))
    );
    // The cleaned output (no ANSI) is in memory and in the file.
    assert!(fx.log_has("Running `target/debug/ai-app-studio`"));
    assert!(fx.log().iter().all(|l| !l.contains('\u{1b}')));
    let file = fs::read_to_string(output::log_path(
        &fx.tmp.path().join("data"),
        LOG_DIR,
        &started.job_id,
    ))
    .unwrap();
    assert!(file.contains("Finished `dev` profile"));
}

#[tokio::test]
async fn stop_cancels_and_kills_the_whole_group() {
    let fx = fx(&[("FAKE_DEV", "hang")]);
    let job = fx.svc.start(&fx.project).unwrap().job_id;
    fx.running_app().await;
    let child: i32 = {
        let p = fx.tmp.path().join("pid.child");
        for _ in 0..100 {
            if p.exists() && !fs::read_to_string(&p).unwrap_or_default().trim().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        fs::read_to_string(p).unwrap().trim().parse().unwrap()
    };
    assert!(unsafe { libc::kill(child, 0) } == 0);
    fx.svc.stop("proj1").await;
    let s = fx.status();
    assert_eq!((s.state, s.live), (JobState::Cancelled, false));
    assert_eq!(
        fx.db.get_job(&job).unwrap().unwrap().error_kind.as_deref(),
        Some("cancelled")
    );
    for _ in 0..100 {
        if unsafe { libc::kill(child, 0) } != 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the app's child process survived Stop");
}

#[tokio::test]
async fn non_zero_exit_fails_with_the_first_error_line() {
    let fx = fx(&[("FAKE_DEV", "fail")]);
    fs::write(fx.tmp.path().join("dev.log"), ERROR_LOG).unwrap();
    let id = fx.svc.start(&fx.project).unwrap().job_id;
    let done = fx.finished().await;
    assert_eq!(done.state, JobState::Failed);
    assert_eq!(
        done.error_message.as_deref(),
        Some("error[E0308]: mismatched types")
    );
    let job = fx.db.get_job(&id).unwrap().unwrap();
    assert_eq!(
        (job.exit_code, job.error_kind.as_deref()),
        (Some(1), Some("exited"))
    );
}

#[tokio::test]
async fn build_error_while_running_shows_build_failed_with_the_first_error() {
    let fx = fx(&[("FAKE_DEV", "hang")]);
    fs::write(fx.tmp.path().join("dev.log"), ERROR_LOG).unwrap();
    fx.svc.start(&fx.project).unwrap();
    let s = fx
        .wait("build failed", |s| {
            matches!(s.phase, Some(PreviewPhase::BuildFailed { .. }))
        })
        .await;
    assert_eq!(
        s.phase,
        Some(PreviewPhase::BuildFailed {
            first_error: Some("error[E0308]: mismatched types".into())
        })
    );
    assert_eq!((s.state, s.live), (JobState::Running, true));
    fx.svc.stop("proj1").await;
}

// ------------------------------------------------------------- install

#[tokio::test]
async fn failed_install_fails_the_job_and_tauri_dev_never_starts() {
    let fx = fx(&[("FAKE_INSTALL", "fail"), ("FAKE_DEV", "ok")]);
    fx.svc.start(&fx.project).unwrap();
    let done = fx.finished().await;
    assert_eq!(done.state, JobState::Failed);
    assert_eq!(
        done.error_message.as_deref(),
        Some("Installing dependencies failed")
    );
    assert_eq!(fx.calls(), ["install --prefer-offline"]);
    assert!(fx.dev_calls().is_empty());
    assert!(
        fx.log_has("ERR_PNPM_FETCH boom"),
        "the install output is in the log"
    );
}

#[tokio::test]
async fn install_uses_the_projects_own_manager() {
    // npm project (package-lock.json); only npm is on PATH.
    let conf = tauri_conf(r#""npm run dev""#);
    let fx = fx_with(
        TemplateId::TauriVue,
        &[
            ("package-lock.json", "{}"),
            ("src-tauri/tauri.conf.json", &conf),
        ],
        &["npm"],
        &[("FAKE_DEV", "ok")],
    );
    fx.svc.start(&fx.project).unwrap();
    assert_eq!(fx.finished().await.state, JobState::Succeeded);
    assert_eq!(fx.calls(), ["install", "exec -- tauri dev"]);
}

#[tokio::test]
async fn missing_manager_names_it() {
    let conf = tauri_conf(r#""yarn dev""#);
    let fx = fx_with(
        TemplateId::TauriVue,
        &[("yarn.lock", ""), ("src-tauri/tauri.conf.json", &conf)],
        &["pnpm"], // pnpm exists, yarn doesn't
        &[],
    );
    fx.svc.start(&fx.project).unwrap();
    let done = fx.finished().await;
    assert_eq!(done.state, JobState::Failed);
    assert_eq!(
        done.error_message.as_deref(),
        Some("This project uses yarn, which isn't installed")
    );
    assert!(fx.calls().is_empty());
}

#[tokio::test]
async fn install_is_skipped_while_a_make_changes_run_is_live() {
    let fx = fx(&[("FAKE_DEV", "ok")]);
    fx.db
        .insert_job("agent1", "proj1", JobState::Running)
        .unwrap();
    fx.db
        .insert_session(&SessionRow {
            id: "s1".into(),
            job_id: "agent1".into(),
            project_id: "proj1".into(),
            profile_id: None,
            mode: SessionMode::Changes,
            prompt: "x".into(),
            provider_session_id: None,
            resumed_from: None,
            log_path: "/l".into(),
            before_ref: None,
            after_ref: None,
            checkpoint_ref: None,
            conversation_start: None,
            resume_fell_back: false,
            created_at: String::new(),
        })
        .unwrap();
    fx.svc.start(&fx.project).unwrap();
    assert_eq!(fx.finished().await.state, JobState::Succeeded);
    assert!(
        fx.calls().iter().all(|c| !c.starts_with("install")),
        "{:?}",
        fx.calls()
    );
    assert_eq!(fx.dev_calls().len(), 1);
    assert!(fx.log_has(INSTALL_SKIPPED));
}

#[tokio::test]
async fn install_runs_during_a_plan_first_run() {
    let fx = fx(&[("FAKE_DEV", "ok")]);
    fx.db
        .insert_job("agent1", "proj1", JobState::Running)
        .unwrap();
    fx.db
        .insert_session(&SessionRow {
            id: "s1".into(),
            job_id: "agent1".into(),
            project_id: "proj1".into(),
            profile_id: None,
            mode: SessionMode::Plan,
            prompt: "x".into(),
            provider_session_id: None,
            resumed_from: None,
            log_path: "/l".into(),
            before_ref: None,
            after_ref: None,
            checkpoint_ref: None,
            conversation_start: None,
            resume_fell_back: false,
            created_at: String::new(),
        })
        .unwrap();
    fx.svc.start(&fx.project).unwrap();
    fx.finished().await;
    assert_eq!(fx.calls()[0], "install --prefer-offline");
}

// ---------------------------------------------------------------- port

fn config_arg(call: &str) -> Option<serde_json::Value> {
    let json = call.split_once("--config ")?.1;
    serde_json::from_str(json).ok()
}

#[tokio::test]
async fn port_override_is_applied_for_the_templates_pnpm_dev() {
    let fx = fx(&[("FAKE_DEV", "ok")]);
    fx.svc.start(&fx.project).unwrap();
    let done = fx.finished().await;
    let port = fx.status().port.expect("the chosen port is reported");
    let cfg = config_arg(&fx.dev_calls()[0]).expect("--config json");
    assert_eq!(cfg["build"]["devUrl"], format!("http://localhost:{port}"));
    assert_eq!(
        cfg["build"]["beforeDevCommand"],
        format!("pnpm dev --port {port} --strictPort")
    );
    assert_eq!(done.port, Some(port));
}

#[tokio::test]
async fn other_dev_commands_run_unmodified() {
    for before in [
        r#""npm run dev""#,
        r#""pnpm dev --host""#,
        r#"{"script":"vite","cwd":"."}"#,
    ] {
        let conf = tauri_conf(before);
        let fx = fx_with(
            TemplateId::TauriVue,
            &[("pnpm-lock.yaml", ""), ("src-tauri/tauri.conf.json", &conf)],
            &["pnpm"],
            &[("FAKE_DEV", "ok")],
        );
        fx.svc.start(&fx.project).unwrap();
        fx.finished().await;
        assert_eq!(fx.dev_calls(), ["exec tauri dev"], "{before}");
        assert_eq!(fx.status().port, None);
    }
}

#[tokio::test]
async fn taken_port_is_retried_once_on_a_new_port() {
    let fx = fx(&[("FAKE_DEV", "portonce")]);
    fx.svc.start(&fx.project).unwrap();
    let done = fx.finished().await;
    assert_eq!(done.state, JobState::Succeeded);
    let dev = fx.dev_calls();
    assert_eq!(dev.len(), 2, "{dev:?}");
    let port = |c: &str| {
        config_arg(c).unwrap()["build"]["devUrl"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let (first, second) = (port(&dev[0]), port(&dev[1]));
    assert!(fx.log_has("was taken; retrying on"));
    let p1 = first.rsplit(':').next().unwrap();
    let p2 = second.rsplit(':').next().unwrap();
    assert!(fx.log_has(&format!("Port {p1} was taken; retrying on {p2}")));
    // Install ran once, not again for the retry.
    assert_eq!(
        fx.calls()
            .iter()
            .filter(|c| c.starts_with("install"))
            .count(),
        1
    );
}

#[tokio::test]
async fn a_second_taken_port_fails_with_a_port_diagnostic() {
    let fx = fx(&[("FAKE_DEV", "portall")]);
    fx.svc.start(&fx.project).unwrap();
    let done = fx.finished().await;
    assert_eq!(done.state, JobState::Failed);
    let msg = done.error_message.unwrap();
    assert!(
        msg.contains("is already in use by another program"),
        "{msg}"
    );
    assert_eq!(fx.dev_calls().len(), 2, "exactly one retry");
}

#[tokio::test]
async fn unmodified_dev_command_gets_no_retry_on_a_taken_port() {
    let conf = tauri_conf(r#""npm run dev""#);
    let fx = fx_with(
        TemplateId::TauriVue,
        &[("pnpm-lock.yaml", ""), ("src-tauri/tauri.conf.json", &conf)],
        &["pnpm"],
        &[("FAKE_DEV", "portall")],
    );
    fx.svc.start(&fx.project).unwrap();
    let done = fx.finished().await;
    assert!(
        done.error_message
            .unwrap()
            .contains("Port 1420 is already in use")
    );
    assert_eq!(fx.dev_calls().len(), 1);
}

// ------------------------------------------------------------ preflight

#[tokio::test]
async fn missing_tool_fails_with_the_doctors_hint_and_spawns_nothing() {
    let fx = fx(&[("FAKE_DEV", "ok")]);
    *fx.tc.report.lock().unwrap() = report(now_secs(), &[("webkit2gtk", "")]);
    fx.svc.start(&fx.project).unwrap();
    let done = fx.finished().await;
    assert_eq!(done.state, JobState::Failed);
    assert_eq!(
        done.error_message.as_deref(),
        Some("Preview needs WebKitGTK 4.1: sudo pacman -S webkit2gtk-4.1")
    );
    assert!(fx.calls().is_empty());
    for (tool, name) in [
        ("node", "Node.js"),
        ("cargo", "Cargo"),
        ("rustc", "Rust compiler"),
        ("pnpm", "pnpm"),
    ] {
        *fx.tc.report.lock().unwrap() = report(now_secs(), &[(tool, "")]);
        fx.svc.start(&fx.project).unwrap();
        let done = fx.finished().await;
        assert!(
            done.error_message
                .unwrap()
                .starts_with(&format!("Preview needs {name}: ")),
            "{tool}"
        );
    }
}

#[tokio::test]
async fn a_misconfigured_override_counts() {
    let fx = fx(&[]);
    fx.tc.report.lock().unwrap().tools[0].state = ToolState::Misconfigured {
        path: Some("/bad/node".into()),
        reason: "the path is not executable".into(),
    };
    fx.svc.start(&fx.project).unwrap();
    assert_eq!(
        fx.finished().await.error_message.as_deref(),
        Some("Preview needs Node.js: the path is not executable")
    );
}

#[tokio::test]
async fn an_old_report_is_refreshed_and_a_fresh_one_is_not() {
    let fx = fx(&[("FAKE_DEV", "ok")]);
    fx.svc.start(&fx.project).unwrap();
    fx.finished().await;
    assert_eq!(
        fx.tc.forced.load(Ordering::SeqCst),
        0,
        "a fresh report is used as is"
    );

    // 11 minutes old: refreshed, and the refreshed report decides.
    *fx.tc.report.lock().unwrap() = report(now_secs() - 11 * 60, &[]);
    fx.svc.start(&fx.project).unwrap();
    fx.finished().await;
    assert_eq!(fx.tc.forced.load(Ordering::SeqCst), 1);
}

// ----------------------------------------------------------- slots

#[tokio::test]
async fn one_preview_per_project_and_only_for_tauri_vue() {
    let fx = fx(&[("FAKE_DEV", "hang")]);
    fx.svc.start(&fx.project).unwrap();
    assert!(matches!(fx.svc.start(&fx.project), Err(PreviewError::Busy)));
    fx.svc.stop("proj1").await;
    fx.svc.start(&fx.project).unwrap(); // the slot is free again
    fx.svc.stop("proj1").await;

    let existing = fx_with(TemplateId::Existing, &[], &["pnpm"], &[]);
    assert!(matches!(
        existing.svc.start(&existing.project),
        Err(PreviewError::Unsupported)
    ));
    assert!(
        existing
            .db
            .list_jobs_of_kind("proj1", "preview")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_preview_does_not_block_an_agent_slot() {
    let fx = fx(&[("FAKE_DEV", "hang")]);
    fx.svc.start(&fx.project).unwrap();
    assert!(fx.sup.slots().claim("proj1", JobKind::Agent, "agent-job"));
    fx.sup.slots().release("proj1", JobKind::Agent);
    fx.svc.stop("proj1").await;
}

#[tokio::test]
async fn restart_replaces_the_preview() {
    let fx = fx(&[("FAKE_DEV", "hang")]);
    let first = fx.svc.start(&fx.project).unwrap().job_id;
    fx.running_app().await;
    let second = fx.svc.restart(&fx.project).await.unwrap().job_id;
    assert_ne!(first, second);
    assert_eq!(
        fx.db.get_job(&first).unwrap().unwrap().state,
        JobState::Cancelled
    );
    assert_eq!(fx.status().job_id, second);
    fx.svc.stop("proj1").await;
}

#[tokio::test]
async fn shutdown_stops_a_running_preview_as_interrupted_and_kills_its_group() {
    let fx = fx(&[("FAKE_DEV", "hang")]);
    let job = fx.svc.start(&fx.project).unwrap().job_id;
    fx.running_app().await;
    tokio::time::timeout(Duration::from_secs(10), fx.sup.shutdown())
        .await
        .unwrap();
    assert_eq!(
        fx.db.get_job(&job).unwrap().unwrap().state,
        JobState::Interrupted
    );
    assert!(!fx.status().live);
}

#[tokio::test]
async fn after_a_restart_the_last_preview_comes_from_the_job_row_and_log_file() {
    let fx = fx(&[("FAKE_DEV", "ok")]);
    let job = fx.svc.start(&fx.project).unwrap().job_id;
    fx.finished().await;
    // A fresh service over the same database and data dir: no in-memory run.
    let again = PreviewService::new(
        fx.db.clone(),
        fx.sup.slots(),
        fx.tc.clone(),
        fx.tmp.path().join("data"),
    );
    let s = again.status("proj1").unwrap().unwrap();
    assert_eq!(
        (s.job_id.as_str(), s.state, s.live, s.has_log),
        (job.as_str(), JobState::Succeeded, false, true)
    );
    assert!(s.phase.is_none());
    let tail = again.log_tail("proj1", None).unwrap().unwrap();
    assert!(
        tail.lines
            .iter()
            .any(|l| l.contains("Finished `dev` profile"))
    );
    // Another project's job id can't read this log.
    assert!(again.log_tail("other", Some(&job)).unwrap().is_none());
    assert!(
        PreviewService::new(
            Arc::new(Db::open_in_memory().unwrap()),
            fx.sup.slots(),
            fx.tc.clone(),
            fx.tmp.path().join("data")
        )
        .status("proj1")
        .unwrap()
        .is_none()
    );
}

// --------------------------------------------------------------- output

#[tokio::test]
async fn output_is_bounded_batched_and_the_phase_parser_sees_every_line() {
    let fx = fx(&[("FAKE_DEV", "flood")]);
    settings::save_preview_limits(
        &fx.db,
        PreviewLimits {
            max_lines: 1_000,
            max_log_mb: 1,
            keep_logs: 5,
        },
    )
    .unwrap();
    let mut events = fx.svc.subscribe();
    let job = fx.svc.start(&fx.project).unwrap().job_id;
    let done = fx.finished().await;
    assert_eq!(done.state, JobState::Succeeded);

    // Memory: only the last 1,000 lines, though the first line (the app-running line) was
    // pushed out; the parser still saw it.
    let tail = fx.svc.log_tail("proj1", None).unwrap().unwrap();
    assert_eq!(tail.lines.len(), 1_000);
    assert_eq!(tail.lines.last().unwrap(), "noise line 6000");
    assert!(!tail.lines.iter().any(|l| l.contains("target/debug/app")));
    let mut seen_app_running = false;
    let (mut batches, mut batched_lines, mut next) = (0, 0usize, None::<u32>);
    while let Ok(e) = events.try_recv() {
        match e {
            PreviewEvent::Status(s) => {
                seen_app_running |= s.phase == Some(PreviewPhase::AppRunning);
            }
            PreviewEvent::Log {
                from_seq, lines, ..
            } => {
                // Batches are contiguous and in order.
                if let Some(n) = next {
                    assert_eq!(from_seq, n);
                }
                next = Some(from_seq + lines.len() as u32);
                batches += 1;
                batched_lines += lines.len();
            }
        }
    }
    assert!(
        seen_app_running,
        "phase parsing must see lines beyond the memory cap"
    );
    assert!(batched_lines >= 6_000, "{batched_lines}");
    assert!(
        batches < 600,
        "events are batched, not one per line: {batches}"
    );
    assert_eq!(tail.next_seq, next.unwrap());
    // The file has every line (6,000 short lines are far below 1 MB).
    let file =
        fs::read_to_string(output::log_path(&fx.tmp.path().join("data"), LOG_DIR, &job)).unwrap();
    assert!(file.lines().count() >= 6_000);
}

#[tokio::test]
async fn only_the_newest_logs_are_kept_per_project_and_rows_stay() {
    let fx = fx(&[("FAKE_DEV", "ok")]);
    settings::save_preview_limits(
        &fx.db,
        PreviewLimits {
            max_lines: 1_000,
            max_log_mb: 1,
            keep_logs: 2,
        },
    )
    .unwrap();
    let mut ids = Vec::new();
    for _ in 0..4 {
        ids.push(fx.svc.start(&fx.project).unwrap().job_id);
        fx.finished().await;
    }
    let exists =
        |i: usize| output::log_path(&fx.tmp.path().join("data"), LOG_DIR, &ids[i]).exists();
    assert!(!exists(0) && !exists(1), "older logs are deleted");
    assert!(exists(2) && exists(3));
    assert_eq!(
        fx.db.list_jobs_of_kind("proj1", "preview").unwrap().len(),
        4
    );
}
