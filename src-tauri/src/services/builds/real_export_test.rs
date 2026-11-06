//! The real export test (F22, eng review D11). Opt-in, because the first run builds the export
//! image (several GB) and each target compiles the template app:
//!
//! `STUDIO_EXPORT_E2E=1 cargo test real_export_test -- --nocapture`
//!
//! Run it before every Studio release (docs/QAToDo.md). Without the variable it skips at once.
//! The bundled tauri-vue template is copied the way `ProjectService` does, its source saved as
//! a release snapshot, and exported for both targets through `BuildService` with the real
//! docker. Every Studio path lives under one `TempDir`; docker's own image and cache volumes
//! are the only things left behind (clear them in Settings > Exports).
use std::collections::HashMap;
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
use crate::services::jobs::JobSupervisor;
use crate::services::projects::{GitCmd, ProjectService};

use export::{DockerState, ExportTarget};

/// Ceiling for the whole export: image build plus two cold compiles.
const EXPORT_TIMEOUT: Duration = Duration::from_secs(90 * 60);
/// The Ubuntu 22.04 baseline (D2): the Linux binary may need at most this glibc.
const MAX_GLIBC: (u32, u32) = (2, 35);

struct RealToolchain {
    doctor: Arc<DiagnosticsService>,
}

impl Toolchain for RealToolchain {
    fn doctor_report(
        &self,
        force: bool,
    ) -> Pin<Box<dyn Future<Output = DoctorReport> + Send + '_>> {
        Box::pin(async move { self.doctor.run(&HashMap::new(), force).await })
    }

    fn spawn_env(&self) -> Vec<(String, String)> {
        self.doctor.spawn_env()
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
    lines[lines.len().saturating_sub(60)..].join("\n")
}

/// The highest `GLIBC_x.y` an ELF binary references (`objdump -T`).
fn max_glibc(binary: &Path) -> (u32, u32) {
    let out = Command::new("objdump")
        .arg("-T")
        .arg(binary)
        .output()
        .expect("objdump (binutils) must be installed");
    assert!(
        out.status.success(),
        "objdump -T failed on {}",
        binary.display()
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let re = regex::Regex::new(r"GLIBC_(\d+)\.(\d+)").unwrap();
    re.captures_iter(&text)
        .map(|c| (c[1].parse().unwrap(), c[2].parse().unwrap()))
        .max()
        .unwrap_or((0, 0))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_export_test() {
    if std::env::var("STUDIO_EXPORT_E2E").as_deref() != Ok("1") {
        eprintln!("skipped: set STUDIO_EXPORT_E2E=1 to export with the real docker (slow, GBs)");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let paths = StudioPaths {
        data_dir: tmp.path().join("data"),
        config_dir: tmp.path().join("config"),
        cache_dir: tmp.path().join("cache"),
        default_projects_dir: tmp.path().join("projects"),
        applications_dir: tmp.path().join("applications"),
    };
    let db = Arc::new(Db::open_in_memory().unwrap());
    let doctor = Arc::new(DiagnosticsService::new());

    // 1. The template, copied the way the app creates a project.
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
            name: "Real Export".into(),
            slug: Some("real-export".into()),
            parent_dir: parent.canonicalize().unwrap().display().to_string(),
            identifier: None,
            init_git: true,
            database: false,
            create_github: None,
            icon: None,
        })
        .unwrap();
    let project = db.get_project(&summary.id).unwrap().unwrap();
    let project_dir = PathBuf::from(&project.workspace_path);
    let cfg = project::read_tauri_config(&project_dir).expect("template config");

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
        }),
        adapter: Arc::new(TauriAppImageAdapter),
        snapshots: snapshots.clone(),
        checkpoints,
        paths: paths.clone(),
        smoke_after: SMOKE_AFTER,
    });
    let ready = svc.export_readiness().await;
    assert_eq!(
        ready.docker,
        DockerState::Ready,
        "the real export test needs a running docker: {:?}",
        ready.detail
    );

    // 2. A release whose saved source is the template (what a build records, M4-D3).
    let release_id = "real-export-release";
    let (_, snapshot_ref) = snapshots
        .snapshot_build(&project.id, &project_dir, release_id, "export test")
        .unwrap();
    let layout = AppLayout::new(&paths, &project.id);
    fs::create_dir_all(layout.release_dir("1.0.0")).unwrap();
    fs::write(layout.release_appimage("1.0.0"), "placeholder").unwrap();
    db.insert_release(&ReleaseRow {
        id: release_id.into(),
        project_id: project.id.clone(),
        job_id: None,
        version: "1.0.0".into(),
        sha256: "ab".repeat(32),
        size_bytes: 1,
        architecture: std::env::consts::ARCH.into(),
        build_profile: "local".into(),
        product_name: cfg.product_name.clone(),
        main_binary_name: cfg.main_binary_name.clone(),
        source_commit: None,
        dirty_source: false,
        snapshot_ref: Some(snapshot_ref),
        smoke: Smoke::Unverified,
        smoke_detail: None,
        notes: None,
        created_at: "2026-01-01T00:00:00.000Z".into(),
        last_activated_at: None,
        files_removed_at: None,
        schema_version: None,
    })
    .unwrap();

    // 3. Export both targets in the real image.
    let t0 = Instant::now();
    svc.start_export(
        &project,
        release_id,
        vec![ExportTarget::Linux, ExportTarget::Windows],
        true,
    )
    .await
    .unwrap();
    let deadline = Instant::now() + EXPORT_TIMEOUT;
    let status = loop {
        if let Ok(Some(s)) = svc.status(&project.id)
            && !s.live
        {
            break s;
        }
        assert!(
            Instant::now() < deadline,
            "export still running after {EXPORT_TIMEOUT:?}\n--- log tail ---\n{}",
            log_tail(&svc, &project.id)
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    assert_eq!(
        status.state,
        JobState::Succeeded,
        "export failed: {:?}\n--- log tail ---\n{}",
        status.error_message,
        log_tail(&svc, &project.id)
    );

    // 4. Files exist, are nonempty and match SHA256SUMS.
    let listed = svc.list_release_exports(&project.id).unwrap();
    assert_eq!(listed.len(), 1);
    let find = |t: ExportTarget, kind: export::ExportFileKind| -> PathBuf {
        let target = listed[0].targets.iter().find(|v| v.target == t).unwrap();
        PathBuf::from(&target.files.iter().find(|f| f.kind == kind).unwrap().path)
    };
    for view in &listed[0].targets {
        assert!(!view.files.is_empty());
        for f in &view.files {
            let size = fs::metadata(&f.path).unwrap().len();
            assert!(
                size > 0 && f.size_bytes as u64 == size,
                "{} is empty",
                f.name
            );
            assert_eq!(
                sha256_file(Path::new(&f.path)).unwrap(),
                f.sha256,
                "{}",
                f.name
            );
        }
    }
    let appimage = find(ExportTarget::Linux, export::ExportFileKind::AppImage);
    find(ExportTarget::Linux, export::ExportFileKind::Deb);
    find(ExportTarget::Linux, export::ExportFileKind::Rpm);
    let setup = find(ExportTarget::Windows, export::ExportFileKind::Nsis);

    // 5. Windows: a PE file. Linux: the binary needs at most glibc 2.35.
    let head = fs::read(&setup).unwrap();
    assert_eq!(&head[..2], b"MZ", "setup.exe must be a Windows executable");
    let work = tmp.path().join("extract");
    fs::create_dir_all(&work).unwrap();
    let out = Command::new(&appimage)
        .arg("--appimage-extract")
        .current_dir(&work)
        .env("APPIMAGE_EXTRACT_AND_RUN", "1")
        .output()
        .expect("run the AppImage's own extractor");
    assert!(out.status.success(), "--appimage-extract failed");
    let bin = fs::read_dir(work.join("squashfs-root/usr/bin"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_file())
        .expect("the app binary inside the AppImage");
    let glibc = max_glibc(&bin);
    assert!(
        glibc <= MAX_GLIBC,
        "the Linux binary needs GLIBC_{}.{}, the baseline is {}.{}",
        glibc.0,
        glibc.1,
        MAX_GLIBC.0,
        MAX_GLIBC.1
    );
    eprintln!(
        "real export test: {:?}, glibc {}.{}",
        t0.elapsed(),
        glibc.0,
        glibc.1
    );
}
