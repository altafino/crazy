//! GhService against a fake `gh` script (the way M4 used fake programs) and real temp git repos.
//! The script answers each `gh <a> <b>` from files in a control directory (`<a>_<b>.out|err|code|
//! sleep`), logs its arguments to `calls.log` and the `GIT_*` environment it got to `env.log`.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tempfile::TempDir;
use tokio::sync::watch;

use super::*;
use crate::domain::ImportOutcome;
use crate::domain::TemplateId;
use crate::persistence::NewProject;
use crate::services::git::{GitRunner, SnapshotStore};
use crate::services::projects::{GitCmd, ProjectService};

const SCRIPT: &str = r#"#!/bin/sh
D="$FAKE_GH_DIR"
echo "$*" >> "$D/calls.log"
env | grep -E '^GIT_' | sort > "$D/env.log"
k="$1_$2"
if [ "$k" = repo_clone ]; then mkdir -p "$4" && echo hi > "$4/README.md"; fi
[ -f "$D/$k.out" ] && cat "$D/$k.out"
[ -f "$D/$k.err" ] && cat "$D/$k.err" >&2
[ -f "$D/$k.sleep" ] && sleep "$(cat "$D/$k.sleep")"
exit "$(cat "$D/$k.code" 2>/dev/null || echo 0)"
"#;

const SIGNED_IN: &str = r#"{"hosts":{"github.com":[{"state":"success","active":true,"host":"github.com","login":"octo","tokenSource":"keyring","scopes":"repo","gitProtocol":"ssh"}]}}"#;
const LOGIN_TEXT: &str = "! One-time code (ABCD-1234) copied to clipboard\nOpen this URL to continue in your web browser: https://github.com/login/device\n";

struct Fx {
    tmp: TempDir,
    fake: PathBuf,
    env: Vec<(String, String)>,
    db: Arc<Db>,
    svc: GhService,
    repo: RepoService,
}

impl Fx {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let fake = root.join("fake");
        fs::create_dir_all(&fake).unwrap();
        let gh = root.join("gh");
        fs::write(&gh, SCRIPT).unwrap();
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
        let global = root.join("gitconfig");
        fs::write(&global, "").unwrap();
        let env: Vec<(String, String)> = vec![
            ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
            ("HOME".into(), root.display().to_string()),
            ("GIT_CONFIG_GLOBAL".into(), global.display().to_string()),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            ("FAKE_GH_DIR".into(), fake.display().to_string()),
        ];
        let db = Arc::new(Db::open_in_memory().unwrap());
        let store = Arc::new(SnapshotStore::new(
            &root.join("data"),
            GitRunner {
                program: "git".into(),
                env: env.clone(),
            },
        ));
        let repo = RepoService::new("git".into(), env.clone(), db.clone(), store);
        let svc = GhService::new(Some(gh), "git".into(), env.clone(), db.clone());
        Self {
            tmp,
            fake,
            env,
            db,
            svc,
            repo,
        }
    }

    fn root(&self) -> PathBuf {
        self.tmp.path().canonicalize().unwrap()
    }

    fn set(&self, file: &str, text: &str) {
        fs::write(self.fake.join(file), text).unwrap();
    }

    fn calls(&self) -> String {
        fs::read_to_string(self.fake.join("calls.log")).unwrap_or_default()
    }

    fn git(&self, dir: &Path, args: &[&str]) {
        let o = Command::new("git")
            .args(["-c", "user.name=T", "-c", "user.email=t@example.com"])
            .args(args)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    }

    /// A repo with one commit.
    fn repo_dir(&self, name: &str) -> PathBuf {
        let d = self.root().join(name);
        fs::create_dir_all(&d).unwrap();
        self.git(&d, &["init", "-q", "--initial-branch=main"]);
        fs::write(d.join("a.txt"), "one\n").unwrap();
        self.git(&d, &["add", "-A"]);
        self.git(&d, &["commit", "-q", "-m", "first"]);
        d
    }

    fn project(&self, id: &str, dir: &Path) {
        self.db
            .insert_project(&NewProject {
                id: id.into(),
                name: id.into(),
                slug: id.into(),
                template_id: TemplateId::Existing,
                app_id: "com.x.a".into(),
                workspace_path: dir.display().to_string(),
                pending_op: None,
            })
            .unwrap();
    }

    async fn create(
        &self,
        dir: &Path,
        name: &str,
        vis: Visibility,
        d: Option<&str>,
    ) -> Result<RemoteCreated, GhError> {
        self.project("p1", dir);
        self.svc
            .create_repo(&self.repo, "p1", dir, name, vis, d)
            .await
    }

    fn parent(&self) -> PathBuf {
        let p = self.root().join("parent");
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn projects(&self) -> ProjectService {
        ProjectService::new(
            self.db.clone(),
            self.root().join("tpl"),
            self.root().join("data"),
            GitCmd {
                program: "git".into(),
                env: self.env.clone(),
            },
        )
    }
}

fn never() -> (watch::Sender<bool>, watch::Receiver<bool>) {
    watch::channel(false)
}

// ------------------------------------------------------------------ auth

#[tokio::test]
async fn auth_state_table() {
    let fx = Fx::new();
    let cases: [(&str, AuthState); 4] = [
        (
            SIGNED_IN,
            AuthState::SignedIn {
                login: "octo".into(),
                git_protocol: "ssh".into(),
            },
        ),
        (r#"{"hosts":{}}"#, AuthState::SignedOut),
        (
            r#"{"hosts":{"github.com":[{"state":"error","active":true,"host":"github.com","error":"Get \"https://api.github.com/\": dial tcp: connection refused"}]}}"#,
            AuthState::Unreachable {
                message: "Get \"https://api.github.com/\": dial tcp: connection refused".into(),
            },
        ),
        (
            r#"{"hosts":{"github.com":[{"state":"error","active":true,"host":"github.com","error":"HTTP 401: Bad credentials (https://api.github.com/)"}]}}"#,
            AuthState::TokenInvalid {
                message: "HTTP 401: Bad credentials (https://api.github.com/)".into(),
            },
        ),
    ];
    for (json, want) in cases {
        fx.set("auth_status.out", json);
        assert_eq!(fx.svc.auth_status().await, want, "{json}");
    }
    let missing = GhService::new(None, "git".into(), fx.env.clone(), fx.db.clone());
    assert_eq!(missing.auth_status().await, AuthState::GhMissing);
    let gone = GhService::new(
        Some(fx.root().join("nope")),
        "git".into(),
        fx.env.clone(),
        fx.db.clone(),
    );
    assert_eq!(gone.auth_status().await, AuthState::GhMissing);
}

#[tokio::test]
async fn auth_error_text_is_redacted() {
    let fx = Fx::new();
    fx.set(
        "auth_status.out",
        r#"{"hosts":{"github.com":[{"state":"error","error":"HTTP 401 for token ghp_abcDEF123456 rejected"}]}}"#,
    );
    let AuthState::TokenInvalid { message } = fx.svc.auth_status().await else {
        panic!()
    };
    assert!(!message.contains("ghp_abcDEF123456"), "{message}");
}

// ------------------------------------------------------------------ login

#[tokio::test]
async fn login_emits_code_once_and_rereads_status() {
    let fx = Fx::new();
    fx.set("auth_login.err", LOGIN_TEXT);
    fx.set("auth_status.out", SIGNED_IN);
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = seen.clone();
    let (_tx, rx) = never();
    let state = fx
        .svc
        .login(move |c| sink.lock().unwrap().push(c), rx)
        .await
        .unwrap();
    assert_eq!(
        state,
        AuthState::SignedIn {
            login: "octo".into(),
            git_protocol: "ssh".into()
        }
    );
    assert_eq!(
        *seen.lock().unwrap(),
        vec![LoginCode {
            code: "ABCD-1234".into(),
            url: "https://github.com/login/device".into()
        }]
    );
    let calls = fx.calls();
    assert!(
        calls
            .contains("auth login --web --hostname github.com --git-protocol https --skip-ssh-key"),
        "{calls}"
    );
    assert!(calls.contains("auth status"), "{calls}");
}

#[tokio::test]
async fn login_stop_kills_the_process_group() {
    let fx = Fx::new();
    fx.set("auth_login.err", LOGIN_TEXT);
    fx.set("auth_login.sleep", "30");
    let (tx, rx) = never();
    let (got_tx, got_rx) = tokio::sync::oneshot::channel();
    let started = std::time::Instant::now();
    let stopper = async {
        got_rx.await.unwrap();
        tx.send(true).unwrap();
    };
    let (res, ()) = tokio::join!(
        fx.svc.login(
            move |c| {
                let _ = got_tx.send(c);
            },
            rx
        ),
        stopper
    );
    assert_eq!(res, Err(GhError::Stopped));
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[tokio::test]
async fn login_with_unparseable_output_is_a_typed_error() {
    let fx = Fx::new();
    fx.set("auth_login.err", "something odd with ghp_abcDEF123456\n");
    let called = Arc::new(AtomicUsize::new(0));
    let c = called.clone();
    let (_tx, rx) = never();
    let err = fx
        .svc
        .login(
            move |_| {
                c.fetch_add(1, Ordering::SeqCst);
            },
            rx,
        )
        .await
        .unwrap_err();
    let GhError::Gh { message } = err else {
        panic!("{err:?}")
    };
    assert!(message.contains("something odd"), "{message}");
    assert!(!message.contains("ghp_abcDEF123456"), "{message}");
    assert_eq!(called.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn login_failure_exit_is_an_error() {
    let fx = Fx::new();
    fx.set(
        "auth_login.err",
        format!("{LOGIN_TEXT}failed to authenticate\n").as_str(),
    );
    fx.set("auth_login.code", "1");
    let (_tx, rx) = never();
    let err = fx.svc.login(|_| {}, rx).await.unwrap_err();
    assert!(
        matches!(err, GhError::Gh { ref message } if message.contains("failed to authenticate")),
        "{err:?}"
    );
}

// ------------------------------------------------------------------ list

#[tokio::test]
async fn list_repos_parses_and_sorts_newest_first() {
    let fx = Fx::new();
    fx.set(
        "repo_list.out",
        r#"[{"nameWithOwner":"o/old","isPrivate":true,"description":"","updatedAt":"2026-01-01T00:00:00Z"},
            {"nameWithOwner":"o/new","isPrivate":false,"description":"fresh","updatedAt":"2026-10-01T00:00:00Z"}]"#,
    );
    let repos = fx.svc.list_repos(500).await.unwrap();
    assert_eq!(
        repos,
        vec![
            RepoSummary {
                name_with_owner: "o/new".into(),
                private: false,
                description: Some("fresh".into()),
                updated_at: "2026-10-01T00:00:00Z".into()
            },
            RepoSummary {
                name_with_owner: "o/old".into(),
                private: true,
                description: None,
                updated_at: "2026-01-01T00:00:00Z".into()
            },
        ]
    );
    assert!(
        fx.calls()
            .contains("repo list --json nameWithOwner,isPrivate,description,updatedAt --limit 100")
    );
}

#[tokio::test]
async fn list_repos_signed_out_is_typed() {
    let fx = Fx::new();
    fx.set(
        "repo_list.err",
        "To get started with GitHub CLI, please run:  gh auth login\n",
    );
    fx.set("repo_list.code", "4");
    assert_eq!(fx.svc.list_repos(10).await, Err(GhError::SignedOut));
}

// ------------------------------------------------------------------ create

#[tokio::test]
async fn create_refuses_a_folder_that_is_not_a_repo() {
    let fx = Fx::new();
    let d = fx.root().join("plain");
    fs::create_dir_all(&d).unwrap();
    assert_eq!(
        fx.create(&d, "x", Visibility::Private, None).await,
        Err(GhError::NotARepo)
    );
    assert_eq!(fx.calls(), "");
}

#[tokio::test]
async fn create_refuses_a_repo_without_commits() {
    let fx = Fx::new();
    let d = fx.root().join("empty");
    fs::create_dir_all(&d).unwrap();
    fx.git(&d, &["init", "-q", "--initial-branch=main"]);
    assert_eq!(
        fx.create(&d, "x", Visibility::Private, None).await,
        Err(GhError::NoCommits)
    );
}

#[tokio::test]
async fn create_refuses_a_subfolder_of_a_larger_repo() {
    let fx = Fx::new();
    let top = fx.repo_dir("mono");
    let sub = top.join("apps/one");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("f"), "x").unwrap();
    let err = fx
        .create(&sub, "x", Visibility::Private, None)
        .await
        .unwrap_err();
    assert_eq!(
        err,
        GhError::InsideLargerRepo {
            top: top.display().to_string()
        }
    );
    assert_eq!(fx.calls(), "");
}

#[tokio::test]
async fn create_refuses_when_origin_exists_and_redacts_its_url() {
    let fx = Fx::new();
    let d = fx.repo_dir("has-origin");
    fx.git(
        &d,
        &[
            "remote",
            "add",
            "origin",
            "https://user:s3cretpass@github.com/o/r.git",
        ],
    );
    let err = fx
        .create(&d, "x", Visibility::Private, None)
        .await
        .unwrap_err();
    let GhError::OriginExists { url } = err else {
        panic!("{err:?}")
    };
    assert!(
        url.contains("github.com/o/r.git") && !url.contains("s3cretpass"),
        "{url}"
    );
    assert_eq!(fx.calls(), "");
}

#[tokio::test]
async fn create_validates_the_name_before_anything_else() {
    let fx = Fx::new();
    let d = fx.repo_dir("v");
    for bad in ["", "has space", "a/b", "..", "x.git", &"a".repeat(101)] {
        let r = fx
            .svc
            .create_repo(&fx.repo, "p1", &d, bad, Visibility::Private, None)
            .await;
        assert!(
            matches!(r, Err(GhError::InvalidName { .. })),
            "{bad:?}: {r:?}"
        );
    }
    assert!(validate_repo_name("My_app-2.0").is_ok());
    assert_eq!(fx.calls(), "");
}

#[tokio::test]
async fn create_private_runs_gh_with_the_right_flags_and_records_the_sync() {
    let fx = Fx::new();
    let d = fx.repo_dir("happy");
    fx.set("repo_create.out", "https://github.com/octo/my-app\n");
    let made = fx
        .create(&d, "my-app", Visibility::Private, None)
        .await
        .unwrap();
    assert_eq!(
        made,
        RemoteCreated {
            name_with_owner: "octo/my-app".into(),
            url: "https://github.com/octo/my-app".into(),
            visibility: Visibility::Private
        }
    );
    let calls = fx.calls();
    assert!(
        calls.contains(&format!(
            "repo create my-app --private --source {} --remote origin --push",
            d.display()
        )),
        "{calls}"
    );
    assert!(!calls.contains("--public") && !calls.contains("--description"));
    let sync = fx.db.get_sync("p1").unwrap().unwrap();
    assert!(sync.ok && sync.operation == SyncOp::Create);
    assert!(
        sync.message
            .unwrap()
            .contains("https://github.com/octo/my-app")
    );
}

#[tokio::test]
async fn create_public_with_description() {
    let fx = Fx::new();
    let d = fx.repo_dir("pub");
    fx.set("repo_create.out", "https://github.com/octo/pub\n");
    fx.create(&d, "pub", Visibility::Public, Some("  my notes app "))
        .await
        .unwrap();
    let calls = fx.calls();
    assert!(
        calls.contains("repo create pub --public --description my notes app --source"),
        "{calls}"
    );
}

#[tokio::test]
async fn create_name_taken_and_secrets_are_redacted_in_git_sync() {
    let fx = Fx::new();
    let d = fx.repo_dir("taken");
    fx.set(
        "repo_create.err",
        "GraphQL: Name already exists on this account (createRepository)\n",
    );
    fx.set("repo_create.code", "1");
    assert_eq!(
        fx.create(&d, "taken", Visibility::Private, None).await,
        Err(GhError::NameTaken)
    );
    assert!(!fx.db.get_sync("p1").unwrap().unwrap().ok);

    fx.set(
        "repo_create.err",
        "boom with token ghp_abcDEF123456 inside\n",
    );
    let err = fx
        .svc
        .create_repo(&fx.repo, "p1", &d, "taken", Visibility::Private, None)
        .await
        .unwrap_err();
    let GhError::Gh { message } = err else {
        panic!("{err:?}")
    };
    assert!(
        message.contains("boom") && !message.contains("ghp_abcDEF123456"),
        "{message}"
    );
    let stored = fx.db.get_sync("p1").unwrap().unwrap().message.unwrap();
    assert!(!stored.contains("ghp_abcDEF123456"), "{stored}");
}

#[tokio::test]
async fn create_gets_the_network_environment() {
    let fx = Fx::new();
    let d = fx.repo_dir("env");
    fx.set("repo_create.out", "https://github.com/octo/env\n");
    fx.create(&d, "env", Visibility::Private, None)
        .await
        .unwrap();
    let env = fs::read_to_string(fx.fake.join("env.log")).unwrap();
    assert!(env.contains("GIT_TERMINAL_PROMPT=0"), "{env}");
    assert!(
        env.contains("GIT_SSH_COMMAND=ssh -o BatchMode=yes"),
        "{env}"
    );
    assert!(env.contains("GIT_CONFIG_COUNT=3"), "{env}");
    assert!(
        env.contains("GIT_CONFIG_KEY_0=credential.https://github.com.helper"),
        "{env}"
    );
    assert!(env.contains("auth git-credential"), "{env}");
    assert!(
        env.contains("GIT_CONFIG_VALUE_1=1000")
            && env.contains("GIT_CONFIG_KEY_2=http.lowSpeedTime"),
        "{env}"
    );
}

/// The mechanism itself: real git reads the config pairs from the environment.
#[test]
fn git_honours_the_config_env_pairs() {
    let fx = Fx::new();
    let env = fx.svc.network_cmd_env();
    let o = Command::new("git")
        .args(["config", "--get", "credential.https://github.com.helper"])
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .output()
        .unwrap();
    let got = String::from_utf8_lossy(&o.stdout);
    assert!(
        got.trim().starts_with("!'") && got.contains("auth git-credential"),
        "{got}"
    );
    let o = Command::new("git")
        .args(["config", "--get", "http.lowSpeedLimit"])
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "1000");
}

// ------------------------------------------------------------------ clone

#[test]
fn repo_specs() {
    let ok = |s: &str| parse_repo_spec(s).unwrap();
    let want = ("stefan".to_owned(), "quick.notes".to_owned());
    for s in [
        "stefan/quick.notes",
        " stefan/quick.notes ",
        "https://github.com/stefan/quick.notes",
        "https://github.com/stefan/quick.notes.git",
        "https://github.com/stefan/quick.notes/",
        "git@github.com:stefan/quick.notes.git",
        "github.com/stefan/quick.notes",
    ] {
        assert_eq!(ok(s), want, "{s}");
    }
    for bad in [
        "",
        "stefan",
        "a/b/c",
        "https://gitlab.com/a/b",
        "-x y/z",
        "o/..",
        "o/na me",
        "/x",
    ] {
        assert_eq!(
            parse_repo_spec(bad),
            Err(GhError::InvalidRepoSpec),
            "{bad:?}"
        );
    }
}

fn entries(dir: &Path) -> Vec<String> {
    let mut v: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[tokio::test]
async fn clone_renames_the_temp_and_imports() {
    let fx = Fx::new();
    let parent = fx.parent();
    let (_tx, rx) = never();
    let outcome = fx
        .projects()
        .clone_and_import(&fx.svc, "https://github.com/octo/hello.git", &parent, rx)
        .await
        .unwrap();
    let ImportOutcome::Imported { project } = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(
        project.workspace_path,
        parent.join("hello").display().to_string()
    );
    assert_eq!(entries(&parent), ["hello"]);
    assert!(parent.join("hello/README.md").is_file());
    let calls = fx.calls();
    assert!(
        calls.contains("repo clone octo/hello ") && calls.contains(".ai-app-studio-clone-"),
        "{calls}"
    );
    let env = fs::read_to_string(fx.fake.join("env.log")).unwrap();
    assert!(
        env.contains("GIT_TERMINAL_PROMPT=0") && env.contains("GIT_CONFIG_COUNT=3"),
        "{env}"
    );
}

#[tokio::test]
async fn clone_failure_leaves_no_temp_folder() {
    let fx = Fx::new();
    let parent = fx.parent();
    fx.set(
        "repo_clone.err",
        "GraphQL: Could not resolve to a Repository with the name 'octo/nope'.\n",
    );
    fx.set("repo_clone.code", "1");
    let (_tx, rx) = never();
    let err = fx.svc.clone("octo/nope", &parent, rx).await.unwrap_err();
    assert!(
        matches!(err, GhError::Gh { ref message } if message.contains("Could not resolve")),
        "{err:?}"
    );
    assert!(entries(&parent).is_empty(), "{:?}", entries(&parent));
}

#[tokio::test]
async fn clone_stop_kills_gh_and_removes_the_temp_folder() {
    let fx = Fx::new();
    let parent = fx.parent();
    fx.set("repo_clone.sleep", "30");
    let (tx, rx) = never();
    let watcher = async {
        for _ in 0..200 {
            if entries(&parent)
                .iter()
                .any(|e| e.starts_with(CLONE_TEMP_PREFIX))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        tx.send(true).unwrap();
    };
    let started = std::time::Instant::now();
    let (res, ()) = tokio::join!(fx.svc.clone("octo/slow", &parent, rx), watcher);
    assert_eq!(res, Err(GhError::Stopped));
    assert!(entries(&parent).is_empty());
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[tokio::test]
async fn clone_refuses_an_existing_target_untouched() {
    let fx = Fx::new();
    let parent = fx.parent();
    fs::create_dir_all(parent.join("hello")).unwrap();
    fs::write(parent.join("hello/mine.txt"), "mine").unwrap();
    let (_tx, rx) = never();
    let err = fx.svc.clone("octo/hello", &parent, rx).await.unwrap_err();
    assert_eq!(
        err,
        GhError::TargetExists {
            path: parent.join("hello").display().to_string()
        }
    );
    assert_eq!(
        fs::read_to_string(parent.join("hello/mine.txt")).unwrap(),
        "mine"
    );
    assert_eq!(fx.calls(), "");
}

#[tokio::test]
async fn clone_without_gh_and_with_a_bad_spec() {
    let fx = Fx::new();
    let parent = fx.parent();
    let missing = GhService::new(None, "git".into(), fx.env.clone(), fx.db.clone());
    let (_tx, rx) = never();
    assert_eq!(
        missing.clone("o/r", &parent, rx.clone()).await,
        Err(GhError::GhMissing)
    );
    assert_eq!(
        fx.svc.clone("not a repo", &parent, rx).await,
        Err(GhError::InvalidRepoSpec)
    );
}

#[test]
fn startup_cleanup_removes_only_clone_temp_folders() {
    let fx = Fx::new();
    let parent = fx.parent();
    for d in [
        ".ai-app-studio-clone-aaa",
        "keep",
        ".ai-app-studio-creating-bbb",
    ] {
        fs::create_dir_all(parent.join(d)).unwrap();
        fs::write(parent.join(d).join("f"), "x").unwrap();
    }
    fx.projects().remove_stale_clone_temps(&parent);
    assert_eq!(entries(&parent), [".ai-app-studio-creating-bbb", "keep"]);
}

// ------------------------------------------------------------------ real GitHub (M5-D14)

/// Opt-in: `STUDIO_GITHUB_E2E=1 cargo test github_e2e -- --nocapture`. Creates a private repo
/// under the signed-in account; the token cannot delete it, so delete it by hand.
#[tokio::test]
async fn github_e2e() {
    if std::env::var("STUDIO_GITHUB_E2E").as_deref() != Ok("1") {
        eprintln!("skipped: set STUDIO_GITHUB_E2E=1 to run against github.com");
        return;
    }
    let env = crate::platform::process_env();
    let find = |bin: &str| {
        let path = env
            .iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        path.split(':')
            .map(|d| Path::new(d).join(bin))
            .find(|p| p.is_file())
    };
    let gh = find("gh").expect("gh on PATH");
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let db = Arc::new(Db::open_in_memory().unwrap());
    let store = Arc::new(SnapshotStore::new(
        &root.join("data"),
        GitRunner {
            program: "git".into(),
            env: env.clone(),
        },
    ));
    let repo = RepoService::new("git".into(), env.clone(), db.clone(), store);
    let svc = GhService::new(Some(gh.clone()), "git".into(), env.clone(), db.clone());
    assert!(
        matches!(svc.auth_status().await, AuthState::SignedIn { .. }),
        "sign in with gh first"
    );

    let git = |dir: &Path, args: &[&str], network: bool| {
        let mut cmd = Command::new("git");
        let env_set = if network {
            network_env(&env, false)
        } else {
            env.clone()
        };
        if network {
            cmd.args(network_config_args(Some(&gh)));
        }
        let o = cmd
            .args(["-c", "user.name=E2E", "-c", "user.email=e2e@example.com"])
            .args(args)
            .env_clear()
            .envs(env_set.iter().map(|(k, v)| (k, v)))
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    };
    let work = root.join("work");
    fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "--initial-branch=main"], false);
    fs::write(work.join("a.txt"), "one\n").unwrap();
    git(&work, &["add", "-A"], false);
    git(&work, &["commit", "-q", "-m", "first"], false);
    db.insert_project(&NewProject {
        id: "e2e".into(),
        name: "e2e".into(),
        slug: "e2e".into(),
        template_id: TemplateId::Existing,
        app_id: "com.x.e2e".into(),
        workspace_path: work.display().to_string(),
        pending_op: None,
    })
    .unwrap();

    let name = format!(
        "studio-e2e-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let made = svc
        .create_repo(
            &repo,
            "e2e",
            &work,
            &name,
            Visibility::Private,
            Some("AI App Studio e2e test"),
        )
        .await
        .unwrap();
    println!("created {} (delete it by hand)", made.url);

    fs::write(work.join("b.txt"), "two\n").unwrap();
    git(&work, &["add", "-A"], false);
    git(&work, &["commit", "-q", "-m", "second"], false);
    git(&work, &["push", "origin", "HEAD"], true);

    let parent = root.join("parent");
    fs::create_dir_all(&parent).unwrap();
    let projects = ProjectService::new(
        db.clone(),
        root.join("tpl"),
        root.join("data"),
        GitCmd {
            program: "git".into(),
            env: env.clone(),
        },
    );
    let (_tx, rx) = never();
    let outcome = projects
        .clone_and_import(&svc, &made.name_with_owner, &parent, rx)
        .await
        .unwrap();
    assert!(
        matches!(outcome, ImportOutcome::Imported { .. }),
        "{outcome:?}"
    );
    let cloned = parent.join(&name);
    for f in ["a.txt", "b.txt"] {
        assert_eq!(
            fs::read(work.join(f)).unwrap(),
            fs::read(cloned.join(f)).unwrap(),
            "{f}"
        );
    }
    println!("e2e ok: {}", made.url);
}
