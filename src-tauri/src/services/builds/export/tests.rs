//! Export tests: a fake `docker` script stands in for Docker (never the real one, never the
//! network). The release source is real Git (shadow repo, `git archive`), the doctor is not
//! used. Everything lives under a temp dir.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempfile::TempDir;

use super::docker;
use super::*;
use crate::domain::TemplateId;
use crate::persistence::{Db, JobState, NewProject, ProjectRow, ReleaseRow, Smoke};
use crate::platform::paths::{AppLayout, StudioPaths};
use crate::services::agents::claude::ClaudeAdapter;
use crate::services::builds::store::sha256_file;
use crate::services::builds::{
    BuildDeps, BuildError, BuildEvent, BuildPhase, BuildService, BuildStatus, SMOKE_AFTER,
    TauriAppImageAdapter,
};
use crate::services::diagnostics::DoctorReport;
use crate::services::git::checkpoint::CheckpointStore;
use crate::services::git::{GitRunner, SnapshotStore};
use crate::services::jobs::{JobKind, JobSupervisor};
use crate::services::preview::Toolchain;
use crate::services::settings;

/// Records every call in $FAKE_LOG; behaviour comes from env vars carried by `spawn_env`:
///   version: $FAKE_VERSION = noperm | down | hang
///   image inspect: $FAKE_IMAGE = absent
///   build (the image): $FAKE_BUILD = fail
///   system df: $FAKE_DF = hang
///   run ... install: $FAKE_INSTALL = fail
///   run ... tauri build, per target: $FAKE_LINUX / $FAKE_WINDOWS =
///     ok (default)   write the bundles, content carries $FAKE_TAG
///     fail           cargo-style error, exit 101
///     none           exit 0 without producing anything
///     hang           touch $FAKE_READY and stay up until stopped
///     wait           touch $FAKE_READY, then wait for $FAKE_GO (up to 10 s), then produce
const DOCKER: &str = r##"#!/bin/bash
echo "$*" >> "$FAKE_LOG"
case "$1" in
  version)
    case "$FAKE_VERSION" in
      noperm) echo "permission denied while trying to connect to the Docker daemon socket at unix:///var/run/docker.sock" >&2; exit 1;;
      down) echo "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?" >&2; exit 1;;
      hang) exec sleep 30;;
    esac
    echo 27.0.0; exit 0;;
  image)
    case "$2" in
      inspect) [ "$FAKE_IMAGE" = absent ] && exit 1; exit 0;;
      ls) echo "ai-app-studio-export:abc123"; exit 0;;
      *) exit 0;;
    esac;;
  build)
    for last; do :; done
    echo "DOCKERFILE: $(head -1 "$last/Dockerfile")" >> "$FAKE_LOG"
    echo "Step 1/9 : FROM ubuntu:22.04"
    [ "$FAKE_BUILD" = fail ] && { echo "error: network unreachable" >&2; exit 1; }
    exit 0;;
  system)
    [ "$FAKE_DF" = hang ] && exec sleep 30
    echo '{"Images":[{"Repository":"ai-app-studio-export","Tag":"abc123","Size":"2GB"}],"Volumes":[{"Name":"ai-app-studio-export-cargo","Size":"500MB"},{"Name":"other","Size":"9GB"}]}'
    exit 0;;
  run)
    src=""; prev=""
    for a in "$@"; do
      if [ "$prev" = "-v" ]; then case "$a" in *:/src) src="${a%:/src}";; esac; fi
      prev="$a"
    done
    echo "SRC: $(ls "$src" | tr '\n' ' ')" >> "$FAKE_LOG"
    args="$*"
    case "$args" in
      *" install"*)
        [ "$FAKE_INSTALL" = fail ] && { echo "ERR_PNPM_FETCH_404 boom" >&2; exit 1; }
        echo "Done in 1s"; exit 0;;
    esac
    case "$args" in
      *x86_64-pc-windows-msvc*) target=windows; mode="$FAKE_WINDOWS";;
      *) target=linux; mode="$FAKE_LINUX";;
    esac
    case "$mode" in
      fail) echo "   Compiling app v0.1.0"; echo "error[E0432]: unresolved import foo" >&2; exit 101;;
      none) echo "nothing"; exit 0;;
      hang) touch "$FAKE_READY"; exec sleep 30;;
      wait) touch "$FAKE_READY"; for i in $(seq 200); do [ -e "$FAKE_GO" ] && break; sleep 0.05; done;;
    esac
    echo "   Compiling app v0.1.0"
    if [ "$target" = linux ]; then
      b="$src/src-tauri/target/release/bundle"
      mkdir -p "$b/appimage" "$b/deb" "$b/rpm"
      echo "linux-appimage $FAKE_TAG" > "$b/appimage/app_1.0.0_amd64.AppImage"
      echo "linux-deb $FAKE_TAG" > "$b/deb/app_1.0.0_amd64.deb"
      echo "linux-rpm $FAKE_TAG" > "$b/rpm/app-1.0.0-1.x86_64.rpm"
    else
      b="$src/src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis"
      mkdir -p "$b"
      printf 'MZ windows %s' "$FAKE_TAG" > "$b/app_1.0.0_x64-setup.exe"
    fi
    exit 0;;
  *) exit 0;;
esac
"##;

struct FakeToolchain {
    env: Mutex<Vec<(String, String)>>,
    bin: PathBuf,
}

impl FakeToolchain {
    fn set(&self, key: &str, value: &str) {
        let mut env = self.env.lock().unwrap();
        env.retain(|(k, _)| k != key);
        env.push((key.into(), value.into()));
    }
}

impl Toolchain for FakeToolchain {
    fn doctor_report(
        &self,
        _force: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = DoctorReport> + Send + '_>> {
        Box::pin(async { unreachable!("exports never read the doctor report") })
    }

    fn spawn_env(&self) -> Vec<(String, String)> {
        self.env.lock().unwrap().clone()
    }

    fn resolve_binary(&self, tool: &str) -> Option<PathBuf> {
        let p = self.bin.join(tool);
        p.exists().then_some(p)
    }
}

const CONF: &str =
    r#"{"productName":"My App","version":"0.2.0","identifier":"com.x.a","bundle":{"active":true}}"#;

struct Ex {
    tmp: TempDir,
    db: Arc<Db>,
    svc: BuildService,
    sup: JobSupervisor<ClaudeAdapter>,
    snapshots: Arc<SnapshotStore>,
    tc: Arc<FakeToolchain>,
    project: ProjectRow,
    paths: StudioPaths,
    /// A kept release "1.0.0" with a saved source.
    release: ReleaseRow,
}

fn ex() -> Ex {
    let tmp = TempDir::new().unwrap();
    let bin = tmp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    fs::write(bin.join("docker"), DOCKER).unwrap();
    fs::set_permissions(bin.join("docker"), fs::Permissions::from_mode(0o755)).unwrap();
    let dir = tmp.path().join("project");
    fs::create_dir_all(dir.join("src-tauri")).unwrap();
    fs::write(dir.join("pnpm-lock.yaml"), "").unwrap();
    fs::write(dir.join("src-tauri/tauri.conf.json"), CONF).unwrap();
    fs::write(dir.join("README.md"), "hello\n").unwrap();

    let env: Vec<(String, String)> = vec![
        ("PATH".into(), "/usr/bin:/bin".into()),
        (
            "FAKE_LOG".into(),
            tmp.path().join("docker.log").display().to_string(),
        ),
        (
            "FAKE_READY".into(),
            tmp.path().join("ready").display().to_string(),
        ),
        (
            "FAKE_GO".into(),
            tmp.path().join("go").display().to_string(),
        ),
        ("FAKE_TAG".into(), "v1".into()),
    ];
    let db = Arc::new(Db::open_in_memory().unwrap());
    db.insert_project(&NewProject {
        id: "proj1".into(),
        name: "a".into(),
        slug: "a".into(),
        template_id: TemplateId::TauriVue,
        app_id: "com.x.a".into(),
        workspace_path: dir.display().to_string(),
        pending_op: None,
    })
    .unwrap();
    let project = db.get_project("proj1").unwrap().unwrap();
    let paths = StudioPaths {
        data_dir: tmp.path().join("data"),
        config_dir: tmp.path().join("config"),
        cache_dir: tmp.path().join("cache"),
        default_projects_dir: tmp.path().join("projects"),
        applications_dir: tmp.path().join("apps"),
    };
    let git_env = vec![
        ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
        ("HOME".into(), tmp.path().display().to_string()),
        ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
        ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
    ];
    let snapshots = Arc::new(SnapshotStore::new(
        &paths.data_dir,
        GitRunner {
            program: "git".into(),
            env: git_env.clone(),
        },
    ));
    let checkpoints = Arc::new(CheckpointStore {
        program: "git".into(),
        env: git_env,
    });
    let sup = JobSupervisor::new(
        db.clone(),
        snapshots.clone(),
        checkpoints.clone(),
        ClaudeAdapter::default(),
        paths.data_dir.clone(),
    );
    let tc = Arc::new(FakeToolchain {
        env: Mutex::new(env),
        bin,
    });
    let svc = BuildService::new(BuildDeps {
        db: db.clone(),
        slots: sup.slots(),
        toolchain: tc.clone(),
        adapter: Arc::new(TauriAppImageAdapter),
        snapshots: snapshots.clone(),
        checkpoints,
        paths: paths.clone(),
        smoke_after: SMOKE_AFTER,
    });
    let mut x = Ex {
        tmp,
        db,
        svc,
        sup,
        snapshots,
        tc,
        project,
        paths,
        release: ReleaseRow {
            id: String::new(),
            project_id: String::new(),
            job_id: None,
            version: String::new(),
            sha256: String::new(),
            size_bytes: 0,
            architecture: String::new(),
            build_profile: String::new(),
            product_name: String::new(),
            main_binary_name: String::new(),
            source_commit: None,
            dirty_source: false,
            snapshot_ref: None,
            smoke: Smoke::Passed,
            smoke_detail: None,
            notes: None,
            created_at: String::new(),
            last_activated_at: None,
            files_removed_at: None,
            schema_version: None,
        },
    };
    x.release = x.seed_release("1.0.0", "2026-01-01T00:00:00.000Z", true);
    x
}

impl Ex {
    fn dir(&self) -> PathBuf {
        PathBuf::from(&self.project.workspace_path)
    }

    fn layout(&self) -> AppLayout {
        AppLayout::new(&self.paths, "proj1")
    }

    fn exports(&self, version: &str) -> PathBuf {
        self.layout().release_dir(version).join("exports")
    }

    /// A kept release with files on disk; `with_source` records its saved source.
    fn seed_release(&self, version: &str, created: &str, with_source: bool) -> ReleaseRow {
        let l = self.layout();
        fs::create_dir_all(l.release_dir(version)).unwrap();
        fs::write(l.release_appimage(version), "x").unwrap();
        let id = format!("rel-{}", version.replace('.', "-"));
        let snapshot_ref = with_source.then(|| {
            self.snapshots
                .snapshot_build("proj1", &self.dir(), &id, "build")
                .unwrap()
                .1
        });
        let row = ReleaseRow {
            id,
            project_id: "proj1".into(),
            job_id: None,
            version: version.into(),
            sha256: "ab".repeat(32),
            size_bytes: 1,
            architecture: "x86_64".into(),
            build_profile: "local".into(),
            product_name: "My App".into(),
            main_binary_name: "my-app".into(),
            source_commit: None,
            dirty_source: false,
            snapshot_ref,
            smoke: Smoke::Passed,
            smoke_detail: None,
            notes: None,
            created_at: created.into(),
            last_activated_at: None,
            files_removed_at: None,
            schema_version: None,
        };
        self.db.insert_release(&row).unwrap();
        row
    }

    fn set(&self, key: &str, value: &str) {
        self.tc.set(key, value);
    }

    async fn start(
        &self,
        targets: &[ExportTarget],
        accept: bool,
    ) -> Result<BuildStatus, BuildError> {
        self.svc
            .start_export(&self.project, &self.release.id, targets.to_vec(), accept)
            .await
    }

    async fn finished(&self) -> BuildStatus {
        for _ in 0..750 {
            if let Ok(Some(s)) = self.svc.status("proj1")
                && !s.live
            {
                return s;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("export never ended; last {:?}", self.svc.status("proj1"));
    }

    /// Starts an export and waits for it to end.
    async fn export(&self, targets: &[ExportTarget]) -> BuildStatus {
        self.start(targets, true).await.unwrap();
        self.finished().await
    }

    async fn wait_for(&self, file: &str) {
        let p = self.tmp.path().join(file);
        for _ in 0..500 {
            if p.exists() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("{file} never appeared");
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.tmp.path().join("docker.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn runs(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|l| l.starts_with("run "))
            .collect()
    }

    fn job(&self, id: &str) -> crate::persistence::JobRow {
        self.db.get_job(id).unwrap().unwrap()
    }

    fn results(&self, id: &str) -> Vec<ExportResult> {
        serde_json::from_str(self.job(id).export_results.as_deref().unwrap()).unwrap()
    }

    fn no_leftovers(&self) {
        let tmp = self.paths.data_dir.join("tmp");
        let left: Vec<_> = fs::read_dir(&tmp)
            .map(|rd| rd.flatten().map(|e| e.file_name()).collect())
            .unwrap_or_default();
        assert!(left.is_empty(), "temp folders left: {left:?}");
        let ex = self.exports("1.0.0");
        let hidden: Vec<_> = fs::read_dir(&ex)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.starts_with('.'))
                    .collect()
            })
            .unwrap_or_default();
        assert!(hidden.is_empty(), "stage folders left: {hidden:?}");
    }
}

const BOTH: [ExportTarget; 2] = [ExportTarget::Linux, ExportTarget::Windows];

fn read(p: impl AsRef<Path>) -> String {
    fs::read_to_string(p).unwrap()
}

// -------------------------------------------------------------- readiness

#[tokio::test]
async fn readiness_is_classified() {
    let x = ex();
    let r = x.svc.export_readiness().await;
    assert_eq!(r.docker, DockerState::Ready);
    assert_eq!(r.docker_version.as_deref(), Some("27.0.0"));
    assert!(r.image_present);
    assert_eq!(r.license_accepted_at, None);
    assert_eq!(r.last_targets, BOTH);

    x.set("FAKE_IMAGE", "absent");
    assert!(!x.svc.export_readiness().await.image_present);

    x.set("FAKE_VERSION", "down");
    let r = x.svc.export_readiness().await;
    assert_eq!(r.docker, DockerState::NotRunning);
    assert!(r.detail.unwrap().contains("Cannot connect"));
    assert!(!r.image_present);

    x.set("FAKE_VERSION", "noperm");
    assert_eq!(
        x.svc.export_readiness().await.docker,
        DockerState::NoPermission
    );

    // No answer in time counts as not running.
    x.set("FAKE_VERSION", "hang");
    x.svc
        .set_docker_timeouts(Duration::from_millis(300), docker::DF_TIMEOUT);
    assert_eq!(
        x.svc.export_readiness().await.docker,
        DockerState::NotRunning
    );

    // No docker binary at all.
    fs::remove_file(x.tmp.path().join("bin/docker")).unwrap();
    assert_eq!(x.svc.export_readiness().await.docker, DockerState::Missing);
}

// --------------------------------------------------------------- refusals

#[tokio::test]
async fn an_export_needs_a_saved_source_and_leaves_nothing_behind() {
    let x = ex();
    let bare = x.seed_release("0.9.0", "2025-01-01T00:00:00.000Z", false);
    let e = x
        .svc
        .start_export(&x.project, &bare.id, vec![ExportTarget::Linux], false)
        .await
        .unwrap_err();
    assert_eq!(
        e.to_string(),
        "This release has no saved source. Build a new release to export it."
    );
    assert!(x.db.list_export_jobs("proj1").unwrap().is_empty());
    assert!(x.calls().is_empty(), "docker must not be asked first");
    // The slot is free again.
    assert!(x.sup.slots().job_id("proj1", JobKind::Build).is_none());
    x.start(&[ExportTarget::Linux], false).await.unwrap();
    x.finished().await;
}

#[tokio::test]
async fn windows_needs_the_license_once() {
    let x = ex();
    let e = x.start(&[ExportTarget::Windows], false).await.unwrap_err();
    assert!(e.to_string().contains("Microsoft"), "{e}");
    assert_eq!(settings::export_license_accepted_at(&x.db), None);
    assert!(x.db.list_export_jobs("proj1").unwrap().is_empty());
    // Linux alone never asks.
    x.start(&[ExportTarget::Linux], false).await.unwrap();
    x.finished().await;
    assert_eq!(settings::export_license_accepted_at(&x.db), None);

    // Accepting stores the date; later exports no longer ask.
    x.start(&[ExportTarget::Windows], true).await.unwrap();
    x.finished().await;
    assert!(settings::export_license_accepted_at(&x.db).is_some());
    x.start(&[ExportTarget::Windows], false).await.unwrap();
    x.finished().await;

    // Withdraw brings the question back.
    x.svc.withdraw_export_license().unwrap();
    assert_eq!(x.svc.export_readiness().await.license_accepted_at, None);
    assert!(x.start(&[ExportTarget::Windows], false).await.is_err());
}

#[tokio::test]
async fn a_busy_slot_a_modifying_run_and_a_missing_docker_refuse() {
    let x = ex();
    assert!(x.sup.slots().claim("proj1", JobKind::Build, "other"));
    let e = x.start(&BOTH, true).await.unwrap_err();
    assert!(matches!(e, BuildError::Busy), "{e}");
    x.sup.slots().release("proj1", JobKind::Build);

    assert!(x.sup.slots().claim("proj1", JobKind::Agent, "agent-job"));
    let e = x.start(&BOTH, true).await.unwrap_err();
    assert!(matches!(e, BuildError::AgentRunning), "{e}");
    x.sup.slots().release("proj1", JobKind::Agent);

    x.set("FAKE_VERSION", "noperm");
    let e = x.start(&BOTH, true).await.unwrap_err();
    assert!(e.to_string().contains("docker group"), "{e}");
    x.set("FAKE_VERSION", "down");
    let e = x.start(&BOTH, true).await.unwrap_err();
    assert!(e.to_string().contains("systemctl start docker"), "{e}");
    // Refused exports leave no job and free the slot.
    assert!(x.db.list_export_jobs("proj1").unwrap().is_empty());
    assert!(x.sup.slots().job_id("proj1", JobKind::Build).is_none());
    // The license is not stored by an export that never started.
    assert_eq!(settings::export_license_accepted_at(&x.db), None);

    let e = x
        .svc
        .start_export(&x.project, "nope", vec![ExportTarget::Linux], false)
        .await
        .unwrap_err();
    assert!(matches!(e, BuildError::NotFound));
    let e = x.start(&[], false).await.unwrap_err();
    assert!(e.to_string().contains("at least one"), "{e}");
}

// ---------------------------------------------------------------- the job

#[tokio::test]
async fn a_full_export_stores_files_with_checksums() {
    let x = ex();
    x.set("FAKE_IMAGE", "absent");
    let mut events = x.svc.subscribe();
    let started = x.start(&BOTH, true).await.unwrap();
    let export = started.export.as_ref().unwrap();
    assert_eq!(export.version, "1.0.0");
    assert_eq!(export.targets, BOTH);
    assert!(export.first_run, "no image yet");
    assert_eq!(started.release_id.as_deref(), Some(x.release.id.as_str()));
    let s = x.finished().await;
    assert_eq!(s.state, JobState::Succeeded, "{:?}", s.error_message);
    assert_eq!(
        s.export.as_ref().unwrap().results,
        [
            ExportResult {
                target: ExportTarget::Linux,
                ok: true,
                message: None
            },
            ExportResult {
                target: ExportTarget::Windows,
                ok: true,
                message: None
            }
        ]
    );

    // The files, their checksums, and the Windows MZ header.
    let ex = x.exports("1.0.0");
    for (target, names) in [
        (
            "linux",
            vec![
                "app_1.0.0_amd64.AppImage",
                "app_1.0.0_amd64.deb",
                "app-1.0.0-1.x86_64.rpm",
            ],
        ),
        ("windows", vec!["app_1.0.0_x64-setup.exe"]),
    ] {
        let dir = ex.join(target);
        let sums = read(dir.join("SHA256SUMS"));
        assert_eq!(sums.lines().count(), names.len());
        for n in names {
            let sha = sha256_file(&dir.join(n)).unwrap();
            assert!(sums.contains(&format!("{sha}  {n}\n")), "{n} not in {sums}");
        }
    }
    assert!(read(ex.join("windows/app_1.0.0_x64-setup.exe")).starts_with("MZ"));
    let mode = fs::metadata(ex.join("linux/app_1.0.0_amd64.AppImage"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o111, 0o111, "the AppImage is executable");

    // The job row, the results document and what the UI lists.
    let job = x.job(&s.job_id);
    assert_eq!(job.state, JobState::Succeeded);
    assert_eq!(
        job.export_release_id.as_deref(),
        Some(x.release.id.as_str())
    );
    assert_eq!(job.export_targets.as_deref(), Some("linux,windows"));
    assert_eq!(x.results(&s.job_id).len(), 2);
    let listed = x.svc.list_release_exports("proj1").unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].release_id, x.release.id);
    let linux = &listed[0].targets[0];
    assert_eq!(linux.target, ExportTarget::Linux);
    assert_eq!(linux.files.len(), 3);
    assert!(linux.built_at.is_some() && linux.failed.is_none());
    assert_eq!(
        linux.files[0].sha256,
        sha256_file(Path::new(&linux.files[0].path)).unwrap()
    );
    assert!(linux.files.iter().all(|f| f.size_bytes > 0.0));
    assert_eq!(listed[0].targets[1].files[0].kind, ExportFileKind::Nsis);

    // Remembered targets, a clean temp area, the log and the phases.
    assert_eq!(settings::export_last_targets(&x.db), BOTH);
    x.no_leftovers();
    let log = x.svc.log_tail("proj1", None).unwrap().unwrap().lines;
    assert!(
        log.iter()
            .any(|l| l.contains("Preparing the build environment"))
    );
    let mut phases = Vec::new();
    while let Ok(e) = events.try_recv() {
        if let BuildEvent::Status(st) = e
            && let Some(p) = st.phase
            && phases.last() != Some(&p)
        {
            phases.push(p);
        }
    }
    assert_eq!(
        phases,
        [
            BuildPhase::Preflight,
            BuildPhase::Environment,
            BuildPhase::Installing,
            BuildPhase::ExportLinux,
            BuildPhase::ExportWindows
        ]
    );
}

#[tokio::test]
async fn docker_is_called_with_the_safe_arguments() {
    let x = ex();
    x.set("FAKE_IMAGE", "absent");
    let s = x.export(&BOTH).await;
    assert_eq!(s.state, JobState::Succeeded);
    let calls = x.calls();
    // The image is built from the embedded Dockerfile on first use.
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with("build -t ai-app-studio-export:"))
    );
    assert!(
        calls.iter().any(|c| c.starts_with("DOCKERFILE: FROM ")
            || c.starts_with("DOCKERFILE: # The export build"))
    );
    let runs = x.runs();
    assert_eq!(runs.len(), 3, "install, linux, windows: {runs:?}");
    let (uid, gid) = docker::host_ids();
    let name = format!("--name studio-export-{}", s.job_id);
    for r in &runs {
        assert!(r.starts_with("run --rm "), "{r}");
        assert!(r.contains(&name), "{r}");
        assert!(r.contains(&format!("--user {uid}:{gid}")), "{r}");
        assert_eq!(r.matches(":/src").count(), 1, "one source mount: {r}");
        assert!(r.contains(&format!(
            "-v {}/data/tmp/export-{}:/src",
            x.tmp.path().display(),
            s.job_id
        )));
        for vol in docker::VOLUMES {
            assert!(r.contains(vol), "{r}");
        }
        assert!(
            !r.contains("docker.sock") && !r.contains("--privileged"),
            "{r}"
        );
        assert!(
            !r.contains(&x.project.workspace_path),
            "the live project is never mounted"
        );
    }
    assert!(
        runs[0].ends_with(" pnpm install --prefer-offline"),
        "{}",
        runs[0]
    );
    assert!(
        runs[1].ends_with(
            r#" pnpm exec tauri build --bundles appimage,deb,rpm --config {"version":"1.0.0"}"#
        ),
        "{}",
        runs[1]
    );
    assert!(
        runs[2].ends_with(r#" pnpm exec tauri build --bundles nsis --runner cargo-xwin --target x86_64-pc-windows-msvc --config {"version":"1.0.0"}"#),
        "{}",
        runs[2]
    );
    // The release's saved source was extracted, not the live folder's git state.
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with("SRC: ") && c.contains("README.md") && c.contains("src-tauri"))
    );
    // With the image present, no build happens.
    let before = x.calls().len();
    x.set("FAKE_IMAGE", "present");
    x.export(&[ExportTarget::Linux]).await;
    assert!(x.calls()[before..].iter().all(|c| !c.starts_with("build ")));
}

#[tokio::test]
async fn partial_success_keeps_what_worked_and_the_old_files_of_what_failed() {
    let x = ex();
    let first = x.export(&BOTH).await;
    assert_eq!(first.state, JobState::Succeeded);
    let ex = x.exports("1.0.0");
    assert!(read(ex.join("linux/app_1.0.0_amd64.AppImage")).contains("v1"));

    // Second export: Linux works (new files), Windows fails (old files stay, design 15).
    x.set("FAKE_TAG", "v2");
    x.set("FAKE_WINDOWS", "fail");
    let s = x.export(&BOTH).await;
    assert_eq!(s.state, JobState::Failed);
    assert_eq!(s.error_kind.as_deref(), Some("exportFailed"));
    let msg = s.error_message.unwrap();
    assert!(msg.starts_with("Export failed for Windows: "), "{msg}");
    assert!(msg.contains("unresolved import"), "{msg}");
    assert!(!msg.contains("Linux"));
    assert!(read(ex.join("linux/app_1.0.0_amd64.AppImage")).contains("v2"));
    assert!(read(ex.join("windows/app_1.0.0_x64-setup.exe")).contains("v1"));
    assert!(ex.join("windows/SHA256SUMS").exists());
    let results = x.results(&s.job_id);
    assert_eq!(
        results.iter().map(|r| (r.target, r.ok)).collect::<Vec<_>>(),
        [(ExportTarget::Linux, true), (ExportTarget::Windows, false)]
    );
    assert!(
        results[1]
            .message
            .as_ref()
            .unwrap()
            .contains("unresolved import")
    );
    x.no_leftovers();

    // The listing: Windows keeps its files and shows the failure; Linux has none.
    let listed = x.svc.list_release_exports("proj1").unwrap();
    let windows = listed[0]
        .targets
        .iter()
        .find(|t| t.target == ExportTarget::Windows)
        .unwrap();
    assert_eq!(windows.files.len(), 1);
    assert!(
        windows
            .failed
            .as_ref()
            .unwrap()
            .message
            .contains("unresolved import")
    );
    assert!(listed[0].targets[0].failed.is_none());

    // A Linux-only export in between does not hide the Windows failure; a good retry clears it.
    x.set("FAKE_TAG", "v3");
    x.export(&[ExportTarget::Linux]).await;
    let listed = x.svc.list_release_exports("proj1").unwrap();
    assert!(listed[0].targets[1].failed.is_some());
    x.set("FAKE_WINDOWS", "ok");
    let s = x.export(&[ExportTarget::Windows]).await;
    assert_eq!(s.state, JobState::Succeeded);
    let listed = x.svc.list_release_exports("proj1").unwrap();
    assert!(listed[0].targets[1].failed.is_none());
    assert!(read(ex.join("windows/app_1.0.0_x64-setup.exe")).contains("v3"));
}

#[tokio::test]
async fn a_target_without_a_fresh_bundle_fails_on_its_own() {
    let x = ex();
    x.set("FAKE_LINUX", "none");
    let s = x.export(&BOTH).await;
    assert_eq!(s.state, JobState::Failed);
    assert_eq!(
        s.error_message.as_deref(),
        Some("Export failed for Linux: No AppImage was produced")
    );
    assert!(!x.exports("1.0.0").join("linux").exists());
    assert!(x.exports("1.0.0").join("windows/SHA256SUMS").exists());
    // Only a failure with no files: the listing still names the target.
    let listed = x.svc.list_release_exports("proj1").unwrap();
    let linux = &listed[0].targets[0];
    assert!(linux.files.is_empty() && linux.built_at.is_none() && linux.failed.is_some());
}

#[tokio::test]
async fn install_and_image_failures_fail_every_target() {
    let x = ex();
    x.set("FAKE_INSTALL", "fail");
    let s = x.export(&BOTH).await;
    assert_eq!(s.state, JobState::Failed);
    let msg = s.error_message.unwrap();
    assert!(
        msg.starts_with("Export failed for Linux and Windows"),
        "{msg}"
    );
    assert!(msg.contains("ERR_PNPM_FETCH_404"), "{msg}");
    assert_eq!(x.results(&s.job_id).iter().filter(|r| !r.ok).count(), 2);
    x.no_leftovers();

    x.set("FAKE_INSTALL", "ok");
    x.set("FAKE_IMAGE", "absent");
    x.set("FAKE_BUILD", "fail");
    let s = x.export(&BOTH).await;
    assert_eq!(s.state, JobState::Failed);
    assert!(
        s.error_message
            .unwrap()
            .contains("Preparing the build environment failed: error: network unreachable")
    );
    x.no_leftovers();
}

#[tokio::test]
async fn a_release_removed_meanwhile_discards_the_export() {
    let x = ex();
    x.set("FAKE_LINUX", "wait");
    x.start(&[ExportTarget::Linux], false).await.unwrap();
    x.wait_for("ready").await;
    // A delete that slipped past the guard: row marked, folder gone.
    x.db.mark_release_files_removed(&x.release.id).unwrap();
    fs::remove_dir_all(x.layout().release_dir("1.0.0")).unwrap();
    fs::write(x.tmp.path().join("go"), "").unwrap();
    let s = x.finished().await;
    assert_eq!(s.state, JobState::Failed);
    assert!(s.error_message.unwrap().contains("removed while exporting"));
    // Nothing was orphaned: the release folder was not brought back.
    assert!(!x.layout().release_dir("1.0.0").exists());
    x.no_leftovers();
}

#[tokio::test]
async fn stop_cancels_stops_the_container_and_frees_the_slot() {
    let x = ex();
    x.set("FAKE_LINUX", "hang");
    let started = x.start(&BOTH, true).await.unwrap();
    x.wait_for("ready").await;
    assert!(x.sup.slots().job_id("proj1", JobKind::Build).is_some());
    x.svc.stop("proj1").await;
    let s = x.finished().await;
    assert_eq!(s.state, JobState::Cancelled);
    assert_eq!(s.error_kind.as_deref(), Some("cancelled"));
    assert!(
        x.calls()
            .contains(&format!("stop -t 3 studio-export-{}", started.job_id)),
        "{:?}",
        x.calls()
    );
    x.no_leftovers();
    assert!(!x.exports("1.0.0").join("linux").exists());
    assert!(x.sup.slots().job_id("proj1", JobKind::Build).is_none());
    // The slot is usable again.
    x.set("FAKE_LINUX", "ok");
    assert_eq!(
        x.export(&[ExportTarget::Linux]).await.state,
        JobState::Succeeded
    );
}

// ------------------------------------------------------------ status, guard

#[tokio::test]
async fn build_status_skips_export_rows() {
    let x = ex();
    // A real build job, then an export.
    x.db.insert_job_of_kind("build-1", "proj1", "build", JobState::Failed)
        .unwrap();
    let s = x.export(&[ExportTarget::Linux]).await;
    assert!(s.export.is_some());
    // This session still shows the export it ran.
    assert!(x.svc.status("proj1").unwrap().unwrap().export.is_some());
    // After a restart only the build job is "the latest build"; the export row is skipped.
    let again = BuildService::new(BuildDeps {
        db: x.db.clone(),
        slots: Arc::default(),
        toolchain: x.tc.clone(),
        adapter: Arc::new(TauriAppImageAdapter),
        snapshots: x.snapshots.clone(),
        checkpoints: Arc::new(CheckpointStore {
            program: "git".into(),
            env: vec![],
        }),
        paths: x.paths.clone(),
        smoke_after: SMOKE_AFTER,
    });
    let st = again.status("proj1").unwrap().unwrap();
    assert_eq!(st.job_id, "build-1");
    assert!(st.export.is_none());
    assert_eq!(
        again
            .log_tail("proj1", None)
            .unwrap()
            .map(|t| t.job_id)
            .as_deref(),
        None
    );

    // With only an export row there is no latest build at all.
    let only = ex();
    only.export(&[ExportTarget::Linux]).await;
    let svc = BuildService::new(BuildDeps {
        db: only.db.clone(),
        slots: Arc::default(),
        toolchain: only.tc.clone(),
        adapter: Arc::new(TauriAppImageAdapter),
        snapshots: only.snapshots.clone(),
        checkpoints: Arc::new(CheckpointStore {
            program: "git".into(),
            env: vec![],
        }),
        paths: only.paths.clone(),
        smoke_after: SMOKE_AFTER,
    });
    assert!(svc.status("proj1").unwrap().is_none());
}

#[tokio::test]
async fn delete_and_pruning_refuse_while_the_release_is_exported() {
    let x = ex();
    x.db.insert_job_of_kind("exp", "proj1", "build", JobState::Running)
        .unwrap();
    x.db.set_job_export("exp", &x.release.id, "linux").unwrap();
    let e = x
        .svc
        .delete_release("proj1", &x.release.id)
        .await
        .unwrap_err();
    assert_eq!(e.to_string(), "Wait until the export finishes.");
    assert!(x.layout().release_dir("1.0.0").exists());
    assert!(
        x.db.get_release(&x.release.id)
            .unwrap()
            .unwrap()
            .files_removed_at
            .is_none()
    );

    // Retention: seven kept releases, the two oldest are victims; the exported one stays.
    for (i, v) in ["1.0.1", "1.0.2", "1.0.3", "1.0.4", "1.0.5", "1.0.6"]
        .iter()
        .enumerate()
    {
        x.seed_release(v, &format!("2026-02-0{}T00:00:00.000Z", i + 1), false);
    }
    x.seed_release("0.9.0", "2025-12-01T00:00:00.000Z", false);
    super::super::apply_retention(&x.svc.inner.deps, &x.project).unwrap();
    assert!(
        x.layout().release_dir("1.0.0").exists(),
        "exported release kept"
    );
    assert!(
        !x.layout().release_dir("0.9.0").exists(),
        "other victim removed"
    );

    // Once the export has ended the delete goes through.
    x.db.transition_job(
        "exp",
        &[JobState::Running],
        JobState::Succeeded,
        &crate::persistence::JobUpdate::default(),
    )
    .unwrap();
    x.svc.delete_release("proj1", &x.release.id).await.unwrap();
    assert!(!x.layout().release_dir("1.0.0").exists());
}

#[tokio::test]
async fn leftovers_of_a_killed_export_are_removed_at_startup() {
    let x = ex();
    let tmp = x.paths.data_dir.join("tmp");
    fs::create_dir_all(tmp.join("export-j1/src")).unwrap();
    fs::write(tmp.join("export-j1.tar"), "t").unwrap();
    fs::create_dir_all(tmp.join("other")).unwrap();
    let ex = x.exports("1.0.0");
    fs::create_dir_all(ex.join(".tmp-j1/linux")).unwrap();
    // A crash between the two renames: the old files come back.
    fs::create_dir_all(ex.join(".old-j1-windows")).unwrap();
    fs::write(ex.join(".old-j1-windows/SHA256SUMS"), "").unwrap();
    // A newer export took over: the old copy goes.
    fs::create_dir_all(ex.join("linux")).unwrap();
    fs::create_dir_all(ex.join(".old-j1-linux")).unwrap();
    x.svc.cleanup_on_startup();
    assert!(!tmp.join("export-j1").exists() && !tmp.join("export-j1.tar").exists());
    assert!(tmp.join("other").exists());
    assert!(!ex.join(".tmp-j1").exists() && !ex.join(".old-j1-linux").exists());
    assert!(ex.join("windows/SHA256SUMS").exists());
    assert!(ex.join("linux").exists());
}

#[test]
fn only_build_refs_are_archived() {
    let x = ex();
    let dest = x.tmp.path().join("out");
    fs::create_dir(&dest).unwrap();
    for bad in ["refs/heads/main", "HEAD", "refs/studio/builds/../../x"] {
        assert!(
            x.snapshots.archive_build("proj1", bad, &dest).is_err(),
            "{bad}"
        );
    }
    let good = x.release.snapshot_ref.clone().unwrap();
    x.snapshots.archive_build("proj1", &good, &dest).unwrap();
    assert_eq!(read(dest.join("README.md")), "hello\n");
    assert!(!x.tmp.path().join("out.tar").exists());
}

// ----------------------------------------------------------------- caches

#[tokio::test]
async fn cache_size_sums_the_export_volumes_and_images_and_times_out() {
    let x = ex();
    assert_eq!(x.svc.export_cache_size().await.unwrap().bytes, 2.5e9);
    x.set("FAKE_DF", "hang");
    x.svc
        .set_docker_timeouts(docker::PROBE_TIMEOUT, Duration::from_millis(300));
    let e = x.svc.export_cache_size().await.unwrap_err();
    assert_eq!(e.to_string(), "Couldn't read Docker volumes");
    // No docker: nothing to measure.
    fs::remove_file(x.tmp.path().join("bin/docker")).unwrap();
    assert_eq!(x.svc.export_cache_size().await.unwrap().bytes, 0.0);
}

#[tokio::test]
async fn clearing_the_caches_is_refused_while_an_export_runs() {
    let x = ex();
    x.db.insert_job_of_kind("exp", "proj1", "build", JobState::Running)
        .unwrap();
    x.db.set_job_export("exp", &x.release.id, "linux").unwrap();
    let e = x.svc.clear_export_caches().await.unwrap_err();
    assert_eq!(e.to_string(), "Wait until the export finishes.");
    assert!(x.calls().is_empty(), "nothing was removed");

    x.db.transition_job(
        "exp",
        &[JobState::Running],
        JobState::Failed,
        &crate::persistence::JobUpdate::default(),
    )
    .unwrap();
    x.svc.clear_export_caches().await.unwrap();
    let calls = x.calls();
    assert!(calls.contains(&"volume rm -f ai-app-studio-export-cargo ai-app-studio-export-pnpm ai-app-studio-export-npm ai-app-studio-export-xwin".to_owned()), "{calls:?}");
    assert!(
        calls.contains(&"image rm -f ai-app-studio-export:abc123".to_owned()),
        "{calls:?}"
    );
}
