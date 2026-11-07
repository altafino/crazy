use super::*;
use crate::domain::TemplateId;
use crate::persistence::{
    Db, InstallRow, JobState, NewProject, ReleaseRow, SessionMode, SessionRow, Smoke,
};
use crate::platform::paths::StudioPaths;
use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

/// A Studio on temp folders (never the real `~/.local/share`) with projects `p1` and `p2`.
struct Fx {
    _tmp: TempDir,
    root: PathBuf,
    st: AppState,
}

impl Fx {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let home = root.join("home");
        let paths = StudioPaths::from_env(|k| match k {
            "HOME" => Some(home.to_string_lossy().into_owned()),
            _ => None,
        })
        .unwrap();
        fs::create_dir_all(&paths.data_dir).unwrap();
        let st = AppState::new(Db::open_in_memory().unwrap(), paths, root.join("templates"));
        let fx = Self {
            _tmp: tmp,
            root,
            st,
        };
        fx.project("p1");
        fx.project("p2");
        fx
    }

    fn data(&self) -> &Path {
        &self.st.paths.data_dir
    }

    fn folder(&self, id: &str) -> PathBuf {
        self.root.join("home/Projects").join(id)
    }

    /// A project with a folder, logs, a session, a release folder, a backup and a shadow repo.
    fn project(&self, id: &str) {
        let folder = self.folder(id);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("main.rs"), "fn main() {}").unwrap();
        let db = &self.st.db;
        db.insert_project(&NewProject {
            id: id.into(),
            name: format!("App {id}"),
            slug: id.into(),
            template_id: TemplateId::TauriVue,
            app_id: format!("com.example.{id}"),
            workspace_path: folder.to_string_lossy().into_owned(),
            pending_op: None,
        })
        .unwrap();
        for (kind, dir) in [("preview", "previews"), ("build", "builds")] {
            let job = format!("{id}-{kind}");
            db.insert_job_of_kind(&job, id, kind, JobState::Succeeded)
                .unwrap();
            touch(&self.data().join(dir).join(format!("{job}.log")));
        }
        let job = format!("{id}-agent");
        db.insert_job(&job, id, JobState::Succeeded).unwrap();
        let log = self.data().join("sessions").join(format!("{id}.jsonl"));
        touch(&log);
        db.insert_session(&SessionRow {
            id: format!("{id}-s"),
            job_id: job,
            project_id: id.into(),
            profile_id: None,
            mode: SessionMode::Changes,
            prompt: "go".into(),
            provider_session_id: None,
            resumed_from: None,
            log_path: log.to_string_lossy().into_owned(),
            before_ref: None,
            after_ref: None,
            checkpoint_ref: None,
            created_at: String::new(),
        })
        .unwrap();
        db.insert_release(&release(id)).unwrap();
        touch(&self.release_file(id));
        touch(&self.st.appdb.backups_dir(id).join("b.sqlite"));
        fs::create_dir_all(self.st.snapshots.shadow(id).unwrap()).unwrap();
    }

    fn release_file(&self, id: &str) -> PathBuf {
        self.data()
            .join("installed")
            .join(id)
            .join("releases/1.0.0/app.AppImage")
    }

    /// Every file Studio stored for `id`, and whether each still exists.
    fn studio_files(&self, id: &str) -> Vec<(PathBuf, bool)> {
        let d = self.data();
        [
            d.join("previews").join(format!("{id}-preview.log")),
            d.join("builds").join(format!("{id}-build.log")),
            d.join("sessions").join(format!("{id}.jsonl")),
            self.release_file(id),
            self.st.appdb.backups_dir(id),
            self.st.snapshots.shadow(id).unwrap(),
        ]
        .into_iter()
        .map(|p| {
            let e = p.exists();
            (p, e)
        })
        .collect()
    }

    fn known(&self, id: &str) -> bool {
        self.st.db.get_project(id).unwrap().is_some()
    }

    async fn remove(&self, id: &str, trash: Option<Box<Trash>>) -> Result<(), CommandError> {
        self.st.remove_project(id.into(), trash).await
    }
}

fn touch(p: &Path) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, "x").unwrap();
}

fn release(project: &str) -> ReleaseRow {
    ReleaseRow {
        id: format!("{project}-r1"),
        project_id: project.into(),
        job_id: None,
        version: "1.0.0".into(),
        sha256: "ab".repeat(32),
        size_bytes: 1,
        architecture: "x86_64".into(),
        build_profile: "local".into(),
        product_name: "App".into(),
        main_binary_name: "app".into(),
        source_commit: None,
        dirty_source: false,
        snapshot_ref: None,
        smoke: Smoke::Passed,
        smoke_detail: None,
        notes: None,
        created_at: "2026-11-07T00:00:00Z".into(),
        last_activated_at: None,
        files_removed_at: None,
        schema_version: None,
    }
}

fn all_exist(files: &[(PathBuf, bool)]) -> bool {
    files.iter().all(|(_, e)| *e)
}

/// A trash that moves the folder into `<root>/trash` and records it.
fn fake_trash(root: &Path, seen: Arc<Mutex<Vec<PathBuf>>>) -> Box<Trash> {
    let bin = root.join("trash");
    Box::new(move |p: &Path| {
        seen.lock().unwrap().push(p.to_path_buf());
        fs::create_dir_all(&bin).unwrap();
        fs::rename(p, bin.join(p.file_name().unwrap())).map_err(|e| e.to_string())
    })
}

#[tokio::test]
async fn removes_studio_data_and_keeps_the_folder() {
    let fx = Fx::new();
    assert!(all_exist(&fx.studio_files("p1")));

    fx.remove("p1", None).await.unwrap();

    assert!(!fx.known("p1"));
    let left: Vec<_> = fx
        .studio_files("p1")
        .into_iter()
        .filter(|(_, e)| *e)
        .collect();
    assert!(left.is_empty(), "left behind: {left:?}");
    assert!(fx.st.db.list_jobs_by_project("p1").unwrap().is_empty());
    assert!(
        fx.folder("p1").join("main.rs").exists(),
        "the project folder stays"
    );
    assert!(
        fx.known("p2") && all_exist(&fx.studio_files("p2")),
        "p2 untouched"
    );
}

#[tokio::test]
async fn trash_moves_the_folder_first() {
    let fx = Fx::new();
    let seen = Arc::new(Mutex::new(vec![]));

    fx.remove("p1", Some(fake_trash(&fx.root, seen.clone())))
        .await
        .unwrap();

    assert_eq!(*seen.lock().unwrap(), vec![fx.folder("p1")]);
    assert!(!fx.folder("p1").exists());
    assert!(fx.root.join("trash/p1/main.rs").exists());
    assert!(!fx.known("p1"));
}

#[tokio::test]
async fn a_failed_trash_removes_nothing() {
    let fx = Fx::new();
    let failing: Box<Trash> = Box::new(|_: &Path| Err("no trash here".into()));

    let err = fx.remove("p1", Some(failing)).await.unwrap_err();

    assert!(err.message.contains("no trash here"), "{}", err.message);
    assert!(fx.known("p1") && all_exist(&fx.studio_files("p1")));
}

#[tokio::test]
async fn trash_refuses_a_folder_around_studio_data() {
    let fx = Fx::new();
    // p3's folder is the fake home, which holds Studio's data dir.
    fx.st
        .db
        .insert_project(&NewProject {
            id: "p3".into(),
            name: "Home".into(),
            slug: "home".into(),
            template_id: TemplateId::TauriVue,
            app_id: "com.example.home".into(),
            workspace_path: fx.root.join("home").to_string_lossy().into_owned(),
            pending_op: None,
        })
        .unwrap();
    let seen = Arc::new(Mutex::new(vec![]));

    let err = fx
        .remove("p3", Some(fake_trash(&fx.root, seen.clone())))
        .await
        .unwrap_err();

    assert!(
        err.message.contains("not moved to the Trash"),
        "{}",
        err.message
    );
    assert!(seen.lock().unwrap().is_empty(), "the trash never ran");
    assert!(fx.known("p3"));
}

#[tokio::test]
async fn refuses_an_installed_app_and_a_running_job() {
    let fx = Fx::new();
    fx.st
        .db
        .upsert_install(&InstallRow {
            project_id: "p1".into(),
            desktop_file: "x.desktop".into(),
            installed_at: "2026-11-07T00:00:00Z".into(),
        })
        .unwrap();
    let err = fx.remove("p1", None).await.unwrap_err();
    assert!(
        err.message.contains("Uninstall it first"),
        "{}",
        err.message
    );

    assert!(fx.st.jobs.slots().claim("p2", JobKind::Build, "busy"));
    let err = fx.remove("p2", None).await.unwrap_err();
    assert!(
        err.message.contains("build or export is running"),
        "{}",
        err.message
    );

    assert!(fx.known("p1") && all_exist(&fx.studio_files("p1")));
    assert!(fx.known("p2") && all_exist(&fx.studio_files("p2")));
}

#[tokio::test]
async fn an_unknown_project_is_not_found() {
    let fx = Fx::new();
    let err = fx.remove("nope", None).await.unwrap_err();
    assert!(err.message.contains("not found"), "{}", err.message);
}
