//! The real build-and-install test (PRD M4 step 8, M4-D16). It runs in every `cargo test`:
//! the bundled tauri-vue template is copied the way `ProjectService` does, built through
//! `BuildService` with the real `TauriAppImageAdapter` (pnpm, vite, cargo, AppImage tooling),
//! smoke-tested, installed with the real `XdgLauncher`, validated with
//! `desktop-file-validate`, re-activated and uninstalled.
//!
//! Safety: every Studio path lives under one `TempDir`, so the real
//! `~/.local/share/{applications,icons,ai-app-studio}` are never touched. `gio launch` is
//! never called. No process-global env is mutated. Only the generated app's cargo target dir
//! is shared (`target/real-build-cache`, ignored by version control) so reruns skip the
//! dependency compile. The first run takes minutes.
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tempfile::TempDir;

use super::*;
use crate::domain::NewProjectRequest;
use crate::persistence::{Db, Smoke};
use crate::services::agents::claude::ClaudeAdapter;
use crate::services::diagnostics::{DiagnosticsService, DoctorReport};
use crate::services::git::GitRunner;
use crate::services::install::{InstallState, Installer, XdgLauncher};
use crate::services::jobs::JobSupervisor;
use crate::services::projects::{GitCmd, ProjectService};

/// Ceiling for the build; a cold build (full dependency compile) is the slow case.
const BUILD_TIMEOUT: Duration = Duration::from_secs(45 * 60);

/// The real doctor's PATH and env, plus a persistent cargo target dir for the generated app.
struct RealToolchain {
    doctor: Arc<DiagnosticsService>,
    target_dir: PathBuf,
}

impl Toolchain for RealToolchain {
    fn doctor_report(
        &self,
        force: bool,
    ) -> Pin<Box<dyn Future<Output = DoctorReport> + Send + '_>> {
        Box::pin(async move { self.doctor.run(&HashMap::new(), force).await })
    }

    fn spawn_env(&self) -> Vec<(String, String)> {
        let mut env = self.doctor.spawn_env();
        env.retain(|(k, _)| k != "CARGO_TARGET_DIR");
        env.push((
            "CARGO_TARGET_DIR".into(),
            self.target_dir.display().to_string(),
        ));
        env
    }

    fn resolve_binary(&self, tool: &str) -> Option<PathBuf> {
        self.doctor.resolve_binary(tool, None)
    }
}

fn log_tail(svc: &BuildService, project_id: &str) -> String {
    let lines = svc
        .log_tail(project_id, None)
        .ok()
        .flatten()
        .map(|t| t.lines)
        .unwrap_or_default();
    let from = lines.len().saturating_sub(60);
    lines[from..].join("\n")
}

fn has_display() -> bool {
    ["WAYLAND_DISPLAY", "DISPLAY"]
        .iter()
        .any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_build_smoke_install_validate_activate_uninstall() {
    let t0 = Instant::now();
    let tmp = TempDir::new().unwrap();
    let paths = StudioPaths {
        data_dir: tmp.path().join("data"),
        config_dir: tmp.path().join("config"),
        cache_dir: tmp.path().join("cache"),
        default_projects_dir: tmp.path().join("projects"),
        applications_dir: tmp.path().join("applications"),
    };
    let target_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/real-build-cache");
    fs::create_dir_all(&target_dir).unwrap();

    let db = Arc::new(Db::open_in_memory().unwrap());
    let doctor = Arc::new(DiagnosticsService::new());

    // 1. Template copy, the way the app creates a project.
    let parent = tmp.path().join("projects");
    fs::create_dir_all(&parent).unwrap();
    let projects = ProjectService::new(
        db.clone(),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../templates/tauri-vue"),
        paths.data_dir.clone(),
        GitCmd {
            program: "git".into(),
            env: crate::platform::process_env(),
        },
    );
    let summary = projects
        .create(&NewProjectRequest {
            name: "Real Build".into(),
            slug: Some("real-build".into()),
            parent_dir: parent.canonicalize().unwrap().display().to_string(),
            identifier: None,
            init_git: true,
        })
        .unwrap();
    let project = db.get_project(&summary.id).unwrap().unwrap();
    let conf = Path::new(&project.workspace_path).join("src-tauri/tauri.conf.json");
    assert!(conf.exists());

    // Wiring as in AppState::new, with temp paths.
    let tool_env = doctor.spawn_env();
    let program = doctor
        .resolve_binary("git", None)
        .unwrap_or_else(|| "git".into());
    let checkpoints = Arc::new(CheckpointStore {
        program: program.clone(),
        env: tool_env.clone(),
    });
    let snapshots = Arc::new(SnapshotStore::new(
        &paths.data_dir,
        GitRunner {
            program,
            env: tool_env,
        },
    ));
    let sup = JobSupervisor::new(
        db.clone(),
        snapshots.clone(),
        checkpoints.clone(),
        ClaudeAdapter::default(),
        paths.data_dir.clone(),
    );
    let svc = BuildService::new(BuildDeps {
        db: db.clone(),
        slots: sup.slots(),
        toolchain: Arc::new(RealToolchain {
            doctor: doctor.clone(),
            target_dir,
        }),
        adapter: Arc::new(TauriAppImageAdapter),
        snapshots,
        checkpoints,
        paths: paths.clone(),
        smoke_after: SMOKE_AFTER,
    });
    let gio = doctor
        .resolve_binary("gio", None)
        .unwrap_or_else(|| "gio".into());
    let installer = Installer::new(paths.clone(), db.clone(), Arc::new(XdgLauncher::new(gio)));

    // 2. Real build (the smoke test, step 3, runs as part of it).
    svc.start(&project, "1.0.0".into(), Some("real build test".into()))
        .unwrap();
    let deadline = Instant::now() + BUILD_TIMEOUT;
    let status = loop {
        if let Ok(Some(s)) = svc.status(&project.id)
            && !s.live
        {
            break s;
        }
        assert!(
            Instant::now() < deadline,
            "build still running after {BUILD_TIMEOUT:?}\n--- log tail ---\n{}",
            log_tail(&svc, &project.id)
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    assert_eq!(
        status.state,
        JobState::Succeeded,
        "build failed: {:?} {:?}\n--- log tail ---\n{}",
        status.error_kind,
        status.error_message,
        log_tail(&svc, &project.id)
    );
    let build_time = t0.elapsed();

    // 3. Smoke result: a display decides between a real launch and Unverified.
    let release_id = status
        .release_id
        .clone()
        .expect("a succeeded build has a release");
    let release = db.get_release(&release_id).unwrap().unwrap();
    assert_eq!(release.version, "1.0.0");
    assert!(release.size_bytes > 0);
    let layout = AppLayout::new(&paths, &project.id);
    let app = layout.release_appimage("1.0.0");
    assert!(app.is_file(), "stored AppImage missing: {}", app.display());
    if has_display() {
        assert!(
            matches!(release.smoke, Smoke::Passed | Smoke::Unverified),
            "smoke with a display must not fail: {:?} {:?}\n--- log tail ---\n{}",
            release.smoke,
            release.smoke_detail,
            log_tail(&svc, &project.id)
        );
    } else {
        assert_eq!(
            release.smoke,
            Smoke::Unverified,
            "{:?}",
            release.smoke_detail
        );
    }

    // 4. Install into the temp data home. Safety: the entry must land inside the temp dir.
    let desktop = layout.desktop_file().to_owned();
    assert!(
        desktop.starts_with(tmp.path()) && layout.root().starts_with(tmp.path()),
        "refusing to install outside the temp dir: {}",
        desktop.display()
    );
    let act = installer.activate(&project.id, &release_id).await.unwrap();
    assert_eq!(
        act.state,
        InstallState::Installed {
            release_id: release_id.clone(),
            version: "1.0.0".into()
        }
    );
    assert!(desktop.is_file());
    assert_eq!(layout.active_version().as_deref(), Some("1.0.0"));

    // 5. The entry passes desktop-file-validate.
    let out = Command::new("desktop-file-validate")
        .arg(&desktop)
        .output()
        .expect("desktop-file-validate (desktop-file-utils) must be installed");
    assert!(
        out.status.success(),
        "desktop-file-validate rejected {}:\n{}{}\n{}",
        desktop.display(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
        fs::read_to_string(&desktop).unwrap_or_default()
    );

    // 6. Re-activate the same release (the Undo path; a second build would double the time),
    //    then uninstall.
    let again = installer.activate(&project.id, &release_id).await.unwrap();
    assert!(matches!(again.state, InstallState::Installed { .. }));
    assert_eq!(layout.active_version().as_deref(), Some("1.0.0"));
    assert!(matches!(
        installer.state(&project.id).await.unwrap(),
        InstallState::Installed { .. }
    ));

    installer.uninstall(&project.id).await.unwrap();
    assert!(!desktop.exists(), "launcher entry survived uninstall");
    assert!(layout.current_link().symlink_metadata().is_err());
    assert_eq!(
        installer.state(&project.id).await.unwrap(),
        InstallState::NotInstalled
    );
    assert!(conf.exists(), "uninstall must not touch the project");
    eprintln!(
        "real build test: total {:?} (build {build_time:?}), smoke {:?}",
        t0.elapsed(),
        release.smoke
    );
}
