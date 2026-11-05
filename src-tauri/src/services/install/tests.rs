use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::launcher::desktop_entry;
use super::*;
use crate::domain::TemplateId;
use crate::persistence::NewProject;

struct Fx {
    _dir: tempfile::TempDir,
    paths: StudioPaths,
    db: Arc<Db>,
}

impl Fx {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().to_str().unwrap().to_owned();
        let paths = StudioPaths::from_env(|k| match k {
            "HOME" => Some("/home/u".into()),
            "XDG_DATA_HOME" => Some(data.clone()),
            _ => None,
        })
        .unwrap();
        let db = Arc::new(Db::open(&dir.path().join("studio.db")).unwrap());
        for id in ["p1", "p2"] {
            db.insert_project(&NewProject {
                id: id.into(),
                name: id.into(),
                slug: id.into(),
                template_id: TemplateId::TauriVue,
                app_id: format!("com.x.{id}"),
                workspace_path: format!("/p/{id}"),
                pending_op: None,
            })
            .unwrap();
        }
        Self {
            _dir: dir,
            paths,
            db,
        }
    }

    fn installer(&self, gio: &str) -> Installer {
        Installer::new(
            self.paths.clone(),
            self.db.clone(),
            Arc::new(XdgLauncher::new(gio.into())),
        )
    }

    fn layout(&self, p: &str) -> AppLayout {
        AppLayout::new(&self.paths, p)
    }

    /// A stored release with real files. `id` doubles as the content.
    fn release(&self, project: &str, id: &str, version: &str, binary: &str) -> ReleaseRow {
        self.release_with(project, id, version, binary, Smoke::Passed)
    }

    fn release_with(
        &self,
        project: &str,
        id: &str,
        version: &str,
        binary: &str,
        smoke: Smoke,
    ) -> ReleaseRow {
        let l = self.layout(project);
        fs::create_dir_all(l.release_dir(version)).unwrap();
        let file = l.release_appimage(version);
        fs::write(&file, format!("appimage {id}")).unwrap();
        fs::write(l.release_icon(version), format!("icon {id}")).unwrap();
        let row = ReleaseRow {
            id: id.into(),
            project_id: project.into(),
            job_id: None,
            version: version.into(),
            sha256: sha256_file(&file).unwrap(),
            size_bytes: 1,
            architecture: "x86_64".into(),
            build_profile: "local".into(),
            product_name: format!("App {id}"),
            main_binary_name: binary.into(),
            source_commit: None,
            dirty_source: false,
            snapshot_ref: None,
            smoke,
            smoke_detail: None,
            notes: None,
            created_at: format!(
                "2026-01-01T00:00:0{}Z",
                self.db.list_releases(project, true).unwrap().len()
            ),
            last_activated_at: None,
            files_removed_at: None,
            schema_version: None,
        };
        self.db.insert_release(&row).unwrap();
        row
    }

    fn write_foreign(&self, project: &str) -> PathBuf {
        let f = self.layout(project).desktop_file().to_owned();
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        fs::write(&f, "[Desktop Entry]\nName=Someone else\n").unwrap();
        f
    }
}

fn spec<'a>(name: &'a str, icon: Option<&'a Path>) -> EntrySpec<'a> {
    EntrySpec {
        project_id: "pid-1",
        product_name: name,
        main_binary_name: "my-app",
        icon,
    }
}

// ---- desktop entry text

#[test]
fn desktop_entry_plain() {
    let t = desktop_entry(
        Path::new("/d/installed/p/current.AppImage"),
        &spec("My App", Some(Path::new("/d/installed/p/icon.png"))),
    );
    assert_eq!(
        t,
        "[Desktop Entry]\nType=Application\nName=My App\n\
         Exec=env APPIMAGELAUNCHER_DISABLE=1 \"/d/installed/p/current.AppImage\"\n\
         TryExec=/d/installed/p/current.AppImage\nIcon=/d/installed/p/icon.png\n\
         Terminal=false\nCategories=Utility;\nStartupWMClass=my-app\n\
         X-AI-App-Studio-Project=pid-1\n"
    );
}

#[test]
fn desktop_entry_omits_icon_without_one() {
    let t = desktop_entry(Path::new("/a/current.AppImage"), &spec("X", None));
    assert!(!t.contains("Icon="));
}

#[test]
fn desktop_entry_escaping_table() {
    // (path, expected Exec line, expected TryExec line)
    let cases = [
        (
            "/d/sp ace/current.AppImage",
            r#"Exec=env APPIMAGELAUNCHER_DISABLE=1 "/d/sp ace/current.AppImage""#,
            "TryExec=/d/sp ace/current.AppImage",
        ),
        (
            r#"/d/q"x/current.AppImage"#,
            r#"Exec=env APPIMAGELAUNCHER_DISABLE=1 "/d/q\\"x/current.AppImage""#,
            r#"TryExec=/d/q"x/current.AppImage"#,
        ),
        (
            "/d/b`x/current.AppImage",
            r#"Exec=env APPIMAGELAUNCHER_DISABLE=1 "/d/b\\`x/current.AppImage""#,
            "TryExec=/d/b`x/current.AppImage",
        ),
        (
            "/d/$x/current.AppImage",
            r#"Exec=env APPIMAGELAUNCHER_DISABLE=1 "/d/\\$x/current.AppImage""#,
            "TryExec=/d/$x/current.AppImage",
        ),
        (
            r"/d/b\x/current.AppImage",
            r#"Exec=env APPIMAGELAUNCHER_DISABLE=1 "/d/b\\\\x/current.AppImage""#,
            r"TryExec=/d/b\\x/current.AppImage",
        ),
        (
            "/d/50%/current.AppImage",
            r#"Exec=env APPIMAGELAUNCHER_DISABLE=1 "/d/50%%/current.AppImage""#,
            "TryExec=/d/50%/current.AppImage",
        ),
    ];
    for (path, exec, try_exec) in cases {
        let t = desktop_entry(Path::new(path), &spec("N", None));
        assert!(t.lines().any(|l| l == exec), "{path}: {t}");
        assert!(t.lines().any(|l| l == try_exec), "{path}: {t}");
    }
}

#[test]
fn desktop_entry_name_is_cleaned_and_escaped() {
    let t = desktop_entry(
        Path::new("/a/current.AppImage"),
        &spec("Bad\nName\x07\r\\x", None),
    );
    assert!(t.lines().any(|l| l == r"Name=BadName\\x"), "{t}");
    // Every line is one key=value (no injected keys from the newline).
    assert_eq!(t.lines().filter(|l| l.starts_with("Name=")).count(), 1);
    assert!(!t.contains("\nName\n"));
    // A name of only control characters falls back to the binary name.
    let t = desktop_entry(Path::new("/a/c.AppImage"), &spec("\n\t", None));
    assert!(t.lines().any(|l| l == "Name=my-app"));
}

#[test]
fn desktop_entry_validates_when_the_tool_is_available() {
    let Ok(validator) = which_validator() else {
        eprintln!("note: desktop-file-validate not on PATH, skipping validation");
        return;
    };
    let d = tempfile::tempdir().unwrap();
    for path in [
        "/d/plain/current.AppImage",
        "/d/sp ace/q\"x`b$y\\z%w/current.AppImage",
    ] {
        let f = d.path().join("t.desktop");
        fs::write(
            &f,
            desktop_entry(
                Path::new(path),
                &spec("Name\nwith newline", Some(Path::new("/d/icon.png"))),
            ),
        )
        .unwrap();
        let out = std::process::Command::new(&validator)
            .arg(&f)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{path}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

fn which_validator() -> Result<PathBuf, ()> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .map(|d| d.join("desktop-file-validate"))
        .find(|p| p.is_file())
        .ok_or(())
}

// ---- install / activate

#[tokio::test]
async fn first_install_links_copies_icon_and_writes_the_entry() {
    let fx = Fx::new();
    let r = fx.release("p1", "r1", "1.0.0", "app-one");
    let inst = fx.installer("/bin/true");
    assert_eq!(inst.state("p1").await.unwrap(), InstallState::NotInstalled);
    let a = inst.activate("p1", "r1").await.unwrap();
    assert_eq!(
        a.state,
        InstallState::Installed {
            release_id: "r1".into(),
            version: "1.0.0".into()
        }
    );
    assert_eq!(a.previous_version, None);
    let l = fx.layout("p1");
    assert_eq!(l.active_version().as_deref(), Some("1.0.0"));
    assert_eq!(fs::read_to_string(l.icon()).unwrap(), "icon r1");
    let entry = fs::read_to_string(l.desktop_file()).unwrap();
    assert!(entry.contains("StartupWMClass=app-one"));
    assert!(entry.contains(&format!("Icon={}", l.icon().display())));
    assert!(entry.contains("X-AI-App-Studio-Project=p1"));
    assert!(entry.contains(&format!("Name={}", r.product_name)));
    assert!(fx.db.get_install("p1").unwrap().is_some());
    assert!(
        fx.db
            .get_release("r1")
            .unwrap()
            .unwrap()
            .last_activated_at
            .is_some()
    );
}

#[tokio::test]
async fn activating_another_release_reports_the_previous_one() {
    let fx = Fx::new();
    fx.release("p1", "r1", "1.0.0", "app-one");
    fx.release("p1", "r2", "1.1.0", "app-one");
    let inst = fx.installer("/bin/true");
    inst.activate("p1", "r1").await.unwrap();
    let entry_before = fs::read_to_string(fx.layout("p1").desktop_file()).unwrap();
    let a = inst.activate("p1", "r2").await.unwrap();
    assert_eq!(a.previous_release_id.as_deref(), Some("r1"));
    assert_eq!(a.previous_version.as_deref(), Some("1.0.0"));
    assert_eq!(fx.layout("p1").active_version().as_deref(), Some("1.1.0"));
    // The entry only changes on first install.
    assert_eq!(
        fs::read_to_string(fx.layout("p1").desktop_file()).unwrap(),
        entry_before
    );
    // Rollback is the same call.
    let back = inst.activate("p1", "r1").await.unwrap();
    assert_eq!(back.previous_release_id.as_deref(), Some("r2"));
    assert_eq!(fx.layout("p1").active_version().as_deref(), Some("1.0.0"));
}

#[tokio::test]
async fn sha_mismatch_is_refused_and_the_old_link_stays() {
    let fx = Fx::new();
    fx.release("p1", "r1", "1.0.0", "app-one");
    let r2 = fx.release("p1", "r2", "1.1.0", "app-one");
    let inst = fx.installer("/bin/true");
    inst.activate("p1", "r1").await.unwrap();
    fs::write(fx.layout("p1").release_appimage(&r2.version), "tampered").unwrap();
    let err = inst.activate("p1", "r2").await.unwrap_err();
    assert!(matches!(err, InstallError::ChecksumMismatch(_)), "{err}");
    assert_eq!(fx.layout("p1").active_version().as_deref(), Some("1.0.0"));
    assert_eq!(
        fs::read_to_string(fx.layout("p1").icon()).unwrap(),
        "icon r1"
    );
}

#[tokio::test]
async fn a_stale_temp_link_from_a_crash_is_replaced() {
    let fx = Fx::new();
    fx.release("p1", "r1", "1.0.0", "app-one");
    let l = fx.layout("p1");
    fs::create_dir_all(l.root()).unwrap();
    symlink("garbage", l.root().join(".current.AppImage.tmp")).unwrap();
    fx.installer("/bin/true")
        .activate("p1", "r1")
        .await
        .unwrap();
    assert_eq!(l.active_version().as_deref(), Some("1.0.0"));
    assert!(
        l.root()
            .join(".current.AppImage.tmp")
            .symlink_metadata()
            .is_err()
    );
    assert!(l.current_link().exists(), "link resolves to the AppImage");
}

#[tokio::test]
async fn install_is_refused_when_policy_says_so() {
    let fx = Fx::new();
    let inst = fx.installer("/bin/true");

    fx.release_with("p1", "rf", "1.0.0", "app-one", Smoke::Failed);
    assert!(matches!(
        inst.activate("p1", "rf").await,
        Err(InstallError::SmokeFailed { .. })
    ));

    fx.release("p1", "rr", "1.1.0", "app-one");
    fx.db.mark_release_files_removed("rr").unwrap();
    assert!(matches!(
        inst.activate("p1", "rr").await,
        Err(InstallError::FilesRemoved { .. })
    ));

    // Unknown release, and another project's release.
    assert!(matches!(
        inst.activate("p1", "nope").await,
        Err(InstallError::ReleaseNotFound)
    ));
    fx.release("p2", "x2", "1.0.0", "other");
    assert!(matches!(
        inst.activate("p1", "x2").await,
        Err(InstallError::ReleaseNotFound)
    ));
    assert_eq!(fx.layout("p1").active_version(), None);
    assert!(!fx.layout("p1").desktop_file().exists());
}

#[tokio::test]
async fn a_binary_name_clash_with_another_install_is_refused() {
    let fx = Fx::new();
    fx.release("p1", "r1", "1.0.0", "same");
    fx.release("p2", "r2", "1.0.0", "same");
    fx.release("p2", "r3", "1.1.0", "different");
    let inst = fx.installer("/bin/true");
    inst.activate("p1", "r1").await.unwrap();
    let err = inst.activate("p2", "r2").await.unwrap_err();
    assert!(matches!(err, InstallError::BinaryClash { .. }), "{err}");
    assert_eq!(fx.layout("p2").active_version(), None);
    inst.activate("p2", "r3").await.unwrap();
}

#[tokio::test]
async fn a_foreign_desktop_file_is_never_overwritten() {
    let fx = Fx::new();
    fx.release("p1", "r1", "1.0.0", "app-one");
    let foreign = fx.write_foreign("p1");
    let err = fx
        .installer("/bin/true")
        .activate("p1", "r1")
        .await
        .unwrap_err();
    assert!(matches!(err, InstallError::ForeignFile(_)), "{err}");
    assert_eq!(
        fs::read_to_string(&foreign).unwrap(),
        "[Desktop Entry]\nName=Someone else\n"
    );
    assert_eq!(fx.layout("p1").active_version(), None);
}

#[tokio::test]
async fn concurrent_activations_serialize_and_leave_a_valid_link() {
    let fx = Fx::new();
    fx.release("p1", "r1", "1.0.0", "app-one");
    fx.release("p1", "r2", "1.1.0", "app-one");
    let inst = Arc::new(fx.installer("/bin/true"));
    let mut tasks = Vec::new();
    for i in 0..8 {
        let inst = inst.clone();
        tasks.push(tokio::spawn(async move {
            inst.activate("p1", if i % 2 == 0 { "r1" } else { "r2" })
                .await
        }));
    }
    for t in tasks {
        t.await.unwrap().unwrap();
    }
    let v = fx.layout("p1").active_version().unwrap();
    assert!(v == "1.0.0" || v == "1.1.0");
    assert!(fx.layout("p1").current_link().exists());
}

// ---- state

#[tokio::test]
async fn state_unknown_release_and_launcher_missing() {
    let fx = Fx::new();
    fx.release("p1", "r1", "1.0.0", "app-one");
    let inst = fx.installer("/bin/true");
    inst.activate("p1", "r1").await.unwrap();
    let l = fx.layout("p1");

    // Symlink to a version without a kept row.
    fs::remove_file(l.current_link()).unwrap();
    symlink(AppLayout::current_target("9.9.9"), l.current_link()).unwrap();
    assert_eq!(
        inst.state("p1").await.unwrap(),
        InstallState::UnknownRelease {
            version: Some("9.9.9".into())
        }
    );
    // Back to a known one, then lose the entry.
    fs::remove_file(l.current_link()).unwrap();
    symlink(AppLayout::current_target("1.0.0"), l.current_link()).unwrap();
    fs::remove_file(l.desktop_file()).unwrap();
    assert_eq!(
        inst.state("p1").await.unwrap(),
        InstallState::LauncherMissing {
            release_id: Some("r1".into()),
            version: Some("1.0.0".into())
        }
    );
}

// ---- uninstall

#[tokio::test]
async fn uninstall_removes_ours_and_leaves_everything_else() {
    let fx = Fx::new();
    fx.release("p1", "r1", "1.0.0", "app-one");
    fx.release("p1", "r2", "1.1.0", "app-one");
    fx.release("p2", "s1", "1.0.0", "app-two");
    let inst = fx.installer("/bin/true");
    inst.activate("p1", "r2").await.unwrap();
    inst.activate("p2", "s1").await.unwrap();
    // A foreign file next to ours must survive.
    let foreign = fx.paths.applications_dir.join("someone-else.desktop");
    fs::write(&foreign, "[Desktop Entry]\n").unwrap();

    let mut ids = inst.uninstall("p1").await.unwrap();
    ids.sort();
    assert_eq!(ids, ["r1", "r2"]);
    let l = fx.layout("p1");
    assert!(!l.desktop_file().exists());
    assert!(
        l.root().symlink_metadata().is_err(),
        "icon, link and files gone"
    );
    for id in ["r1", "r2"] {
        assert!(
            fx.db
                .get_release(id)
                .unwrap()
                .unwrap()
                .files_removed_at
                .is_some()
        );
    }
    assert!(fx.db.get_install("p1").unwrap().is_none());
    assert_eq!(inst.state("p1").await.unwrap(), InstallState::NotInstalled);
    // The sibling project and the foreign entry are untouched.
    let l2 = fx.layout("p2");
    assert!(l2.desktop_file().exists() && l2.icon().exists() && l2.current_link().exists());
    assert!(
        fx.db
            .get_release("s1")
            .unwrap()
            .unwrap()
            .files_removed_at
            .is_none()
    );
    assert!(fx.db.get_install("p2").unwrap().is_some());
    assert!(foreign.exists());
}

#[tokio::test]
async fn uninstall_refuses_a_foreign_entry_and_deletes_nothing() {
    let fx = Fx::new();
    fx.release("p1", "r1", "1.0.0", "app-one");
    let inst = fx.installer("/bin/true");
    inst.activate("p1", "r1").await.unwrap();
    let f = fx.layout("p1").desktop_file().to_owned();
    fs::write(&f, "[Desktop Entry]\nName=Replaced\n").unwrap();
    let err = inst.uninstall("p1").await.unwrap_err();
    assert!(matches!(err, InstallError::ForeignFile(_)), "{err}");
    assert!(f.exists());
    assert!(fx.layout("p1").release_appimage("1.0.0").exists());
    assert!(fx.db.get_install("p1").unwrap().is_some());
}

// ---- repair

#[tokio::test]
async fn repair_rewrites_a_missing_entry_and_icon() {
    let fx = Fx::new();
    fx.release("p1", "r1", "1.0.0", "app-one");
    let inst = fx.installer("/bin/true");
    inst.activate("p1", "r1").await.unwrap();
    let l = fx.layout("p1");
    fs::remove_file(l.desktop_file()).unwrap();
    fs::remove_file(l.icon()).unwrap();
    let s = inst.repair("p1").await.unwrap();
    assert_eq!(
        s,
        InstallState::Installed {
            release_id: "r1".into(),
            version: "1.0.0".into()
        }
    );
    assert!(l.icon().exists());
    assert!(
        fs::read_to_string(l.desktop_file())
            .unwrap()
            .contains("Icon=")
    );
}

#[tokio::test]
async fn repair_refuses_foreign_and_not_installed() {
    let fx = Fx::new();
    fx.release("p1", "r1", "1.0.0", "app-one");
    let inst = fx.installer("/bin/true");
    assert!(matches!(
        inst.repair("p1").await,
        Err(InstallError::NotInstalled)
    ));
    inst.activate("p1", "r1").await.unwrap();
    fs::remove_file(fx.layout("p1").desktop_file()).unwrap();
    let f = fx.write_foreign("p1");
    assert!(matches!(
        inst.repair("p1").await,
        Err(InstallError::ForeignFile(_))
    ));
    assert!(fs::read_to_string(f).unwrap().contains("Someone else"));
}

// ---- launch

#[tokio::test]
async fn launch_reports_gio_failure_and_refuses_when_not_installed() {
    let fx = Fx::new();
    fx.release("p1", "r1", "1.0.0", "app-one");
    let failing = fx.installer("/bin/false");
    assert!(matches!(
        failing.launch("p1", vec![]).await,
        Err(InstallError::NotInstalled)
    ));
    failing.activate("p1", "r1").await.unwrap();
    let err = failing.launch("p1", vec![]).await.unwrap_err();
    assert!(matches!(err, InstallError::Launch(_)), "{err}");

    let missing = fx.installer("/nonexistent/gio");
    let err = missing.launch("p1", vec![]).await.unwrap_err();
    assert!(err.to_string().contains("/nonexistent/gio"), "{err}");

    // A stderr message is passed through.
    let script = fx.paths.data_dir.join("fake-gio.sh");
    fs::write(&script, "#!/bin/sh\necho 'no such launcher' >&2\nexit 3\n").unwrap();
    fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let err = fx
        .installer(script.to_str().unwrap())
        .launch("p1", vec![])
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "no such launcher");

    // And success.
    fx.installer("/bin/true")
        .launch("p1", vec![])
        .await
        .unwrap();
}

// ---- app databases: backup before a release switch (DR-7, R10)

use crate::domain::{DatabaseEntry, ProjectMeta};
use crate::persistence::{BackupReason, DataSet};

impl Fx {
    /// Gives the project a real workspace with the `database` entry; the app's data file is
    /// `<XDG data home>/com.x.p1/app.db`.
    fn with_database(&self, project: &str) {
        let ws = self._dir.path().join(format!("ws-{project}"));
        fs::create_dir_all(ws.join("src-tauri")).unwrap();
        fs::write(
            ws.join("src-tauri/tauri.conf.json"),
            format!(r#"{{"productName":"App","identifier":"com.x.{project}"}}"#),
        )
        .unwrap();
        let mut meta = ProjectMeta::new(project, project, TemplateId::TauriVue, "com.x");
        meta.id = project.into();
        meta.database = Some(DatabaseEntry {
            engine: "sqlite".into(),
        });
        meta.write(&ws).unwrap();
        let mut row = self.db.get_project(project).unwrap().unwrap();
        row.workspace_path = ws.display().to_string();
        self.db.update_project(&row).unwrap();
    }

    fn app_dbs(&self, _project: &str) -> crate::services::appdb::AppDb {
        crate::services::appdb::AppDb::new(self.db.clone(), &self.paths)
    }

    fn app_db(&self, project: &str) -> PathBuf {
        self._dir.path().join(format!("com.x.{project}/app.db"))
    }

    fn write_app_data(&self, project: &str, value: &str) {
        let f = self.app_db(project);
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        let _ = fs::remove_file(&f);
        rusqlite::Connection::open(&f)
            .unwrap()
            .execute_batch(&format!(
                "CREATE TABLE t (v TEXT); INSERT INTO t VALUES ('{value}'); PRAGMA user_version = 3;"
            ))
            .unwrap();
    }
}

fn backup_value(fx: &Fx, backup_id: &str) -> String {
    let b = fx.db.get_backup(backup_id).unwrap().unwrap();
    rusqlite::Connection::open(b.path)
        .unwrap()
        .query_row("SELECT v FROM t", [], |r| r.get(0))
        .unwrap()
}

#[tokio::test]
async fn switching_releases_backs_up_the_apps_data_first_and_the_backup_follows_the_old_release() {
    let fx = Fx::new();
    fx.with_database("p1");
    fx.release("p1", "r1", "1.0.0", "app-one");
    fx.release("p1", "r2", "1.1.0", "app-one");
    let inst = fx.installer("/bin/true");
    // First install, and the app has not created data yet: nothing to back up.
    inst.activate("p1", "r1").await.unwrap();
    assert!(fx.db.list_backups("p1", None).unwrap().is_empty());

    fx.write_app_data("p1", "written by 1.0.0");
    inst.activate("p1", "r2").await.unwrap();
    let backups = fx.db.list_backups("p1", None).unwrap();
    assert_eq!(backups.len(), 1);
    let b = &backups[0];
    assert_eq!(
        (b.reason, b.which, b.label.as_deref()),
        (
            BackupReason::Release,
            DataSet::Installed,
            Some("before 1.1.0")
        )
    );
    assert_eq!(
        b.release_id.as_deref(),
        Some("r1"),
        "owned by the replaced release"
    );
    assert_eq!(backup_value(&fx, &b.id), "written by 1.0.0");
    assert_eq!(fx.layout("p1").active_version().as_deref(), Some("1.1.0"));

    // Roll back: the data written by 1.1.0 is saved too, as "before 1.0.0", owned by 1.1.0.
    fx.write_app_data("p1", "written by 1.1.0");
    inst.activate("p1", "r1").await.unwrap();
    let backups = fx.db.list_backups("p1", None).unwrap();
    assert_eq!(backups.len(), 2);
    assert_eq!(backups[0].label.as_deref(), Some("before 1.0.0"));
    assert_eq!(backups[0].release_id.as_deref(), Some("r2"));
    assert_eq!(backup_value(&fx, &backups[0].id), "written by 1.1.0");
    assert_eq!(fx.layout("p1").active_version().as_deref(), Some("1.0.0"));
}

#[tokio::test]
async fn a_failed_backup_stops_the_switch_and_changes_nothing() {
    let fx = Fx::new();
    fx.with_database("p1");
    fx.release("p1", "r1", "1.0.0", "app-one");
    fx.release("p1", "r2", "1.1.0", "app-one");
    let inst = fx.installer("/bin/true");
    inst.activate("p1", "r1").await.unwrap();
    // A damaged data file cannot be copied.
    let f = fx.app_db("p1");
    fs::create_dir_all(f.parent().unwrap()).unwrap();
    fs::write(&f, "garbage ".repeat(500)).unwrap();

    let e = inst.activate("p1", "r2").await.unwrap_err();
    assert!(matches!(e, InstallError::BackupFailed { .. }), "{e}");
    let msg = e.to_string();
    let (headline, reason) = msg.split_once('\n').unwrap();
    assert_eq!(
        headline,
        "Couldn't back up App r2's data, so nothing was changed"
    );
    assert!(reason.contains("not a database"), "{reason}");
    assert_eq!(
        fx.layout("p1").active_version().as_deref(),
        Some("1.0.0"),
        "still on the old release"
    );
    assert!(fx.db.list_backups("p1", None).unwrap().is_empty());
    assert!(
        fx.db
            .get_release("r2")
            .unwrap()
            .unwrap()
            .last_activated_at
            .is_none(),
        "r2 was not marked activated"
    );
}

#[tokio::test]
async fn a_first_install_over_existing_data_backs_it_up_for_the_new_release() {
    let fx = Fx::new();
    fx.with_database("p1");
    fx.release("p1", "r1", "1.0.0", "app-one");
    fx.write_app_data("p1", "left from an earlier install");
    fx.installer("/bin/true")
        .activate("p1", "r1")
        .await
        .unwrap();
    let b = &fx.db.list_backups("p1", None).unwrap()[0];
    assert_eq!(b.label.as_deref(), Some("before 1.0.0"));
    assert_eq!(b.release_id.as_deref(), Some("r1"));
}

#[tokio::test]
async fn activating_the_active_release_again_makes_no_backup() {
    let fx = Fx::new();
    fx.with_database("p1");
    fx.release("p1", "r1", "1.0.0", "app-one");
    let inst = fx.installer("/bin/true");
    inst.activate("p1", "r1").await.unwrap();
    fx.write_app_data("p1", "x");
    inst.activate("p1", "r1").await.unwrap();
    assert!(fx.db.list_backups("p1", None).unwrap().is_empty());
}

#[tokio::test]
async fn a_project_without_the_entry_is_switched_without_any_backup() {
    let fx = Fx::new();
    fx.release("p2", "r1", "1.0.0", "app-two");
    fx.release("p2", "r2", "1.1.0", "app-two");
    fx.write_app_data("p2", "x");
    let inst = fx.installer("/bin/true");
    inst.activate("p2", "r1").await.unwrap();
    inst.activate("p2", "r2").await.unwrap();
    assert!(fx.db.list_backups("p2", None).unwrap().is_empty());
}

#[tokio::test]
async fn uninstalling_deletes_the_automatic_backups_with_the_releases() {
    let fx = Fx::new();
    fx.with_database("p1");
    fx.release("p1", "r1", "1.0.0", "app-one");
    fx.release("p1", "r2", "1.1.0", "app-one");
    let inst = fx.installer("/bin/true");
    inst.activate("p1", "r1").await.unwrap();
    fx.write_app_data("p1", "x");
    inst.activate("p1", "r2").await.unwrap();
    let manual = fx.app_dbs("p1").backup("p1", DataSet::Installed).unwrap();
    let auto = fx.db.list_release_backups("r1").unwrap().remove(0);
    inst.uninstall("p1").await.unwrap();
    assert!(!Path::new(&auto.path).exists());
    assert!(fx.db.list_release_backups("r1").unwrap().is_empty());
    let left = fx.db.list_backups("p1", None).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].id, manual.id);
    // The app's own data stays where it was.
    assert!(fx.app_db("p1").exists());
}
