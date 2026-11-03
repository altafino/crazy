use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use super::*;

struct Env {
    root: tempfile::TempDir,
    db: Arc<Db>,
    svc: ProjectService,
}

impl Env {
    fn new() -> Self {
        let template = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../templates/tauri-vue");
        Self::with_template(template, "git".into())
    }

    fn with_template(template: PathBuf, git: PathBuf) -> Self {
        let root = tempfile::tempdir().unwrap();
        let db = Arc::new(Db::open_in_memory().unwrap());
        fs::create_dir(root.path().join("parent")).unwrap();
        let svc = ProjectService::new(
            db.clone(),
            template,
            root.path().join("data"),
            GitCmd {
                program: git,
                env: crate::platform::process_env(),
            },
        );
        Self { root, db, svc }
    }

    fn parent(&self) -> PathBuf {
        self.root.path().join("parent").canonicalize().unwrap()
    }

    fn req(&self, name: &str, slug: &str) -> NewProjectRequest {
        NewProjectRequest {
            name: name.into(),
            slug: Some(slug.into()),
            parent_dir: self.parent().to_string_lossy().into_owned(),
            identifier: None,
            init_git: false,
        }
    }

    /// A folder outside the library, as if the user had it already.
    fn folder(&self, name: &str) -> PathBuf {
        let p = self.root.path().join("elsewhere").join(name);
        fs::create_dir_all(&p).unwrap();
        p.canonicalize().unwrap()
    }
}

fn json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn write_conf(dir: &Path, name: &str, identifier: &str) {
    fs::create_dir_all(dir.join("src-tauri")).unwrap();
    fs::write(
        dir.join(TAURI_CONF),
        serde_json::json!({"productName": name, "identifier": identifier}).to_string(),
    )
    .unwrap();
}

fn write_meta(dir: &Path, id: &str, slug: &str) {
    let mut m = ProjectMeta::new("Old", slug, TemplateId::TauriVue, "com.old.app");
    m.id = id.into();
    m.write(dir).unwrap();
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

// ---- create ----

#[test]
fn create_substitutes_awkward_names_as_parsed_data() {
    let cases = [
        (r#"He said "Hi" \ 🎉"#, "quotes-emoji", "quotes-emoji"),
        ("日本語アプリ", "cjk-app", "cjk-app"),
        ("3D <Viewer> & Co", "3d-viewer", "app-3d-viewer"),
    ];
    for (name, slug, cargo) in cases {
        let env = Env::new();
        let p = env.svc.create(&env.req(name, slug)).unwrap();
        let dir = PathBuf::from(&p.workspace_path);
        assert_eq!(dir, env.parent().join(slug));

        let conf = json(&dir.join(TAURI_CONF));
        assert_eq!(conf["productName"], name);
        assert_eq!(conf["app"]["windows"][0]["title"], name);
        assert_eq!(conf["mainBinaryName"], cargo);
        assert_eq!(conf["identifier"], p.app_id);
        assert_eq!(json(&dir.join("package.json"))["name"], slug);

        let cargo_toml: toml_edit::DocumentMut =
            fs::read_to_string(dir.join("src-tauri/Cargo.toml"))
                .unwrap()
                .parse()
                .unwrap();
        assert_eq!(cargo_toml["package"]["name"].as_str(), Some(cargo));
        assert_eq!(cargo_toml["lib"]["name"].as_str(), Some("app_lib"));

        let html = fs::read_to_string(dir.join("index.html")).unwrap();
        assert!(!html.contains("Tauri + Vue"));
        assert!(!html.contains("<Viewer>"), "name must be HTML-escaped");
        let readme = fs::read_to_string(dir.join("README.md")).unwrap();
        assert_eq!(readme.lines().next().unwrap(), format!("# {name}"));

        let meta = ProjectMeta::read(&dir).unwrap().unwrap();
        assert_eq!(
            (meta.id.as_str(), meta.name.as_str()),
            (p.id.as_str(), name)
        );
        assert_eq!(meta.template_id, TemplateId::TauriVue);
        assert_eq!(env.svc.list().unwrap(), vec![p]);
        assert_eq!(env.db.list_projects().unwrap()[0].pending_op, None);
    }
}

#[test]
fn html_title_is_escaped_not_dropped() {
    let env = Env::new();
    let p = env.svc.create(&env.req("A <b> & \"c\"", "esc")).unwrap();
    let html = fs::read_to_string(Path::new(&p.workspace_path).join("index.html")).unwrap();
    assert!(html.contains("<title>A &lt;b&gt; &amp; &quot;c&quot;</title>"));
}

#[test]
fn create_skips_build_artifacts_and_key_order_is_kept() {
    let env0 = Env::new();
    let tpl = env0.root.path().join("tpl");
    template::copy_template(&env0.svc.template_dir, &tpl).unwrap();
    for d in ["node_modules", "dist", ".git", "src-tauri/target"] {
        fs::create_dir_all(tpl.join(d)).unwrap();
        fs::write(tpl.join(d).join("junk"), "x").unwrap();
    }
    let env = Env::with_template(tpl, "git".into());
    let p = env.svc.create(&env.req("App", "app")).unwrap();
    let dir = Path::new(&p.workspace_path);
    for d in ["node_modules", "dist", ".git", "src-tauri/target"] {
        assert!(!dir.join(d).exists(), "{d} must not be copied");
    }
    let pkg = fs::read_to_string(dir.join("package.json")).unwrap();
    assert!(pkg.find("\"name\"").unwrap() < pkg.find("\"scripts\"").unwrap());
}

#[test]
fn create_with_git_makes_a_commit_and_without_makes_no_repo() {
    let env = Env::new();
    let mut req = env.req("With Git", "with-git");
    req.init_git = true;
    let p = env.svc.create(&req).unwrap();
    let out = Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(&p.workspace_path)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("Initial commit"));
    // The vendored .gitignore must keep generated output out of the first commit.
    let tracked = Command::new("git")
        .args(["ls-files"])
        .current_dir(&p.workspace_path)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&tracked.stdout).contains(".ai-app-studio/project.json"));

    let p = env.svc.create(&env.req("No Git", "no-git")).unwrap();
    assert!(!Path::new(&p.workspace_path).join(".git").exists());
}

#[test]
fn created_project_ignores_env_files_but_not_the_example() {
    let env = Env::new();
    let mut req = env.req("Env Ignore", "env-ignore");
    req.init_git = true;
    let p = env.svc.create(&req).unwrap();
    let dir = Path::new(&p.workspace_path);
    let ignored = |name: &str| {
        Command::new("git")
            .args(["check-ignore", "-q", name])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    };
    assert!(ignored(".env"));
    assert!(ignored(".env.local"));
    assert!(!ignored(".env.example"));
}

#[test]
fn git_failure_cleans_up_but_keeps_user_folder() {
    let env = Env::new();
    let picked = env.parent().join("picked");
    fs::create_dir(&picked).unwrap();
    let svc = ProjectService::new(
        env.db.clone(),
        env.svc.template_dir.clone(),
        env.svc.studio_data_dir.clone(),
        GitCmd {
            program: "/nonexistent/git".into(),
            env: crate::platform::process_env(),
        },
    );
    let mut req = env.req("Picked", "picked");
    req.init_git = true;
    let err = svc.create(&req).unwrap_err();
    assert!(matches!(err, ProjectError::Git(_)), "{err}");
    assert!(picked.is_dir() && names_in(&picked).is_empty());
    assert_eq!(
        names_in(&env.parent()),
        ["picked"],
        "temp folder must be gone"
    );
    assert!(env.db.list_projects().unwrap().is_empty());
}

#[test]
fn create_fills_an_existing_empty_folder() {
    let env = Env::new();
    fs::create_dir(env.parent().join("empty")).unwrap();
    let p = env.svc.create(&env.req("Empty", "empty")).unwrap();
    assert!(Path::new(&p.workspace_path).join("package.json").is_file());
}

#[test]
fn non_empty_target_is_refused_and_untouched() {
    let env = Env::new();
    let full = env.parent().join("full");
    fs::create_dir(&full).unwrap();
    fs::write(full.join("mine.txt"), "x").unwrap();
    let err = env.svc.create(&env.req("Full", "full")).unwrap_err();
    let ProjectError::Validation(errs) = err else {
        panic!("{err}")
    };
    assert_eq!(errs[0].field, "slug");
    assert_eq!(names_in(&full), ["mine.txt"]);
    assert!(env.db.list_projects().unwrap().is_empty());
}

#[test]
fn validate_new_reports_every_problem_at_once() {
    let env = Env::new();
    let req = NewProjectRequest {
        name: "   ".into(),
        slug: None,
        parent_dir: "relative/dir".into(),
        identifier: Some("nodots".into()),
        init_git: false,
    };
    let fields: Vec<_> = env
        .svc
        .validate_new(&req)
        .into_iter()
        .map(|e| e.field)
        .collect();
    assert_eq!(fields, ["name", "slug", "identifier", "parentDir"]);
    assert!(env.svc.validate_new(&env.req("Fine", "fine")).is_empty());
}

#[test]
fn slug_collision_is_refused() {
    let env = Env::new();
    env.svc.create(&env.req("One", "same")).unwrap();
    let mut second = env.req("Two", "same");
    second.parent_dir = env.folder("other-parent").to_string_lossy().into_owned();
    let errs = env.svc.validate_new(&second);
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].field, "slug");
}

#[test]
fn target_inside_library_project_or_data_dir_is_refused() {
    let env = Env::new();
    let p = env.svc.create(&env.req("Outer", "outer")).unwrap();
    let mut inside = env.req("Inner", "inner");
    inside.parent_dir = p.workspace_path.clone();
    assert!(!env.svc.validate_new(&inside).is_empty());

    let data = env.root.path().join("data");
    fs::create_dir(&data).unwrap();
    let mut in_data = env.req("Data", "dataproj");
    in_data.parent_dir = data.to_string_lossy().into_owned();
    assert!(!env.svc.validate_new(&in_data).is_empty());
}

#[test]
fn empty_slug_name_is_refused() {
    let env = Env::new();
    let mut req = env.req("日本語", "x");
    req.slug = None;
    let errs = env.svc.validate_new(&req);
    assert_eq!(errs[0].message, "choose a name with letters or digits");
}

// ---- import ----

#[test]
fn import_tauri_folder_without_metadata_reads_conf_and_writes_json() {
    let env = Env::new();
    let dir = env.folder("some-app");
    write_conf(&dir, "Some App", "org.me.someapp");
    let ImportOutcome::Imported { project } = env.svc.import(&dir).unwrap() else {
        panic!()
    };
    assert_eq!(project.template_id, TemplateId::TauriVue);
    assert_eq!(
        (project.name.as_str(), project.app_id.as_str()),
        ("Some App", "org.me.someapp")
    );
    assert_eq!(project.slug, "some-app");
    let meta = ProjectMeta::read(&dir).unwrap().unwrap();
    assert_eq!(meta.id, project.id);
    assert_eq!(project.state, ProjectState::Ok);
    assert!(!dir.join(".git").exists(), "import never runs git init");
}

#[test]
fn import_plain_folder_is_existing_template() {
    let env = Env::new();
    let dir = env.folder("Plain Folder");
    let ImportOutcome::Imported { project } = env.svc.import(&dir).unwrap() else {
        panic!()
    };
    assert_eq!(project.template_id, TemplateId::Existing);
    assert!(!project.template_id.supports_operations());
    assert_eq!(project.name, "Plain Folder");
    assert!(!dir.join(".git").exists());
}

#[test]
fn import_colliding_slug_gets_suffix() {
    let env = Env::new();
    let a = env.folder("a/same");
    let b = env.folder("b/same");
    env.svc.import(&a).unwrap();
    let ImportOutcome::Imported { project } = env.svc.import(&b).unwrap() else {
        panic!()
    };
    assert_eq!(project.slug, "same-2");
}

#[cfg(unix)]
#[test]
fn reimport_via_symlink_is_already_in_library() {
    let env = Env::new();
    let dir = env.folder("real");
    env.svc.import(&dir).unwrap();
    let link = env.root.path().join("link");
    std::os::unix::fs::symlink(&dir, &link).unwrap();
    assert!(matches!(
        env.svc.import(&link).unwrap(),
        ImportOutcome::AlreadyInLibrary { .. }
    ));
    assert_eq!(env.db.list_projects().unwrap().len(), 1);
}

#[test]
fn import_with_unknown_id_keeps_json_untouched() {
    let env = Env::new();
    let dir = env.folder("known");
    write_meta(&dir, "11111111-1111-4111-8111-111111111111", "known");
    let path = dir.join(".ai-app-studio/project.json");
    let before = fs::read(&path).unwrap();
    let ImportOutcome::Imported { project } = env.svc.import(&dir).unwrap() else {
        panic!()
    };
    assert_eq!(project.id, "11111111-1111-4111-8111-111111111111");
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn import_refuses_newer_schema_and_malformed_json_without_touching_them() {
    let env = Env::new();
    for (name, content) in [("newer", r#"{"schema":2,"id":"x"}"#), ("bad", "{ nope")] {
        let dir = env.folder(name);
        fs::create_dir_all(dir.join(".ai-app-studio")).unwrap();
        let path = dir.join(".ai-app-studio/project.json");
        fs::write(&path, content).unwrap();
        let err = env.svc.import(&dir).unwrap_err();
        assert!(matches!(err, ProjectError::Meta(_)), "{err}");
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
    }
    let newer = env
        .svc
        .import(&env.root.path().join("elsewhere/newer"))
        .unwrap_err();
    assert!(newer.to_string().contains("newer Studio"));
    assert!(env.db.list_projects().unwrap().is_empty());
}

#[test]
fn import_rejects_files_and_missing_paths() {
    let env = Env::new();
    let file = env.folder("f").join("a.txt");
    fs::write(&file, "x").unwrap();
    assert!(matches!(
        env.svc.import(&file),
        Err(ProjectError::BadFolder(_))
    ));
    assert!(matches!(
        env.svc.import(&env.root.path().join("nope")),
        Err(ProjectError::BadFolder(_))
    ));
}

#[test]
fn known_id_with_missing_old_folder_offers_relink_then_relinks() {
    let env = Env::new();
    let p = env.svc.create(&env.req("Moved", "moved")).unwrap();
    let old = PathBuf::from(&p.workspace_path);
    let new = env.folder("new-home");
    fs::remove_dir(&new).unwrap();
    fs::rename(&old, &new).unwrap();

    let ImportOutcome::NeedsRelink { existing, new_path } = env.svc.import(&new).unwrap() else {
        panic!()
    };
    assert_eq!(existing.id, p.id);
    assert_eq!(existing.state, ProjectState::MissingWorkspace);
    assert_eq!(Path::new(&new_path), new);

    let relinked = env.svc.relink(&p.id, &new).unwrap();
    assert_eq!(relinked.id, p.id);
    assert_eq!(Path::new(&relinked.workspace_path), new);
    assert_eq!(relinked.state, ProjectState::Ok);
}

#[test]
fn relink_refuses_a_folder_of_another_project() {
    let env = Env::new();
    let a = env.svc.create(&env.req("A", "a")).unwrap();
    let b = env.svc.create(&env.req("B", "b")).unwrap();
    assert!(env.svc.relink(&a.id, Path::new(&b.workspace_path)).is_err());
    assert!(matches!(
        env.svc.relink("nope", Path::new(&a.workspace_path)),
        Err(ProjectError::NotFound)
    ));
}

#[test]
fn known_id_with_existing_old_folder_is_a_copy_and_gets_new_identity() {
    let env = Env::new();
    let orig = env.svc.create(&env.req("My App", "my-app")).unwrap();
    let copy_dir = env.folder("copy");
    fs::remove_dir(&copy_dir).unwrap();
    template::copy_template(Path::new(&orig.workspace_path), &copy_dir).unwrap();
    let copy_dir = copy_dir.canonicalize().unwrap();

    let ImportOutcome::NeedsCopyConfirm { existing, .. } = env.svc.import(&copy_dir).unwrap()
    else {
        panic!()
    };
    assert_eq!(existing.id, orig.id);

    let copy = env.svc.import_as_copy(&copy_dir).unwrap();
    assert_ne!(copy.id, orig.id);
    assert_eq!(copy.slug, "my-app-2");
    assert_eq!(copy.app_id, "com.aiappstudio.myapp2");
    assert_ne!(copy.app_id, orig.app_id);

    let meta = ProjectMeta::read(&copy_dir).unwrap().unwrap();
    assert_eq!(
        (meta.id.as_str(), meta.app_id.as_str()),
        (copy.id.as_str(), copy.app_id.as_str())
    );
    assert_eq!(json(&copy_dir.join(TAURI_CONF))["identifier"], copy.app_id);
    assert_eq!(copy.state, ProjectState::Ok);
    // The original is untouched.
    let orig_conf = json(&Path::new(&orig.workspace_path).join(TAURI_CONF));
    assert_eq!(orig_conf["identifier"], orig.app_id);
    assert_eq!(
        env.db.get_project(&copy.id).unwrap().unwrap().pending_op,
        None
    );
    assert_eq!(env.svc.list().unwrap().len(), 2);
}

// ---- list ----

#[test]
fn list_reports_missing_workspace_and_metadata_mismatch() {
    let env = Env::new();
    let a = env.svc.create(&env.req("Alpha", "alpha")).unwrap();
    let b = env.svc.create(&env.req("Beta", "beta")).unwrap();
    let c = env.svc.create(&env.req("Gamma", "gamma")).unwrap();

    fs::remove_dir_all(&a.workspace_path).unwrap();
    write_conf(Path::new(&b.workspace_path), "Beta", "org.changed.beta");
    let conf_c = Path::new(&c.workspace_path).join(TAURI_CONF);
    template::edit_json(&conf_c, |v| {
        v["productName"] = "Renamed".into();
        Ok(())
    })
    .unwrap();

    let list = env.svc.list().unwrap();
    assert_eq!(list[0].state, ProjectState::MissingWorkspace);
    let ProjectState::MetadataMismatch { detail } = &list[1].state else {
        panic!("{:?}", list[1])
    };
    assert!(detail.contains("identifier") && !detail.contains("name"));
    let ProjectState::MetadataMismatch { detail } = &list[2].state else {
        panic!()
    };
    assert!(detail.contains("name"));
    // project.json wins: the cached name is not rewritten from tauri.conf.json.
    assert_eq!(list[2].name, "Gamma");
}

// ---- crash recovery ----

fn pending(env: &Env, id: &str, slug: &str, path: &Path, op: PendingOp) {
    env.db
        .insert_project(&NewProject {
            id: id.into(),
            name: slug.into(),
            slug: slug.into(),
            template_id: TemplateId::TauriVue,
            app_id: "com.x.y".into(),
            workspace_path: path.to_string_lossy().into_owned(),
            pending_op: Some(op),
        })
        .unwrap();
}

#[test]
fn reconcile_create_crashed_before_rename_removes_temp_and_row() {
    let env = Env::new();
    let target = env.parent().join("half");
    let temp = env.parent().join(format!("{TEMP_PREFIX}id-1"));
    fs::create_dir_all(temp.join(".ai-app-studio")).unwrap();
    fs::create_dir(&target).unwrap(); // the user-picked empty folder
    pending(&env, "id-1", "half", &target, PendingOp::Create);
    assert!(
        env.svc.list().unwrap().is_empty(),
        "pending rows are hidden"
    );

    env.svc.reconcile_on_startup().unwrap();
    assert!(!temp.exists());
    assert!(target.is_dir(), "user's folder is never removed");
    assert!(env.db.list_projects().unwrap().is_empty());
}

#[test]
fn reconcile_create_crashed_after_rename_clears_pending() {
    let env = Env::new();
    let target = env.parent().join("done");
    fs::create_dir(&target).unwrap();
    write_meta(&target, "id-2", "done");
    pending(&env, "id-2", "done", &target, PendingOp::Create);

    env.svc.reconcile_on_startup().unwrap();
    assert_eq!(
        env.db.get_project("id-2").unwrap().unwrap().pending_op,
        None
    );
    assert_eq!(env.svc.list().unwrap().len(), 1);
}

#[test]
fn reconcile_copy_with_old_json_drops_row_and_with_new_json_clears_pending() {
    let env = Env::new();
    let old = env.folder("copy-old");
    write_meta(&old, "original-id", "copy-old");
    pending(&env, "new-id-a", "copy-old-2", &old, PendingOp::Copy);

    let done = env.folder("copy-done");
    write_meta(&done, "new-id-b", "copy-done");
    pending(&env, "new-id-b", "copy-done", &done, PendingOp::Copy);

    env.svc.reconcile_on_startup().unwrap();
    assert!(env.db.get_project("new-id-a").unwrap().is_none());
    assert_eq!(
        env.db.get_project("new-id-b").unwrap().unwrap().pending_op,
        None
    );
    // project.json of the half-copied folder still names the original project.
    assert_eq!(ProjectMeta::read(&old).unwrap().unwrap().id, "original-id");
}

#[test]
fn reconcile_ignores_settled_rows() {
    let env = Env::new();
    let p = env.svc.create(&env.req("Stable", "stable")).unwrap();
    env.svc.reconcile_on_startup().unwrap();
    assert_eq!(env.svc.list().unwrap(), vec![p]);
}
