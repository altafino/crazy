//! Build service tests: a fake `pnpm` stands in for the package manager and `tauri build`
//! (it copies a fixture AppImage into the bundle folder), the doctor is a canned report, Git
//! and the shadow repo are real. The fixture AppImage is a copy of `/usr/bin/true` (or
//! `false`) with the type-2 magic written into the ELF header's padding, which is how real
//! AppImages carry it, so it passes verification and really runs for the test launch.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use tempfile::TempDir;

use super::*;
use crate::domain::TemplateId;
use crate::persistence::{NewProject, SessionRow};
use crate::platform::graphics::GraphicsInfo;
use crate::services::agents::claude::ClaudeAdapter;
use crate::services::diagnostics::{GraphicsRow, ShellEnv, ShellStatus, ToolSource, ToolStatus};
use crate::services::git::GitRunner;
use crate::services::jobs::JobSupervisor;

/// Records every call in $FAKE_LOG; behaviour comes from env vars carried by `spawn_env`.
///   install: $FAKE_INSTALL = fail -> exit 1
///   build (`exec tauri build ...`): $FAKE_BUILD =
///     ok (default)  copy $FAKE_APPIMAGE into $FAKE_BUNDLE, exit 0
///     fail          cargo-style errors, exit 101
///     plain         unrecognised output, exit 3
///     none          exit 0 without producing anything
///     hang          stay up until stopped (leaves a child; pid in $FAKE_PIDFILE)
///     two           two new AppImages
///     junk / empty  an AppImage that is a shell script / zero bytes
///     badapp        an AppImage that starts and exits 1
const SCRIPT: &str = r##"#!/bin/bash
echo "$* | NO_STRIP=$NO_STRIP" >> "$FAKE_LOG"
case "$*" in
  install*) [ "$FAKE_INSTALL" = fail ] && { echo "ERR_PNPM_FETCH boom" >&2; exit 1; }; echo "Done in 1s"; exit 0;;
esac
case "$FAKE_BUILD" in
  fail) echo "   Compiling app v0.1.0"; echo "error[E0432]: unresolved import foo" >&2; echo "error: could not compile \`app\`" >&2; exit 101;;
  plain) echo "something odd happened"; exit 3;;
  none) echo "nothing"; exit 0;;
  hang) echo "   Compiling app v0.1.0"; sleep 30 & echo $! > "$FAKE_PIDFILE"; wait;;
  junk) mkdir -p "$FAKE_BUNDLE"; printf '#!/bin/sh\necho hi\n' > "$FAKE_BUNDLE/x.AppImage"; exit 0;;
  empty) mkdir -p "$FAKE_BUNDLE"; : > "$FAKE_BUNDLE/x.AppImage"; exit 0;;
  badapp) mkdir -p "$FAKE_BUNDLE"; cp "$FAKE_APPIMAGE_FAIL" "$FAKE_BUNDLE/x.AppImage"; exit 0;;
  two) mkdir -p "$FAKE_BUNDLE"; cp "$FAKE_APPIMAGE" "$FAKE_BUNDLE/a.AppImage"; cp "$FAKE_APPIMAGE" "$FAKE_BUNDLE/b.AppImage"; exit 0;;
  *) echo "   Compiling app v0.1.0"; mkdir -p "$FAKE_BUNDLE"; cp "$FAKE_APPIMAGE" "$FAKE_BUNDLE/app_1.0.0_amd64.AppImage"; echo "    Bundling app_1.0.0_amd64.AppImage"; exit 0;;
esac
"##;

const TOOLS: [(&str, &str, &str); 7] = [
    ("node", "Node.js", "sudo pacman -S nodejs"),
    ("pnpm", "pnpm", "sudo pacman -S pnpm"),
    ("cargo", "Cargo", "Install Rust with rustup"),
    ("rustc", "Rust compiler", "Install Rust with rustup"),
    (
        "webkit2gtk",
        "WebKitGTK 4.1",
        "sudo pacman -S webkit2gtk-4.1",
    ),
    ("xdg-open", "xdg-open", "sudo pacman -S xdg-utils"),
    ("libfuse2", "libfuse2 (AppImage)", "sudo pacman -S fuse2"),
];

fn report(checked_at: u32, missing: &[&str]) -> DoctorReport {
    let tools = TOOLS
        .iter()
        .map(|(id, name, hint)| ToolStatus {
            id: (*id).into(),
            name: (*name).into(),
            state: if missing.contains(id) {
                ToolState::Missing {
                    hint: (*hint).into(),
                }
            } else {
                ToolState::Installed {
                    version: "1".into(),
                    path: "/x".into(),
                    source: ToolSource::ShellPath,
                }
            },
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

struct FakeToolchain {
    report: Mutex<DoctorReport>,
    forced: AtomicU32,
    env: Vec<(String, String)>,
    bin: PathBuf,
}

impl Toolchain for FakeToolchain {
    fn doctor_report(
        &self,
        force: bool,
    ) -> std::pin::Pin<Box<dyn Future<Output = DoctorReport> + Send + '_>> {
        if force {
            self.forced.fetch_add(1, Ordering::SeqCst);
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

/// A runnable file with the AppImage type-2 magic: `src` (an ELF) with `AI\x02` at offset 8.
fn appimage_fixture(dir: &Path, name: &str, src: &str) -> PathBuf {
    let mut bytes = fs::read(src).unwrap();
    bytes[8..11].copy_from_slice(b"AI\x02");
    let p = dir.join(name);
    fs::write(&p, bytes).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn script(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn git_ok(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .is_ok_and(|o| o.status.success())
}

struct Fx {
    tmp: TempDir,
    db: Arc<Db>,
    svc: BuildService,
    sup: JobSupervisor<ClaudeAdapter>,
    snapshots: Arc<SnapshotStore>,
    tc: Arc<FakeToolchain>,
    project: ProjectRow,
    paths: StudioPaths,
}

const CONF: &str =
    r#"{"productName":"My App","version":"0.2.0","identifier":"com.x.a","bundle":{"active":true}}"#;

/// `env` overrides the base environment of the fake toolchain (an empty value removes a key
/// for the display check, which treats empty as unset).
fn fx_with(
    template: TemplateId,
    conf: &str,
    adapter: Arc<dyn BuildAdapter>,
    env: &[(&str, &str)],
) -> Fx {
    let tmp = TempDir::new().unwrap();
    let bin = tmp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    fs::write(bin.join("pnpm"), SCRIPT).unwrap();
    fs::set_permissions(bin.join("pnpm"), fs::Permissions::from_mode(0o755)).unwrap();
    let dir = tmp.path().join("project");
    fs::create_dir_all(dir.join("src-tauri/icons")).unwrap();
    fs::write(dir.join("pnpm-lock.yaml"), "").unwrap();
    fs::write(dir.join("src-tauri/tauri.conf.json"), conf).unwrap();
    fs::write(dir.join("src-tauri/icons/128x128@2x.png"), "png").unwrap();
    fs::write(dir.join("README.md"), "hello\n").unwrap();

    let fixtures = tmp.path().join("fixtures");
    fs::create_dir(&fixtures).unwrap();
    let valid = appimage_fixture(&fixtures, "valid.AppImage", "/usr/bin/true");
    let failing = appimage_fixture(&fixtures, "failing.AppImage", "/usr/bin/false");
    let mut full_env: Vec<(String, String)> = vec![
        ("PATH".into(), format!("{}:/usr/bin:/bin", bin.display())),
        ("DISPLAY".into(), ":99".into()),
        (
            "FAKE_LOG".into(),
            tmp.path().join("calls.log").display().to_string(),
        ),
        (
            "FAKE_PIDFILE".into(),
            tmp.path().join("pid").display().to_string(),
        ),
        ("FAKE_APPIMAGE".into(), valid.display().to_string()),
        ("FAKE_APPIMAGE_FAIL".into(), failing.display().to_string()),
        (
            "FAKE_BUNDLE".into(),
            dir.join("src-tauri/target/release/bundle/appimage")
                .display()
                .to_string(),
        ),
    ];
    for (k, v) in env {
        full_env.retain(|(ek, _)| ek != k);
        full_env.push(((*k).into(), (*v).into()));
    }

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
        report: Mutex::new(report(now_secs(), &[])),
        forced: AtomicU32::new(0),
        env: full_env,
        bin,
    });
    let svc = BuildService::new(BuildDeps {
        db: db.clone(),
        slots: sup.slots(),
        toolchain: tc.clone(),
        adapter,
        snapshots: snapshots.clone(),
        checkpoints,
        paths: paths.clone(),
        smoke_after: Duration::from_millis(400),
    });
    Fx {
        tmp,
        db,
        svc,
        sup,
        snapshots,
        tc,
        project,
        paths,
    }
}

fn fx(env: &[(&str, &str)]) -> Fx {
    fx_with(
        TemplateId::TauriVue,
        CONF,
        Arc::new(TauriAppImageAdapter),
        env,
    )
}

impl Fx {
    fn dir(&self) -> PathBuf {
        PathBuf::from(&self.project.workspace_path)
    }

    fn layout(&self) -> AppLayout {
        AppLayout::new(&self.paths, "proj1")
    }

    fn start(&self, version: &str) -> Result<BuildStatus, BuildError> {
        self.svc.start(&self.project, version.into(), None)
    }

    fn status(&self) -> BuildStatus {
        self.svc.status("proj1").unwrap().unwrap()
    }

    async fn wait(&self, what: &str, pred: impl Fn(&BuildStatus) -> bool) -> BuildStatus {
        for _ in 0..750 {
            if let Ok(Some(s)) = self.svc.status("proj1")
                && pred(&s)
            {
                return s;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("never reached: {what}; last {:?}", self.svc.status("proj1"));
    }

    async fn finished(&self) -> BuildStatus {
        self.wait("build ended", |s| !s.live).await
    }

    /// Starts a build and waits for it to end.
    async fn build(&self, version: &str) -> BuildStatus {
        self.start(version).unwrap();
        self.finished().await
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.tmp.path().join("calls.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn log(&self) -> Vec<String> {
        self.svc.log_tail("proj1", None).unwrap().unwrap().lines
    }

    fn releases(&self) -> Vec<ReleaseRow> {
        self.db.list_releases("proj1", true).unwrap()
    }

    fn job_row(&self, id: &str) -> crate::persistence::JobRow {
        self.db.get_job(id).unwrap().unwrap()
    }

    fn init_git(&self) -> String {
        let d = self.dir();
        git(&d, &["init", "-q", "-b", "main"]);
        git(&d, &["add", "."]);
        git(&d, &["commit", "-qm", "first"]);
        git(&d, &["rev-parse", "HEAD"])
    }

    fn shadow_has(&self, refname: &str) -> bool {
        let shadow = self.snapshots.shadow("proj1").unwrap();
        git_ok(
            &shadow,
            &[
                &format!("--git-dir={}", shadow.display()),
                "rev-parse",
                "--verify",
                "-q",
                refname,
            ],
        )
    }

    fn env(&self, key: &str) -> String {
        self.tc
            .env
            .iter()
            .find(|(k, _)| k == key)
            .unwrap()
            .1
            .clone()
    }

    /// Build refs in the shadow repo.
    fn shadow_refs(&self) -> Vec<String> {
        let shadow = self.snapshots.shadow("proj1").unwrap();
        if !shadow.exists() {
            return vec![];
        }
        git(
            &shadow,
            &[
                &format!("--git-dir={}", shadow.display()),
                "for-each-ref",
                "--format=%(refname)",
                "refs/studio/builds/",
            ],
        )
        .lines()
        .map(str::to_owned)
        .collect()
    }

    fn project_has(&self, refname: &str) -> bool {
        git_ok(&self.dir(), &["rev-parse", "--verify", "-q", refname])
    }

    /// A release row with files on disk, for retention and delete tests.
    fn seed_release(&self, version: &str, created: &str, activated: Option<&str>) -> ReleaseRow {
        let l = self.layout();
        fs::create_dir_all(l.release_dir(version)).unwrap();
        fs::write(l.release_appimage(version), "x").unwrap();
        let row = ReleaseRow {
            id: format!("rel-{version}"),
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
            snapshot_ref: None,
            smoke: Smoke::Passed,
            smoke_detail: None,
            notes: None,
            created_at: created.into(),
            last_activated_at: activated.map(str::to_owned),
            files_removed_at: None,
            schema_version: None,
        };
        self.db.insert_release(&row).unwrap();
        row
    }

    fn activate(&self, version: &str) {
        let l = self.layout();
        fs::create_dir_all(l.root()).unwrap();
        let _ = fs::remove_file(l.current_link());
        std::os::unix::fs::symlink(AppLayout::current_target(version), l.current_link()).unwrap();
    }

    fn files_of(&self, version: &str) -> bool {
        self.layout().release_appimage(version).exists()
    }
}

fn phases(events: &mut broadcast::Receiver<BuildEvent>) -> Vec<BuildPhase> {
    let mut out: Vec<BuildPhase> = Vec::new();
    while let Ok(e) = events.try_recv() {
        if let BuildEvent::Status(s) = e
            && let Some(p) = s.phase
            && out.last() != Some(&p)
        {
            out.push(p);
        }
    }
    out
}

fn pid_gone(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid, 0) != 0 }
}

async fn assert_pid_gone(pidfile: &Path, what: &str) {
    for _ in 0..200 {
        if let Ok(t) = fs::read_to_string(pidfile)
            && let Ok(pid) = t.trim().parse::<i32>()
            && pid_gone(pid)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("{what}: the child process survived");
}

// -------------------------------------------------------------- refusals

#[tokio::test]
async fn refuses_a_project_without_an_appimage_builder() {
    let fx = fx_with(
        TemplateId::Existing,
        CONF,
        Arc::new(TauriAppImageAdapter),
        &[],
    );
    let e = fx.start("1.0.0").unwrap_err();
    assert!(matches!(e, BuildError::Unsupported));
    assert_eq!(e.to_string(), "No AppImage builder configured");
    assert!(fx.db.list_jobs_by_project("proj1").unwrap().is_empty());
    assert!(fx.calls().is_empty());
}

#[tokio::test]
async fn a_second_build_is_refused_and_a_finished_one_frees_the_slot() {
    let fx = fx(&[("FAKE_BUILD", "hang")]);
    let first = fx.start("1.0.0").unwrap();
    assert!(matches!(fx.start("1.0.1"), Err(BuildError::Busy)));
    assert_eq!(fx.db.list_jobs_by_project("proj1").unwrap().len(), 1);
    fx.svc.stop("proj1").await;
    assert_eq!(fx.status().job_id, first.job_id);
    // Free again after the stop.
    assert!(fx.sup.slots().job_id("proj1", JobKind::Build).is_none());
    assert!(fx.start("1.0.1").is_ok());
    fx.svc.stop("proj1").await;
}

#[tokio::test]
async fn refuses_while_a_make_changes_run_is_live_but_not_a_plan_run() {
    let fx = fx(&[]);
    let slots = fx.sup.slots();
    let session = |job: &str, mode| SessionRow {
        id: format!("s-{job}"),
        job_id: job.into(),
        project_id: "proj1".into(),
        profile_id: None,
        mode,
        prompt: "p".into(),
        provider_session_id: None,
        resumed_from: None,
        log_path: "/dev/null".into(),
        before_ref: None,
        after_ref: None,
        checkpoint_ref: None,
        created_at: String::new(),
    };
    // A running Make-changes job in the database.
    fx.db
        .insert_job("agent1", "proj1", JobState::Running)
        .unwrap();
    fx.db
        .insert_session(&session("agent1", SessionMode::Changes))
        .unwrap();
    assert!(matches!(fx.start("1.0.0"), Err(BuildError::AgentRunning)));

    // A run that claimed its slot but has no session yet may be a Make-changes run.
    fx.db
        .transition_job(
            "agent1",
            &[JobState::Running],
            JobState::Succeeded,
            &JobUpdate::default(),
        )
        .unwrap();
    assert!(slots.claim("proj1", JobKind::Agent, "agent2"));
    assert!(matches!(fx.start("1.0.0"), Err(BuildError::AgentRunning)));

    // A plan run only reads.
    fx.db
        .insert_job("agent2", "proj1", JobState::Running)
        .unwrap();
    fx.db
        .insert_session(&session("agent2", SessionMode::Plan))
        .unwrap();
    assert!(fx.start("1.0.0").is_ok());
    // No leftovers from the refusals: one build job (plus the two agent jobs).
    assert_eq!(fx.db.list_jobs_of_kind("proj1", "build").unwrap().len(), 1);
    fx.finished().await;
}

#[tokio::test]
async fn refuses_an_invalid_version_without_leaving_anything_behind() {
    let fx = fx(&[]);
    for v in [
        "",
        "1",
        "1.2",
        "01.2.3",
        "v1.2.3",
        "1.2.3+build.5",
        "1.2.3-",
        "../1.2.3",
        "1.2.3/../../x",
        "1.2.3 ",
        "1.2.3-é",
    ] {
        let e = fx.start(v).unwrap_err();
        assert!(matches!(e, BuildError::InvalidVersion(_)), "{v:?}");
        assert!(e.to_string().contains("MAJOR.MINOR.PATCH"));
    }
    assert!(fx.db.list_jobs_by_project("proj1").unwrap().is_empty());
    assert!(fx.sup.slots().job_id("proj1", JobKind::Build).is_none());
    assert!(fx.calls().is_empty());
    assert!(fx.start("1.2.3-rc.1").is_ok());
    fx.finished().await;
}

#[tokio::test]
async fn refuses_a_version_that_has_a_kept_release_but_not_a_removed_one() {
    let fx = fx(&[]);
    let kept = fx.seed_release("1.0.0", "2026-01-01T00:00:00.000Z", None);
    let e = fx.start("1.0.0").unwrap_err();
    assert!(matches!(e, BuildError::VersionExists(ref v) if v == "1.0.0"));
    assert!(fx.sup.slots().job_id("proj1", JobKind::Build).is_none());
    assert!(fx.db.list_jobs_by_project("proj1").unwrap().is_empty());

    fx.db.mark_release_files_removed(&kept.id).unwrap();
    assert!(fx.start("1.0.0").is_ok());
    fx.finished().await;
}

// ------------------------------------------------------------ happy path

#[tokio::test]
async fn a_build_stores_a_verified_release_and_records_everything() {
    let fx = fx(&[]);
    let mut events = fx.svc.subscribe();
    let started = fx
        .svc
        .start(&fx.project, "1.0.0".into(), Some("  First one \n".into()))
        .unwrap();
    assert_eq!((started.state, started.live), (JobState::Running, true));
    let s = fx.finished().await;
    assert_eq!(s.state, JobState::Succeeded, "{:?}", s.error_message);
    assert_eq!(s.version.as_deref(), Some("1.0.0"));
    assert!(s.error_kind.is_none() && s.has_log);

    let rows = fx.releases();
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(s.release_id.as_deref(), Some(r.id.as_str()));
    assert_eq!(r.version, "1.0.0");
    assert_eq!(r.job_id.as_deref(), Some(s.job_id.as_str()));
    assert_eq!(r.smoke, Smoke::Passed);
    assert_eq!(r.architecture, std::env::consts::ARCH);
    assert_eq!(r.build_profile, "local");
    assert_eq!(
        (r.product_name.as_str(), r.main_binary_name.as_str()),
        ("My App", "My App")
    );
    assert_eq!(r.notes.as_deref(), Some("First one"));
    assert!(r.created_at.ends_with('Z') && r.created_at.contains('T'));
    assert!(r.files_removed_at.is_none() && r.last_activated_at.is_none());

    let l = fx.layout();
    let stored = l.release_appimage("1.0.0");
    assert_eq!(r.sha256, sha256_file(&stored).unwrap());
    assert_eq!(r.size_bytes, fs::metadata(&stored).unwrap().len() as i64);
    assert_eq!(
        fs::metadata(&stored).unwrap().permissions().mode() & 0o111,
        0o111
    );
    assert_eq!(fs::read_to_string(l.release_icon("1.0.0")).unwrap(), "png");
    // Nothing the installer owns was touched or created.
    assert!(!l.current_link().exists() && !l.icon().exists());
    assert!(
        fs::read_dir(l.release_dir("1.0.0"))
            .unwrap()
            .flatten()
            .all(|e| !e.file_name().to_string_lossy().ends_with(".tmp"))
    );

    // The adapter's commands and environment.
    let calls = fx.calls();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert!(calls[0].starts_with("install"));
    assert_eq!(
        calls[1],
        r#"exec tauri build --bundles appimage --config {"version":"1.0.0"} | NO_STRIP=true"#
    );
    assert!(fx.log().iter().any(|l| l.contains("Bundling app_1.0.0")));
    assert_eq!(
        phases(&mut events),
        [
            BuildPhase::Preflight,
            BuildPhase::Installing,
            BuildPhase::Compiling,
            BuildPhase::Packaging,
            BuildPhase::Storing,
            BuildPhase::Testing,
        ]
    );
}

#[tokio::test]
async fn output_reaches_subscribers_in_batches_and_the_log_survives_a_restart() {
    let fx = fx(&[]);
    let mut events = fx.svc.subscribe();
    let s = fx.build("1.0.0").await;
    let mut batched = Vec::new();
    while let Ok(e) = events.try_recv() {
        if let BuildEvent::Log { lines, .. } = e {
            batched.extend(lines);
        }
    }
    assert!(batched.iter().any(|l| l.contains("Compiling app")));
    // After a restart the status and the log come from the job row, the release row and the
    // log file.
    let svc2 = BuildService::new(BuildDeps {
        db: fx.db.clone(),
        slots: Arc::default(),
        toolchain: fx.tc.clone(),
        adapter: Arc::new(TauriAppImageAdapter),
        snapshots: fx.snapshots.clone(),
        checkpoints: Arc::new(CheckpointStore {
            program: "git".into(),
            env: vec![],
        }),
        paths: fx.paths.clone(),
        smoke_after: SMOKE_AFTER,
    });
    let again = svc2.status("proj1").unwrap().unwrap();
    assert_eq!(again.job_id, s.job_id);
    assert_eq!(again.state, JobState::Succeeded);
    assert_eq!(again.version.as_deref(), Some("1.0.0"));
    assert!(again.release_id.is_some() && !again.live && again.has_log);
    let tail = svc2.log_tail("proj1", None).unwrap().unwrap();
    assert!(tail.lines.iter().any(|l| l.contains("Bundling")));
    // Another project's job id reads nothing.
    assert!(svc2.log_tail("other", Some(&s.job_id)).unwrap().is_none());
}

// -------------------------------------------------------------- failures

/// What a failed build must leave alone (M4: it never touches installed files).
fn seed_installed(fx: &Fx) -> (PathBuf, PathBuf) {
    fx.seed_release("0.9.0", "2026-01-01T00:00:00.000Z", None);
    fx.activate("0.9.0");
    let l = fx.layout();
    fs::write(l.icon(), "launcher icon").unwrap();
    (l.current_link(), l.icon())
}

fn assert_untouched(fx: &Fx) {
    let l = fx.layout();
    assert_eq!(l.active_version().as_deref(), Some("0.9.0"));
    assert_eq!(fs::read_to_string(l.icon()).unwrap(), "launcher icon");
    assert_eq!(
        fs::read_to_string(l.release_appimage("0.9.0")).unwrap(),
        "x"
    );
    assert_eq!(fx.releases().len(), 1, "no release row for a failed build");
    assert!(
        !l.release_dir("1.0.0").exists(),
        "no leftover release folder"
    );
}

async fn failed_with(fx: &Fx, kind: &str) -> BuildStatus {
    seed_installed(fx);
    let s = fx.build("1.0.0").await;
    assert_eq!(s.state, JobState::Failed, "{s:?}");
    assert_eq!(s.error_kind.as_deref(), Some(kind), "{s:?}");
    assert!(s.release_id.is_none());
    let job = fx.job_row(&s.job_id);
    assert_eq!(job.error_kind.as_deref(), Some(kind));
    assert_untouched(fx);
    s
}

#[tokio::test]
async fn a_missing_tool_fails_preflight_with_the_doctors_hint_and_runs_nothing() {
    for (tool, hint) in [
        ("xdg-open", "Build needs xdg-open: sudo pacman -S xdg-utils"),
        (
            "libfuse2",
            "Build needs libfuse2 (AppImage): sudo pacman -S fuse2",
        ),
        (
            "webkit2gtk",
            "Build needs WebKitGTK 4.1: sudo pacman -S webkit2gtk-4.1",
        ),
        ("pnpm", "Build needs pnpm: sudo pacman -S pnpm"),
        ("node", "Build needs Node.js: sudo pacman -S nodejs"),
    ] {
        let fx = fx(&[]);
        *fx.tc.report.lock().unwrap() = report(now_secs(), &[tool]);
        let s = failed_with(&fx, "preflight").await;
        assert_eq!(s.error_message.as_deref(), Some(hint));
        assert!(fx.calls().is_empty());
        // The source snapshot taken first is gone again.
        assert!(fx.shadow_refs().is_empty());
    }
}

#[tokio::test]
async fn an_old_doctor_report_is_refreshed_before_the_build() {
    let fx = fx(&[]);
    *fx.tc.report.lock().unwrap() = report(now_secs() - 11 * 60, &[]);
    let s = fx.build("1.0.0").await;
    assert_eq!(s.state, JobState::Succeeded);
    assert_eq!(fx.tc.forced.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_failed_install_stops_before_tauri_build() {
    let fx = fx(&[("FAKE_INSTALL", "fail")]);
    let s = failed_with(&fx, "installFailed").await;
    assert_eq!(
        s.error_message.as_deref(),
        Some("Installing dependencies failed")
    );
    assert_eq!(fx.calls().len(), 1);
}

#[tokio::test]
async fn a_failed_build_reports_the_first_error_line() {
    let fx = fx(&[("FAKE_BUILD", "fail")]);
    let s = failed_with(&fx, "buildFailed").await;
    assert_eq!(
        s.error_message.as_deref(),
        Some("error[E0432]: unresolved import foo")
    );
    assert_eq!(fx.job_row(&s.job_id).exit_code, Some(101));
}

#[tokio::test]
async fn unrecognised_build_output_falls_back_to_the_exit_code() {
    let fx = fx(&[("FAKE_BUILD", "plain")]);
    let s = failed_with(&fx, "buildFailed").await;
    assert_eq!(
        s.error_message.as_deref(),
        Some("The build stopped with exit code 3.")
    );
    assert_eq!(fx.job_row(&s.job_id).exit_code, Some(3));
}

#[tokio::test]
async fn no_artifact_cases() {
    let fx1 = fx(&[("FAKE_BUILD", "none")]);
    let s = failed_with(&fx1, "noArtifact").await;
    assert_eq!(s.error_message.as_deref(), Some("No AppImage was produced"));

    let fx2 = fx(&[("FAKE_BUILD", "two")]);
    let s = failed_with(&fx2, "noArtifact").await;
    assert!(
        s.error_message
            .unwrap()
            .starts_with("No AppImage was produced")
    );

    // bundle.active is false: refused with that reason before any command runs.
    let fx3 = fx_with(
        TemplateId::TauriVue,
        r#"{"productName":"My App","bundle":{"active":false}}"#,
        Arc::new(TauriAppImageAdapter),
        &[],
    );
    let s = failed_with(&fx3, "noArtifact").await;
    assert!(s.error_message.unwrap().contains("bundle.active"));
    assert!(fx3.calls().is_empty());
}

#[tokio::test]
async fn a_stale_appimage_is_not_an_artifact() {
    let fx = fx(&[("FAKE_BUILD", "none")]);
    let b = fx.dir().join("src-tauri/target/release/bundle/appimage");
    fs::create_dir_all(&b).unwrap();
    let old = b.join("old.AppImage");
    fs::copy(fx.env("FAKE_APPIMAGE"), &old).unwrap();
    fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(3600))
        .unwrap();
    failed_with(&fx, "noArtifact").await;
}

#[tokio::test]
async fn a_file_that_is_not_an_appimage_fails_verification() {
    let fx1 = fx(&[("FAKE_BUILD", "junk")]);
    let s = failed_with(&fx1, "verifyFailed").await;
    assert!(s.error_message.unwrap().contains("ELF"));

    let fx2 = fx(&[("FAKE_BUILD", "empty")]);
    let s = failed_with(&fx2, "verifyFailed").await;
    assert_eq!(s.error_message.as_deref(), Some("The AppImage is empty"));
}

#[tokio::test]
async fn a_store_failure_is_reported_and_leaves_no_partial_file() {
    let fx = fx(&[]);
    seed_installed(&fx);
    // A file where the release folder must go.
    let blocker = fx.layout().release_dir("1.0.0");
    fs::write(&blocker, "in the way").unwrap();
    let s = fx.build("1.0.0").await;
    assert_eq!(s.error_kind.as_deref(), Some("storeFailed"), "{s:?}");
    assert_eq!(fs::read_to_string(&blocker).unwrap(), "in the way");
    let l = fx.layout();
    assert_eq!(l.active_version().as_deref(), Some("0.9.0"));
    assert_eq!(fx.releases().len(), 1);
}

#[tokio::test]
async fn stop_cancels_the_build_and_kills_its_whole_group() {
    let fx = fx(&[("FAKE_BUILD", "hang")]);
    seed_installed(&fx);
    let job = fx.start("1.0.0").unwrap().job_id;
    fx.wait("compiling", |s| s.phase == Some(BuildPhase::Compiling))
        .await;
    for _ in 0..100 {
        if fx.tmp.path().join("pid").exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    fx.svc.stop("proj1").await;
    let s = fx.status();
    assert_eq!((s.state, s.live), (JobState::Cancelled, false));
    assert_eq!(s.error_kind.as_deref(), Some("cancelled"));
    assert_eq!(fx.job_row(&job).state, JobState::Cancelled);
    assert_pid_gone(&fx.tmp.path().join("pid"), "Stop").await;
    assert_untouched(&fx);
}

#[tokio::test]
async fn shutdown_interrupts_a_running_build() {
    let fx = fx(&[("FAKE_BUILD", "hang")]);
    let job = fx.start("1.0.0").unwrap().job_id;
    fx.wait("compiling", |s| s.phase == Some(BuildPhase::Compiling))
        .await;
    tokio::time::timeout(Duration::from_secs(10), fx.sup.shutdown())
        .await
        .unwrap();
    assert_eq!(fx.job_row(&job).state, JobState::Interrupted);
    assert!(!fx.status().live);
    assert!(fx.releases().is_empty());
}

// --------------------------------------------------------- source record

#[tokio::test]
async fn a_git_project_records_head_dirty_flag_and_a_ref_in_its_own_repo() {
    let fx = fx(&[]);
    let head = fx.init_git();
    // Uncommitted edits and a new file: the build includes them and says so.
    fs::write(fx.dir().join("README.md"), "edited\n").unwrap();
    fs::write(fx.dir().join("new.txt"), "new\n").unwrap();
    let s = fx.build("1.0.0").await;
    assert_eq!(s.state, JobState::Succeeded, "{:?}", s.error_message);
    let r = &fx.releases()[0];
    assert_eq!(r.source_commit.as_deref(), Some(head.as_str()));
    assert!(r.dirty_source);
    let refname = format!("refs/studio/builds/{}", r.id);
    assert_eq!(r.snapshot_ref.as_deref(), Some(refname.as_str()));
    assert!(fx.project_has(&refname), "ref in the project repo");
    assert!(fx.shadow_has(&refname), "ref in the shadow repo");
    // The snapshot holds the dirty tree (the edit and the new file).
    assert_eq!(
        git(&fx.dir(), &["show", &format!("{refname}:README.md")]),
        "edited"
    );
    assert_eq!(
        git(&fx.dir(), &["show", &format!("{refname}:new.txt")]),
        "new"
    );
    // The user's repo is otherwise as it was: HEAD, no extra files in .git/info.
    assert_eq!(git(&fx.dir(), &["rev-parse", "HEAD"]), head);
    assert!(!fx.dir().join(".git/info/attributes").exists());
    let ex = fs::read_to_string(fx.dir().join(".git/info/exclude")).unwrap_or_default();
    assert!(!ex.contains("node_modules/"), "{ex}");
}

#[tokio::test]
async fn a_clean_commit_is_not_dirty() {
    let fx = fx(&[]);
    // The build output dir is untracked; ignore it so the tree is clean.
    fs::write(fx.dir().join(".gitignore"), "src-tauri/target/\n").unwrap();
    let head = fx.init_git();
    let s = fx.build("1.0.0").await;
    assert_eq!(s.state, JobState::Succeeded, "{:?}", s.error_message);
    let r = &fx.releases()[0];
    assert_eq!(r.source_commit.as_deref(), Some(head.as_str()));
    assert!(!r.dirty_source);
}

#[tokio::test]
async fn an_unborn_repo_has_no_commit() {
    let fx = fx(&[]);
    git(&fx.dir(), &["init", "-q", "-b", "main"]);
    let s = fx.build("1.0.0").await;
    assert_eq!(s.state, JobState::Succeeded, "{:?}", s.error_message);
    assert_eq!(fx.releases()[0].source_commit, None);
}

#[tokio::test]
async fn a_non_git_project_has_no_commit_and_only_a_shadow_ref() {
    let fx = fx(&[]);
    let s = fx.build("1.0.0").await;
    assert_eq!(s.state, JobState::Succeeded, "{:?}", s.error_message);
    let r = &fx.releases()[0];
    assert_eq!(r.source_commit, None);
    assert!(!r.dirty_source);
    let refname = format!("refs/studio/builds/{}", r.id);
    assert_eq!(r.snapshot_ref.as_deref(), Some(refname.as_str()));
    assert!(fx.shadow_has(&refname));
    assert!(!fx.dir().join(".git").exists(), "no repository was created");
}

#[tokio::test]
async fn build_refs_survive_run_pruning_and_a_failed_build_removes_its_ref() {
    let fx = fx(&[]);
    fx.init_git();
    let s = fx.build("1.0.0").await;
    assert_eq!(s.state, JobState::Succeeded);
    let refname = fx.releases()[0].snapshot_ref.clone().unwrap();
    // The 20-run pruning looks only at refs/studio/runs/.
    fx.snapshots.prune("proj1", 0).unwrap();
    assert!(fx.shadow_has(&refname));
    assert!(fx.project_has(&refname));

    // A failing build leaves no ref behind in either repository.
    let before: Vec<String> = git(
        &fx.dir(),
        &["for-each-ref", "--format=%(refname)", "refs/studio/builds/"],
    )
    .lines()
    .map(str::to_owned)
    .collect();
    assert_eq!(before, std::slice::from_ref(&refname));
    *fx.tc.report.lock().unwrap() = report(now_secs(), &["libfuse2"]);
    let s = fx.build("1.0.1").await;
    assert_eq!(s.error_kind.as_deref(), Some("preflight"));
    let after = git(
        &fx.dir(),
        &["for-each-ref", "--format=%(refname)", "refs/studio/builds/"],
    );
    assert_eq!(after, refname);
}

// ----------------------------------------------------------- smoke test

type SmokeResult = Option<(Smoke, Option<String>)>;

/// Runs the smoke test directly on `app`; returns its result and the build log it wrote.
async fn smoke_path(
    fx: &Fx,
    app: &Path,
    cancel: Option<watch::Receiver<bool>>,
) -> (SmokeResult, Vec<String>) {
    let status = fx.status_for_test();
    let run = Run {
        job_id: "j".into(),
        project_id: "proj1".into(),
        status: Mutex::new(status),
        parser: Mutex::new(PhaseParser::new(PreviewPhase::Starting)),
        sink: Mutex::new(LogSink::new(
            &fx.tmp.path().join("smoke.log"),
            &settings::load_preview_limits(&fx.db),
        )),
    };
    let (_tx, rx) = watch::channel(false);
    let mut rx = cancel.unwrap_or(rx);
    let out = fx
        .svc
        .inner
        .smoke(&run, &fx.project, app, &fx.tmp.path().join("work"), &mut rx)
        .await;
    let log = lock(&run.sink).tail().lines;
    (out, log)
}

impl Fx {
    fn status_for_test(&self) -> BuildStatus {
        BuildStatus {
            job_id: "j".into(),
            project_id: "proj1".into(),
            version: None,
            state: JobState::Running,
            phase: None,
            started_at: None,
            completed_at: None,
            error_kind: None,
            error_message: None,
            release_id: None,
            live: true,
            has_log: false,
        }
    }

    /// Smoke-tests a stand-in script (the fixtures "are" AppImages).
    async fn smoke(&self, body: &str) -> (SmokeResult, Vec<String>) {
        fs::create_dir_all(self.tmp.path().join("work")).unwrap();
        let app = self.tmp.path().join("standin.sh");
        script(&app, body);
        smoke_path(self, &app, None).await
    }

    fn pidfile(&self) -> PathBuf {
        self.tmp.path().join("pid")
    }
}

#[tokio::test]
async fn smoke_passes_when_the_app_is_still_running_and_the_group_is_gone_after() {
    let fx = fx(&[]);
    let (r, _) = fx
        .smoke(r#"echo started; sleep 30 & echo $! > "$FAKE_PIDFILE"; wait"#)
        .await;
    assert_eq!(r, Some((Smoke::Passed, None)));
    assert_pid_gone(&fx.pidfile(), "test launch").await;
}

#[tokio::test]
async fn smoke_passes_on_a_clean_early_exit() {
    let fx = fx(&[]);
    let (r, log) = fx.smoke("echo bye; exit 0").await;
    assert_eq!(r, Some((Smoke::Passed, None)));
    assert_eq!(log, ["[test launch] bye"]);
}

#[tokio::test]
async fn smoke_fails_with_the_panic_line_and_the_exit_code_is_not_needed() {
    let fx = fx(&[]);
    let (r, log) = fx
        .smoke("echo \"thread 'main' panicked at src/main.rs:3:5:\" >&2; echo boom >&2; exit 101")
        .await;
    let line = "thread 'main' panicked at src/main.rs:3:5:";
    assert_eq!(r, Some((Smoke::Failed, Some(line.into()))));
    assert!(log.contains(&format!("[test launch] {line}")), "{log:?}");
    assert!(log.contains(&"[test launch] boom".to_owned()));
}

#[tokio::test]
async fn smoke_fails_on_a_crash_line_even_when_the_exit_code_is_zero_or_the_app_stays_up() {
    let fx = fx(&[]);
    let (r, _) = fx
        .smoke("echo '** (app:1): ERROR **: no display'; exit 0")
        .await;
    assert_eq!(
        r,
        Some((
            Smoke::Failed,
            Some("** (app:1): ERROR **: no display".into())
        ))
    );
    let (r, _) = fx
        .smoke(
            r#"echo "thread 'x' panicked at a:1:1:"; sleep 30 & echo $! > "$FAKE_PIDFILE"; wait"#,
        )
        .await;
    assert_eq!(r.unwrap().0, Smoke::Failed);
    assert_pid_gone(&fx.pidfile(), "crashed app").await;
}

#[tokio::test]
async fn smoke_fails_on_a_non_zero_exit() {
    let fx = fx(&[]);
    let (r, _) = fx.smoke("exit 3").await;
    assert_eq!(r, Some((Smoke::Failed, Some("exited with code 3".into()))));
}

#[tokio::test]
async fn smoke_is_unverified_when_it_cannot_run() {
    let fx = fx(&[]);
    fs::create_dir_all(fx.tmp.path().join("work")).unwrap();
    let (r, _) = smoke_path(&fx, &fx.tmp.path().join("missing.AppImage"), None).await;
    let (smoke, detail) = r.unwrap();
    assert_eq!(smoke, Smoke::Unverified);
    assert!(detail.unwrap().contains("Could not start the test launch"));

    // Without a display there is nothing to test against; the app is not even started.
    let fx = fx_with_no_display();
    let (r, log) = fx.smoke(r#"echo ran >> "$FAKE_PIDFILE""#).await;
    let (smoke, detail) = r.unwrap();
    assert_eq!(smoke, Smoke::Unverified);
    assert!(detail.unwrap().contains("WAYLAND_DISPLAY and DISPLAY"));
    assert!(!fx.pidfile().exists());
    assert!(log[0].starts_with("[test launch] No display"));
}

fn fx_with_no_display() -> Fx {
    fx(&[("DISPLAY", ""), ("WAYLAND_DISPLAY", "")])
}

#[tokio::test]
async fn smoke_runs_in_the_release_folder_with_the_appimage_launcher_disabled() {
    let fx = fx(&[]);
    let (r, log) = fx
        .smoke(r#"echo "launcher=$APPIMAGELAUNCHER_DISABLE"; pwd"#)
        .await;
    assert_eq!(r.unwrap().0, Smoke::Passed);
    assert_eq!(log[0], "[test launch] launcher=1");
    assert_eq!(
        log[1],
        format!("[test launch] {}", fx.tmp.path().join("work").display())
    );
}

#[tokio::test]
async fn stopping_the_build_during_the_test_launch_kills_the_app() {
    let fx = fx(&[]);
    fs::create_dir_all(fx.tmp.path().join("work")).unwrap();
    let app = fx.tmp.path().join("standin.sh");
    script(&app, r#"sleep 30 & echo $! > "$FAKE_PIDFILE"; wait"#);
    let (tx, rx) = watch::channel(false);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        let _ = tx.send(true);
    });
    let (r, _) = smoke_path(&fx, &app, Some(rx)).await;
    assert_eq!(r, None);
    assert_pid_gone(&fx.pidfile(), "stopped test launch").await;
}

#[tokio::test]
async fn a_failing_test_launch_still_stores_the_release_and_records_the_failure() {
    let fx = fx(&[("FAKE_BUILD", "badapp")]);
    let s = fx.build("1.0.0").await;
    assert_eq!(s.state, JobState::Succeeded, "{:?}", s.error_message);
    let r = &fx.releases()[0];
    assert_eq!(r.smoke, Smoke::Failed);
    assert_eq!(r.smoke_detail.as_deref(), Some("exited with code 1"));
    assert!(fx.files_of("1.0.0"));
}

#[tokio::test]
async fn without_a_display_the_release_is_unverified() {
    let fx = fx_with_no_display();
    let s = fx.build("1.0.0").await;
    assert_eq!(s.state, JobState::Succeeded, "{:?}", s.error_message);
    let r = &fx.releases()[0];
    assert_eq!(r.smoke, Smoke::Unverified);
    assert!(r.smoke_detail.as_deref().unwrap().contains("No display"));
}

// ------------------------------------------------------------- retention

fn row(version: &str, created: &str, activated: Option<&str>) -> ReleaseRow {
    ReleaseRow {
        id: format!("id-{version}"),
        project_id: "p".into(),
        job_id: None,
        version: version.into(),
        sha256: String::new(),
        size_bytes: 0,
        architecture: "x86_64".into(),
        build_profile: "local".into(),
        product_name: "A".into(),
        main_binary_name: "a".into(),
        source_commit: None,
        dirty_source: false,
        snapshot_ref: None,
        smoke: Smoke::Passed,
        smoke_detail: None,
        notes: None,
        created_at: created.into(),
        last_activated_at: activated.map(str::to_owned),
        files_removed_at: None,
        schema_version: None,
    }
}

/// Ten releases, newest first: 1.0.9 ... 1.0.0. 1.0.0 is active (the oldest), 1.0.1 was
/// activated before it, 1.0.3 earlier still.
fn ten() -> Vec<ReleaseRow> {
    (0..10)
        .rev()
        .map(|i| {
            let activated = match i {
                0 => Some("2026-04-01T00:00:00.000Z"),
                1 => Some("2026-03-01T00:00:00.000Z"),
                3 => Some("2026-02-01T00:00:00.000Z"),
                _ => None,
            };
            row(
                &format!("1.0.{i}"),
                &format!("2026-01-{:02}T00:00:00.000Z", i + 1),
                activated,
            )
        })
        .collect()
}

#[test]
fn retention_keeps_active_previous_and_the_five_newest() {
    let rows = ten();
    let gone: Vec<&str> = retention_victims(&rows, Some("1.0.0"))
        .iter()
        .map(|r| r.version.as_str())
        .collect();
    assert_eq!(gone, ["1.0.4", "1.0.3", "1.0.2"]);
}

#[test]
fn retention_never_removes_the_active_or_the_previous_release_however_old() {
    let rows = ten();
    // Active is the oldest and the previous one is the second oldest: both stay.
    let gone = retention_victims(&rows, Some("1.0.0"));
    assert!(
        gone.iter()
            .all(|r| r.version != "1.0.0" && r.version != "1.0.1")
    );
    // With nothing active, "previous" is simply the most recently activated one.
    let gone: Vec<&str> = retention_victims(&rows, None)
        .iter()
        .map(|r| r.version.as_str())
        .collect();
    assert_eq!(gone, ["1.0.4", "1.0.3", "1.0.2", "1.0.1"]);
    // Few releases: nothing goes.
    assert!(retention_victims(&rows[..5], None).is_empty());
}

#[tokio::test]
async fn a_successful_build_applies_retention_to_files_and_rows_but_not_the_active_release() {
    let fx = fx(&[]);
    for i in 0..6 {
        fx.seed_release(
            &format!("0.1.{i}"),
            &format!("2026-01-{:02}T00:00:00.000Z", i + 1),
            None,
        );
    }
    fx.activate("0.1.0");
    let s = fx.build("1.0.0").await;
    assert_eq!(s.state, JobState::Succeeded, "{:?}", s.error_message);
    // Kept: active 0.1.0, and the five newest (1.0.0, 0.1.5, 0.1.4, 0.1.3, 0.1.2).
    assert!(!fx.files_of("0.1.1"));
    for v in ["0.1.0", "0.1.2", "0.1.3", "0.1.4", "0.1.5", "1.0.0"] {
        assert!(fx.files_of(v), "{v}");
    }
    let rows = fx.releases();
    assert_eq!(rows.len(), 7, "rows stay");
    let removed: Vec<&str> = rows
        .iter()
        .filter(|r| r.files_removed_at.is_some())
        .map(|r| r.version.as_str())
        .collect();
    assert_eq!(removed, ["0.1.1"]);
    assert_eq!(fx.layout().active_version().as_deref(), Some("0.1.0"));
    // A removed version can be built again (M4-D8).
    assert!(fx.start("0.1.1").is_ok());
    fx.finished().await;
}

// ---------------------------------------------------------- next version

#[tokio::test]
async fn next_version_is_the_next_patch_above_every_release_ever_built() {
    let fx = fx(&[]);
    // Nothing built: the project's configured version.
    assert_eq!(fx.svc.next_version(&fx.project).unwrap(), "0.2.0");
    let a = fx.seed_release("0.2.0", "2026-01-01T00:00:00.000Z", None);
    assert_eq!(fx.svc.next_version(&fx.project).unwrap(), "0.2.1");
    fx.seed_release("0.3.5", "2026-01-02T00:00:00.000Z", None);
    assert_eq!(fx.svc.next_version(&fx.project).unwrap(), "0.3.6");
    // Removed releases still count.
    let b = fx.db.get_release("rel-0.3.5").unwrap().unwrap();
    fx.db.mark_release_files_removed(&b.id).unwrap();
    assert_eq!(fx.svc.next_version(&fx.project).unwrap(), "0.3.6");
    let _ = a;
    // A configured version above all of them wins.
    fs::write(
        fx.dir().join("src-tauri/tauri.conf.json"),
        r#"{"productName":"A","version":"2.0.0","bundle":{"active":true}}"#,
    )
    .unwrap();
    assert_eq!(fx.svc.next_version(&fx.project).unwrap(), "2.0.0");
}

#[tokio::test]
async fn next_version_defaults_to_0_1_0() {
    let fx = fx_with(
        TemplateId::TauriVue,
        r#"{"productName":"A","bundle":{"active":true}}"#,
        Arc::new(TauriAppImageAdapter),
        &[],
    );
    assert_eq!(fx.svc.next_version(&fx.project).unwrap(), "0.1.0");
}

// -------------------------------------------------------- delete release

#[tokio::test]
async fn the_active_release_cannot_be_deleted() {
    let fx = fx(&[]);
    let r = fx.seed_release("1.0.0", "2026-01-01T00:00:00.000Z", None);
    fx.activate("1.0.0");
    let e = fx.svc.delete_release("proj1", &r.id).await.unwrap_err();
    assert!(matches!(e, BuildError::ActiveRelease), "{e}");
    assert!(fx.files_of("1.0.0"));
    assert!(
        fx.db
            .get_release(&r.id)
            .unwrap()
            .unwrap()
            .files_removed_at
            .is_none()
    );
}

#[tokio::test]
async fn deleting_a_release_removes_its_files_row_flag_and_refs() {
    let fx = fx(&[]);
    fx.init_git();
    let a = fx.build("1.0.0").await;
    assert_eq!(a.state, JobState::Succeeded, "{:?}", a.error_message);
    let b = fx.build("1.0.1").await;
    assert_eq!(b.state, JobState::Succeeded, "{:?}", b.error_message);
    fx.activate("1.0.1");
    let first = fx
        .releases()
        .into_iter()
        .find(|r| r.version == "1.0.0")
        .unwrap();
    let refname = first.snapshot_ref.clone().unwrap();
    assert!(fx.project_has(&refname) && fx.shadow_has(&refname));

    fx.svc.delete_release("proj1", &first.id).await.unwrap();
    assert!(!fx.layout().release_dir("1.0.0").exists());
    assert!(fx.files_of("1.0.1"), "other releases are untouched");
    let row = fx.db.get_release(&first.id).unwrap().unwrap();
    assert!(
        row.files_removed_at.is_some(),
        "the row stays, marked removed"
    );
    assert!(!fx.project_has(&refname) && !fx.shadow_has(&refname));
    // The other release keeps its refs.
    let other = fx
        .releases()
        .into_iter()
        .find(|r| r.version == "1.0.1")
        .unwrap();
    assert!(fx.project_has(other.snapshot_ref.as_deref().unwrap()));
    // Deleting again is harmless; unknown ids and other projects' releases are not found.
    fx.svc.delete_release("proj1", &first.id).await.unwrap();
    assert!(matches!(
        fx.svc.delete_release("proj1", "nope").await,
        Err(BuildError::NotFound)
    ));
    assert!(matches!(
        fx.svc.delete_release("other", &other.id).await,
        Err(BuildError::NotFound)
    ));
}

#[tokio::test]
async fn forget_refs_deletes_the_build_refs_of_the_given_releases() {
    let fx = fx(&[]);
    fx.init_git();
    fx.build("1.0.0").await;
    let r = fx.releases().remove(0);
    let refname = r.snapshot_ref.clone().unwrap();
    let project = fx.db.get_project("proj1").unwrap().unwrap();
    fx.svc.forget_refs(&project, &[r.id]);
    assert!(!fx.project_has(&refname) && !fx.shadow_has(&refname));
    // Missing refs are fine.
    fx.svc.forget_refs(&project, &["not-a-release".to_owned()]);
}

// --------------------------------------------------------------- startup

#[tokio::test]
async fn startup_removes_leftover_temp_files_of_a_killed_build() {
    let fx = fx(&[]);
    let dir = fx.layout().release_dir("1.0.0");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("app.AppImage.tmp"), "half").unwrap();
    fs::write(dir.join("icon.png.tmp"), "half").unwrap();
    fs::write(dir.join("app.AppImage"), "whole").unwrap();
    fx.svc.cleanup_on_startup();
    assert!(!dir.join("app.AppImage.tmp").exists() && !dir.join("icon.png.tmp").exists());
    assert_eq!(
        fs::read_to_string(dir.join("app.AppImage")).unwrap(),
        "whole"
    );
}

#[tokio::test]
async fn a_project_that_never_built_has_no_status_and_no_log() {
    let fx = fx(&[]);
    assert!(fx.svc.status("proj1").unwrap().is_none());
    assert!(fx.svc.log_tail("proj1", None).unwrap().is_none());
    fx.svc.stop("proj1").await; // nothing running is not an error
}

// ------------------------------------------------ app databases (D3, D14, DR-8)

use crate::domain::{DatabaseEntry, ProjectMeta};
use crate::persistence::{BackupReason, BackupRow, DataSet, InstallRow};

impl Fx {
    /// The project gets the `database` entry; with `data`, the app is installed and has
    /// created `<data home>/com.x.a/app.db` (the identifier of `CONF`) from that SQL.
    fn with_database(&self, data: Option<&str>) {
        let mut meta = ProjectMeta::new("a", "a", TemplateId::TauriVue, "com.x.a");
        meta.database = Some(DatabaseEntry {
            engine: "sqlite".into(),
        });
        meta.write(&self.dir()).unwrap();
        if let Some(sql) = data {
            let file = self.installed_db();
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            rusqlite::Connection::open(&file)
                .unwrap()
                .execute_batch(sql)
                .unwrap();
            self.db
                .upsert_install(&InstallRow {
                    project_id: "proj1".into(),
                    desktop_file: "/d".into(),
                    installed_at: "t".into(),
                })
                .unwrap();
        }
    }

    /// The installed app's real data file: `<XDG data home>/com.x.a/app.db`.
    fn installed_db(&self) -> PathBuf {
        self.paths.data_dir.parent().unwrap().join("com.x.a/app.db")
    }

    fn test_launch_dir(&self) -> PathBuf {
        self.paths.data_dir.join(TEST_HOME_DIR)
    }
}

const APP_DATA: &str =
    "CREATE TABLE t (v TEXT); INSERT INTO t VALUES ('real'); PRAGMA user_version = 2;";

#[tokio::test]
async fn the_test_launch_gets_its_own_temp_xdg_folders_and_they_are_deleted_after() {
    // The toolchain env carries "real" XDG folders; the app must not see them.
    let fx = fx(&[
        ("XDG_DATA_HOME", "/real/share"),
        ("XDG_CONFIG_HOME", "/real/config"),
        ("XDG_CACHE_HOME", "/real/cache"),
    ]);
    let (r, log) = fx
        .smoke(
            r#"echo "d=$XDG_DATA_HOME"; echo "c=$XDG_CONFIG_HOME"; echo "k=$XDG_CACHE_HOME"
               touch "$XDG_DATA_HOME/marker" "$XDG_CONFIG_HOME/marker" "$XDG_CACHE_HOME/marker""#,
        )
        .await;
    assert_eq!(r, Some((Smoke::Passed, None)));
    let root = fx.test_launch_dir().join("j");
    let want = |kind: &str, sub: &str| format!("[test launch] {kind}={}", root.join(sub).display());
    assert_eq!(log[0], want("d", "data"));
    assert_eq!(log[1], want("c", "config"));
    assert_eq!(log[2], want("k", "cache"));
    assert!(!root.exists(), "the folders are deleted afterwards");
    assert!(!Path::new("/real/share/marker").exists());
}

#[tokio::test]
async fn a_project_without_the_database_entry_launches_on_empty_data() {
    let fx = fx(&[]);
    // Installed data exists, but the project has no entry: never copied.
    let file = fx.installed_db();
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "x").unwrap();
    let (r, log) = fx
        .smoke(r#"[ -e "$XDG_DATA_HOME/com.x.a/app.db" ] && echo copied || echo empty"#)
        .await;
    assert_eq!(r, Some((Smoke::Passed, None)));
    assert_eq!(log, ["[test launch] empty"]);
}

#[tokio::test]
async fn the_test_launch_opens_a_copy_of_the_installed_data_and_never_the_real_file() {
    let fx = fx(&[]);
    fx.with_database(Some(APP_DATA));
    let real = fx.installed_db();
    let before = fs::read(&real).unwrap();
    let (r, log) = fx
        .smoke(
            r#"head -c 15 "$XDG_DATA_HOME/com.x.a/app.db"; echo
               echo scribble >> "$XDG_DATA_HOME/com.x.a/app.db""#,
        )
        .await;
    assert_eq!(r, Some((Smoke::Passed, None)), "{log:?}");
    assert!(
        log.iter().any(|l| l == "[test launch] SQLite format 3"),
        "{log:?}"
    );
    assert_eq!(
        fs::read(&real).unwrap(),
        before,
        "the real file is untouched"
    );
    assert!(!real.with_file_name("app.db-wal").exists());
    assert!(!fx.test_launch_dir().join("j").exists());
}

#[tokio::test]
async fn a_crash_on_the_copied_data_fails_the_test_launch() {
    let fx = fx(&[]);
    fx.with_database(Some(APP_DATA));
    let (r, _) = fx
        .smoke(r#"[ -e "$XDG_DATA_HOME/com.x.a/app.db" ] && exit 4; exit 0"#)
        .await;
    assert_eq!(r, Some((Smoke::Failed, Some("exited with code 4".into()))));
}

#[tokio::test]
async fn when_the_data_cannot_be_copied_the_launch_runs_on_empty_data_and_is_unverified() {
    let fx = fx(&[]);
    fx.with_database(Some(APP_DATA));
    fs::write(fx.installed_db(), "garbage ".repeat(500)).unwrap();
    let (r, log) = fx
        .smoke(r#"[ -e "$XDG_DATA_HOME/com.x.a/app.db" ] && echo copied || echo empty"#)
        .await;
    let (smoke, detail) = r.unwrap();
    assert_eq!(smoke, Smoke::Unverified);
    let detail = detail.unwrap();
    assert!(
        detail.starts_with("Couldn't copy the installed app's data: ")
            && detail.contains("not a database"),
        "{detail}"
    );
    assert!(log.iter().any(|l| l == "[test launch] empty"), "{log:?}");
    // A real crash stays a failure: Unverified is only "at best".
    let (r, _) = fx.smoke("exit 3").await;
    assert_eq!(r.unwrap().0, Smoke::Failed);
}

#[tokio::test]
async fn startup_cleanup_removes_test_launch_folders_a_crash_left_behind() {
    let fx = fx(&[]);
    let left = fx.test_launch_dir().join("old-job/data");
    fs::create_dir_all(&left).unwrap();
    fx.svc.cleanup_on_startup();
    assert!(!fx.test_launch_dir().exists());
}

#[tokio::test]
async fn a_build_records_the_schema_version_of_the_built_source() {
    let fx = fx(&[]);
    let migrations = fx.dir().join("src-tauri/migrations");
    fs::create_dir_all(&migrations).unwrap();
    for n in [
        "0001_init.sql",
        "0003_scores.sql",
        "0002_x.sql",
        "notes.md",
        "9_bad.sql",
    ] {
        fs::write(migrations.join(n), "").unwrap();
    }
    let s = fx.build("1.0.0").await;
    assert_eq!(s.state, JobState::Succeeded, "{:?}", s.error_message);
    assert_eq!(fx.releases()[0].schema_version, Some(3));
    // No migrations folder: NULL.
    let none = fx_with(
        TemplateId::TauriVue,
        CONF,
        Arc::new(TauriAppImageAdapter),
        &[],
    );
    let s = none.build("1.0.0").await;
    assert_eq!(s.state, JobState::Succeeded, "{:?}", s.error_message);
    assert_eq!(none.releases()[0].schema_version, None);
}

fn release_backup(fx: &Fx, id: &str, release: &str, reason: BackupReason) -> PathBuf {
    let path = fx.tmp.path().join(format!("{id}.db"));
    fs::write(&path, "backup").unwrap();
    fx.db
        .insert_backup(&BackupRow {
            id: id.into(),
            project_id: "proj1".into(),
            which: DataSet::Installed,
            reason,
            release_id: Some(release.into()),
            label: Some("before 1.1.0".into()),
            path: path.display().to_string(),
            size_bytes: 6,
            schema_version: Some(2),
            created_at: "2026-01-01T00:00:00Z".into(),
        })
        .unwrap();
    path
}

#[tokio::test]
async fn deleting_a_release_deletes_its_automatic_backups_and_keeps_manual_ones() {
    let fx = fx(&[]);
    let a = fx.seed_release("1.0.0", "2026-01-01T00:00:00.000Z", None);
    let b = fx.seed_release("1.1.0", "2026-01-02T00:00:00.000Z", None);
    fx.activate("1.1.0");
    let auto_a = release_backup(&fx, "auto-a", &a.id, BackupReason::Release);
    let auto_b = release_backup(&fx, "auto-b", &b.id, BackupReason::Release);
    let manual = release_backup(&fx, "manual-a", &a.id, BackupReason::Manual);
    fx.svc.delete_release("proj1", &a.id).await.unwrap();
    assert!(!auto_a.exists() && fx.db.get_backup("auto-a").unwrap().is_none());
    assert!(auto_b.exists() && fx.db.get_backup("auto-b").unwrap().is_some());
    assert!(manual.exists() && fx.db.get_backup("manual-a").unwrap().is_some());
}

#[tokio::test]
async fn retention_deleting_old_releases_deletes_their_backups() {
    let fx = fx(&[]);
    for i in 0..6 {
        fx.seed_release(
            &format!("1.0.{i}"),
            &format!("2026-01-0{}T00:00:00.000Z", i + 1),
            None,
        );
    }
    fx.activate("1.0.5");
    let oldest = release_backup(&fx, "auto-old", "rel-1.0.0", BackupReason::Release);
    let s = fx.build("2.0.0").await;
    assert_eq!(s.state, JobState::Succeeded, "{:?}", s.error_message);
    assert!(
        fx.releases()
            .iter()
            .any(|r| r.version == "1.0.0" && r.files_removed_at.is_some())
    );
    assert!(!oldest.exists() && fx.db.get_backup("auto-old").unwrap().is_none());
}
