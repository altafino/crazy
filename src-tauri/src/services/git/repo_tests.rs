//! RepoService against real temp repos (PRD M3 steps 3 and 5).
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;

use super::repo::*;
use super::{GitRunner, SnapshotStore};
use crate::domain::TemplateId;
use crate::persistence::{Db, JobState, NewProject, SessionMode, SessionRow};

struct Fx {
    tmp: TempDir,
    env: Vec<(String, String)>,
    db: Arc<Db>,
    svc: RepoService,
}

fn env_for(tmp: &Path, identity: bool) -> Vec<(String, String)> {
    let global = tmp.join("gitconfig");
    if identity {
        fs::write(
            &global,
            "[user]\n\tname = Test User\n\temail = test@example.com\n",
        )
        .unwrap();
    } else {
        fs::write(&global, "").unwrap();
    }
    let home = tmp.join("home");
    fs::create_dir_all(&home).unwrap();
    vec![
        ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
        ("HOME".into(), home.display().to_string()),
        ("GIT_CONFIG_GLOBAL".into(), global.display().to_string()),
        ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
    ]
}

fn fx_with(identity: bool) -> Fx {
    let tmp = TempDir::new().unwrap();
    let env = env_for(tmp.path(), identity);
    let db = Arc::new(Db::open_in_memory().unwrap());
    let store = Arc::new(SnapshotStore::new(
        &tmp.path().join("data"),
        GitRunner {
            program: "git".into(),
            env: env.clone(),
        },
    ));
    let svc = RepoService::new("git".into(), env.clone(), db.clone(), store);
    Fx { tmp, env, db, svc }
}

fn fx() -> Fx {
    fx_with(true)
}

impl Fx {
    /// A new folder under the temp root.
    fn dir(&self, name: &str) -> PathBuf {
        let d = self.tmp.path().join(name);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let o = self.git_raw(dir, args);
        assert!(
            o.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).into_owned()
    }

    fn git_raw(&self, dir: &Path, args: &[&str]) -> std::process::Output {
        Command::new("git")
            .args(["-c", "user.name=Fixture", "-c", "user.email=fx@example.com"])
            .args(args)
            .current_dir(dir)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .output()
            .unwrap()
    }

    /// Repo with `files` committed on main.
    fn repo(&self, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let d = self.dir(name);
        self.git(&d, &["init", "-q", "--initial-branch=main"]);
        for (p, c) in files {
            write(&d, p, c);
        }
        if !files.is_empty() {
            self.git(&d, &["add", "-A"]);
            self.git(&d, &["commit", "-q", "-m", "first"]);
        }
        d
    }

    fn add_project(&self, id: &str, dir: &Path) {
        self.db
            .insert_project(&NewProject {
                id: id.into(),
                name: id.into(),
                slug: id.into(),
                template_id: TemplateId::TauriVue,
                app_id: "com.x.a".into(),
                workspace_path: dir.display().to_string(),
                pending_op: None,
            })
            .unwrap();
    }

    fn live_changes_run(&self, id: &str) {
        self.add_project(id, Path::new("/nonexistent"));
        self.db.insert_job("j1", id, JobState::Running).unwrap();
        self.db
            .insert_session(&SessionRow {
                id: "s1".into(),
                job_id: "j1".into(),
                project_id: id.into(),
                profile_id: None,
                mode: SessionMode::Changes,
                prompt: "x".into(),
                provider_session_id: None,
                resumed_from: None,
                log_path: "/l".into(),
                before_ref: None,
                after_ref: None,
                created_at: String::new(),
            })
            .unwrap();
    }

    fn hook(&self, repo: &Path, name: &str, body: &str) {
        let p = repo.join(".git/hooks").join(name);
        fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
}

fn write(dir: &Path, rel: &str, content: &str) {
    let p = dir.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, content).unwrap();
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| (*x).to_owned()).collect()
}

fn entry<'a>(st: &'a RepoStatus, path: &str) -> &'a StatusEntry {
    st.entries
        .iter()
        .find(|e| e.path == path)
        .unwrap_or_else(|| {
            panic!(
                "no entry {path}: {:?}",
                st.entries.iter().map(|e| &e.path).collect::<Vec<_>>()
            )
        })
}

async fn status(f: &Fx, d: &Path) -> RepoStatus {
    f.svc.status("p1", d, &[]).await.unwrap()
}

fn tracked(f: &Fx, d: &Path) -> Vec<String> {
    f.git(d, &["ls-files"]).lines().map(str::to_owned).collect()
}

// ---- parser ---------------------------------------------------------------

#[test]
fn parser_reads_every_record_type() {
    let h = "1 .M N... 100644 100644 100644 aaaa bbbb ";
    let mut b = Vec::new();
    for rec in [
        "# branch.oid abcdef1234567890",
        "# branch.head main",
        &format!("{h}src/a b.txt"),
        "1 A. N... 000000 100644 100644 0000 bbbb new.txt",
        "1 .D N... 100644 100644 000000 aaaa bbbb gone.txt",
        "1 .M S.M. 160000 160000 160000 aaaa bbbb sub",
        "2 R. N... 100644 100644 100644 aaaa bbbb R100 renamed.txt",
        "old name.txt",
        "u UU N... 100644 100644 100644 100644 aaaa bbbb cccc conflict.txt",
        "? loose.txt",
        "? dir/new\nline.txt",
    ] {
        b.extend_from_slice(rec.as_bytes());
        b.push(0);
    }
    b.extend_from_slice(b"? bad-\xff\xfe.bin\0");
    let p = parse_status(&b);
    let mut st = vec![];
    for e in &p.entries {
        st.push(e.to_entry(""));
    }
    let by = |n: &str| {
        st.iter()
            .find(|e| e.path == n)
            .unwrap_or_else(|| panic!("{n}"))
    };
    assert!(matches!(p.head, HeadState::Branch { name } if name == "main"));
    assert_eq!(by("src/a b.txt").kind, EntryKind::Modified);
    assert!(!by("src/a b.txt").staged && by("src/a b.txt").unstaged);
    assert_eq!(by("new.txt").kind, EntryKind::Added);
    assert!(by("new.txt").staged);
    assert_eq!(by("gone.txt").kind, EntryKind::Deleted);
    assert!(by("sub").submodule);
    let r = by("renamed.txt");
    assert_eq!(
        (r.kind, r.old_path.as_deref()),
        (EntryKind::Renamed, Some("old name.txt"))
    );
    assert_eq!(by("conflict.txt").kind, EntryKind::Conflicted);
    assert_eq!(by("loose.txt").kind, EntryKind::Untracked);
    assert_eq!(by("dir/new\nline.txt").kind, EntryKind::Untracked);
    let bad = st.iter().find(|e| e.non_utf8).expect("non-utf8 entry");
    assert!(bad.path.starts_with("bad-"));
}

#[test]
fn parser_reads_unborn_and_detached_heads() {
    let unborn = parse_status(b"# branch.oid (initial)\0# branch.head trunk\0");
    assert!(matches!(unborn.head, HeadState::Unborn { name } if name == "trunk"));
    let det = parse_status(b"# branch.oid 0123456789abcdef\0# branch.head (detached)\0");
    assert!(matches!(det.head, HeadState::Detached { short } if short == "0123456"));
}

// ---- status ---------------------------------------------------------------

#[tokio::test]
async fn not_a_repo_is_a_typed_error() {
    let f = fx();
    let d = f.dir("plain");
    assert_eq!(
        f.svc.status("p1", &d, &[]).await.unwrap_err(),
        SourceError::NotARepo
    );
    assert_eq!(f.svc.history(&d).await.unwrap_err(), SourceError::NotARepo);
}

#[tokio::test]
async fn status_reports_every_kind_with_counts_and_last_commit() {
    let f = fx();
    let d = f.repo(
        "r",
        &[
            ("a.txt", "1\n2\n3\n"),
            ("b.txt", "x\n"),
            ("c.txt", "keep\n"),
            ("old.txt", "same\nsame\nsame\n"),
        ],
    );
    write(&d, "a.txt", "1\n2\n3\n4\n5\n");
    fs::remove_file(d.join("b.txt")).unwrap();
    f.git(&d, &["mv", "old.txt", "new.txt"]);
    write(&d, "fresh.txt", "l1\nl2\n");
    write(&d, "bin.dat", "a\0b");
    let st = status(&f, &d).await;
    assert_eq!(
        st.head,
        HeadState::Branch {
            name: "main".into()
        }
    );
    let a = entry(&st, "a.txt");
    assert_eq!(
        (a.kind, a.additions, a.deletions),
        (EntryKind::Modified, Some(2), Some(0))
    );
    assert_eq!(entry(&st, "b.txt").kind, EntryKind::Deleted);
    let n = entry(&st, "new.txt");
    assert_eq!(
        (n.kind, n.old_path.as_deref(), n.staged),
        (EntryKind::Renamed, Some("old.txt"), true)
    );
    let u = entry(&st, "fresh.txt");
    assert_eq!((u.kind, u.additions), (EntryKind::Untracked, Some(2)));
    assert!(entry(&st, "bin.dat").binary);
    assert_eq!(st.total, 5);
    assert!(!st.truncated);
    let last = st.last_commit.unwrap();
    assert_eq!(last.subject, "first");
    assert_eq!(last.hash.len(), 40);
    assert!(last.time > 1_000_000_000);
}

#[tokio::test]
async fn unborn_repo_status_and_empty_history() {
    let f = fx();
    let d = f.repo("u", &[]);
    write(&d, "a.txt", "hi\n");
    f.git(&d, &["add", "a.txt"]);
    write(&d, "b.txt", "yo\n");
    let st = status(&f, &d).await;
    assert_eq!(
        st.head,
        HeadState::Unborn {
            name: "main".into()
        }
    );
    assert!(st.last_commit.is_none());
    let a = entry(&st, "a.txt");
    assert_eq!((a.kind, a.additions), (EntryKind::Added, Some(1)));
    assert_eq!(entry(&st, "b.txt").kind, EntryKind::Untracked);
    assert!(f.svc.history(&d).await.unwrap().is_empty());
}

#[tokio::test]
async fn detached_head_is_reported() {
    let f = fx();
    let d = f.repo("d", &[("a", "1\n")]);
    f.git(&d, &["checkout", "-q", "--detach"]);
    let st = status(&f, &d).await;
    assert!(matches!(st.head, HeadState::Detached { .. }));
    assert!(matches!(
        f.svc.branches(&d).await.unwrap().head,
        HeadState::Detached { .. }
    ));
}

#[tokio::test]
async fn awkward_paths_survive_newlines_and_non_utf8() {
    let f = fx();
    let d = f.repo("w", &[("keep", "1\n")]);
    write(&d, "we ird\nname.txt", "x\n");
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        fs::write(d.join(std::ffi::OsStr::from_bytes(b"bad-\xff.txt")), "y").unwrap();
    }
    let st = status(&f, &d).await;
    assert!(entry(&st, "we ird\nname.txt").kind == EntryKind::Untracked);
    #[cfg(unix)]
    {
        let bad = st
            .entries
            .iter()
            .find(|e| e.non_utf8)
            .expect("lossy entry flagged");
        let err = f
            .svc
            .commit(&d, "m", std::slice::from_ref(&bad.path))
            .await
            .unwrap_err();
        assert!(matches!(err, SourceError::Invalid { .. }), "{err:?}");
    }
    f.svc
        .commit(&d, "odd name", &s(&["we ird\nname.txt"]))
        .await
        .unwrap();
    assert!(tracked(&f, &d).iter().any(|p| p.contains("we ird")));
}

#[tokio::test]
async fn status_is_capped_at_5000_with_the_total() {
    let f = fx();
    let d = f.repo("big", &[("a", "1\n")]);
    for i in 0..5010 {
        write(&d, &format!("many/f{i}.txt"), "x\n");
    }
    let st = status(&f, &d).await;
    assert_eq!(st.entries.len(), 5000);
    assert_eq!(st.total, 5010);
    assert!(st.truncated);
}

#[tokio::test]
async fn status_counts_staged_entries_outside_the_ticked_set() {
    let f = fx();
    let d = f.repo("s", &[("a", "1\n"), ("b", "1\n")]);
    write(&d, "a", "2\n");
    write(&d, "b", "2\n");
    f.git(&d, &["add", "b"]);
    let none = f.svc.status("p1", &d, &[]).await.unwrap();
    assert_eq!(none.staged_outside, 1);
    let ticked = f.svc.status("p1", &d, &s(&["b"])).await.unwrap();
    assert_eq!(ticked.staged_outside, 0);
}

// ---- layouts (M3-D26) -----------------------------------------------------

#[tokio::test]
async fn monorepo_subfolder_is_scoped_to_the_project_folder() {
    let f = fx();
    let top = f.repo("mono", &[("app/a.txt", "1\n"), ("other/o.txt", "1\n")]);
    write(&top, "app/a.txt", "2\n");
    write(&top, "app/new/n.txt", "n\n");
    write(&top, "other/o.txt", "2\n");
    let app = top.join("app");
    let st = status(&f, &app).await;
    assert!(st.location.is_subfolder);
    assert_eq!(st.location.prefix, "app/");
    let paths: Vec<_> = st.entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["a.txt", "new/n.txt"]);
    assert_eq!(st.total, 2);

    let d = f.svc.file_diff(&app, "a.txt", None, false).await.unwrap();
    assert_eq!(d.hunks[0].lines.len(), 2);

    f.svc
        .commit(&app, "app only", &s(&["a.txt", "new/n.txt"]))
        .await
        .unwrap();
    // The sibling folder's edit is untouched and history is scoped to the folder.
    assert!(
        f.git(&top, &["status", "--porcelain"])
            .contains("other/o.txt")
    );
    let hist = f.svc.history(&app).await.unwrap();
    assert_eq!(
        hist.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(),
        ["app only", "first"]
    );
    assert!(f.svc.branches(&app).await.unwrap().affects_whole_repo);
    assert!(!f.svc.branches(&top).await.unwrap().affects_whole_repo);
}

#[tokio::test]
async fn linked_worktree_works_and_is_flagged() {
    let f = fx();
    let main = f.repo("main-wt", &[("a", "1\n")]);
    let wt = f.tmp.path().join("linked");
    f.git(
        &main,
        &["worktree", "add", "-q", "-b", "side", wt.to_str().unwrap()],
    );
    write(&wt, "a", "2\n");
    let st = status(&f, &wt).await;
    assert!(st.location.is_linked);
    assert_eq!(
        st.head,
        HeadState::Branch {
            name: "side".into()
        }
    );
    assert!(Path::new(&st.location.common_dir).ends_with(".git"));
    f.svc.commit(&wt, "in worktree", &s(&["a"])).await.unwrap();
    assert_eq!(
        f.git(&wt, &["log", "-1", "--format=%s"]).trim(),
        "in worktree"
    );
}

#[tokio::test]
async fn submodule_layouts() {
    let f = fx();
    let sub = f.repo("subsrc", &[("s.txt", "1\n")]);
    let top = f.repo("outer", &[("a", "1\n")]);
    f.git(
        &top,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            sub.to_str().unwrap(),
            "vendor",
        ],
    );
    f.git(&top, &["commit", "-q", "-m", "add sub"]);
    write(&top, "vendor/s.txt", "2\n");
    let st = status(&f, &top).await;
    let e = entry(&st, "vendor");
    assert!(e.submodule && e.unstaged, "{e:?}");
    // A project that lives inside the submodule resolves to the submodule's own repo.
    let inner = status(&f, &top.join("vendor")).await;
    assert!(!inner.location.is_subfolder);
    assert!(inner.location.is_linked);
    assert_eq!(entry(&inner, "s.txt").kind, EntryKind::Modified);
}

// ---- diff -----------------------------------------------------------------

#[tokio::test]
async fn file_diff_covers_tracked_untracked_binary_and_unborn() {
    let f = fx();
    let d = f.repo("df", &[("a.txt", "one\ntwo\n"), ("img.bin", "a\0b")]);
    write(&d, "a.txt", "one\nTWO\n");
    write(&d, "n.txt", "x\ny\n");
    write(&d, "img.bin", "a\0b\0c");
    let t = f.svc.file_diff(&d, "a.txt", None, false).await.unwrap();
    assert_eq!(t.hunks.len(), 1);
    assert!(t.hunks[0].lines.iter().any(|l| l.text == "TWO"));
    let u = f.svc.file_diff(&d, "n.txt", None, false).await.unwrap();
    assert!(
        u.hunks[0]
            .lines
            .iter()
            .all(|l| l.kind == super::LineKind::Added)
    );
    assert_eq!(u.hunks[0].lines.len(), 2);
    let b = f.svc.file_diff(&d, "img.bin", None, false).await.unwrap();
    assert!(b.binary);
    assert_eq!((b.old_size, b.new_size), (Some(3), Some(5)));
    assert!(matches!(
        f.svc.file_diff(&d, "../x", None, false).await.unwrap_err(),
        SourceError::Invalid { .. }
    ));

    let un = f.repo("dfu", &[]);
    write(&un, "a", "1\n");
    f.git(&un, &["add", "a"]);
    let x = f.svc.file_diff(&un, "a", None, false).await.unwrap();
    assert_eq!(x.hunks[0].lines.len(), 1);
}

#[tokio::test]
async fn file_diff_follows_a_staged_rename() {
    let f = fx();
    let d = f.repo("rn", &[("old.txt", "a\nb\nc\nd\n")]);
    f.git(&d, &["mv", "old.txt", "new.txt"]);
    write(&d, "new.txt", "a\nb\nc\nD\n");
    let x = f
        .svc
        .file_diff(&d, "new.txt", Some("old.txt"), false)
        .await
        .unwrap();
    assert_eq!(
        x.hunks[0]
            .lines
            .iter()
            .filter(|l| l.kind != super::LineKind::Context)
            .count(),
        2
    );
}

// ---- commit ---------------------------------------------------------------

#[tokio::test]
async fn commit_takes_exactly_the_ticked_paths() {
    let f = fx();
    let d = f.repo("c", &[("a", "1\n"), ("b", "1\n"), ("other", "1\n")]);
    write(&d, "a", "2\n");
    write(&d, "b", "2\n");
    write(&d, "other", "2\n");
    write(&d, "fresh.txt", "new\n");
    write(&d, "unticked.txt", "no\n");
    f.git(&d, &["add", "other"]); // pre-staged, not ticked
    let out = f
        .svc
        .commit(&d, "  my message  ", &s(&["a", "fresh.txt"]))
        .await
        .unwrap();
    assert_eq!((out.files, out.commit.subject.as_str()), (2, "my message"));
    let committed: Vec<_> = f
        .git(&d, &["show", "--name-only", "--format=", "HEAD"])
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(committed, ["a", "fresh.txt"]);
    // The other staged file stays staged, b and unticked.txt stay out.
    assert!(f.git(&d, &["diff", "--cached", "--name-only"]).trim() == "other");
    let st = status(&f, &d).await;
    assert_eq!(entry(&st, "b").kind, EntryKind::Modified);
    assert_eq!(entry(&st, "unticked.txt").kind, EntryKind::Untracked);
}

#[tokio::test]
async fn commit_in_an_unborn_repo_makes_the_first_commit() {
    let f = fx();
    let d = f.repo("first", &[]);
    write(&d, "a", "1\n");
    write(&d, "b", "1\n");
    f.svc.commit(&d, "init", &s(&["a"])).await.unwrap();
    assert_eq!(tracked(&f, &d), ["a"]);
    assert_eq!(f.svc.history(&d).await.unwrap().len(), 1);
}

#[tokio::test]
async fn commit_with_a_rename_passes_old_and_new() {
    let f = fx();
    let d = f.repo("cr", &[("old.txt", "x\ny\n"), ("z", "1\n")]);
    f.git(&d, &["mv", "old.txt", "new.txt"]);
    // Unstaged rename: the old name is a deletion, the new one untracked.
    let d2 = f.repo("cr2", &[("old.txt", "x\ny\n")]);
    fs::rename(d2.join("old.txt"), d2.join("new.txt")).unwrap();
    f.svc.commit(&d, "mv", &s(&["new.txt"])).await.unwrap();
    assert_eq!(tracked(&f, &d), ["new.txt", "z"]);
    f.svc
        .commit(&d2, "mv", &s(&["new.txt", "old.txt"]))
        .await
        .unwrap();
    assert_eq!(tracked(&f, &d2), ["new.txt"]);
}

#[tokio::test]
async fn commit_validates_input() {
    let f = fx();
    let d = f.repo("v", &[("a", "1\n")]);
    write(&d, "a", "2\n");
    for (msg, paths) in [
        ("  ", s(&["a"])),
        ("m", vec![]),
        ("m", s(&["clean-nope"])),
        ("m", s(&["../a"])),
        ("m", s(&[".git/config"])),
    ] {
        let e = f.svc.commit(&d, msg, &paths).await.unwrap_err();
        assert!(
            matches!(e, SourceError::Invalid { .. }),
            "{msg:?} {paths:?} -> {e:?}"
        );
    }
}

#[tokio::test]
async fn failing_hook_rejects_and_unstages_only_what_we_added() {
    let f = fx();
    let d = f.repo("h", &[("a", "1\n"), ("staged", "1\n")]);
    write(&d, "a", "2\n");
    write(&d, "staged", "2\n");
    write(&d, "fresh.txt", "x\n");
    f.git(&d, &["add", "staged"]);
    f.hook(
        &d,
        "pre-commit",
        "echo 'lint failed: no semicolons' >&2\nexit 1",
    );
    let e = f
        .svc
        .commit(&d, "m", &s(&["a", "fresh.txt"]))
        .await
        .unwrap_err();
    match e {
        SourceError::HookRejected { output } => assert!(output.contains("lint failed"), "{output}"),
        other => panic!("{other:?}"),
    }
    let st = status(&f, &d).await;
    assert_eq!(
        entry(&st, "fresh.txt").kind,
        EntryKind::Untracked,
        "our git add was undone"
    );
    assert!(
        entry(&st, "staged").staged,
        "the user's own staging is untouched"
    );
    assert_eq!(f.git(&d, &["log", "--oneline"]).lines().count(), 1);
}

#[tokio::test]
async fn sleeping_hook_times_out_and_cleans_up() {
    let f = fx();
    let d = f.repo("t", &[("a", "1\n")]);
    write(&d, "a", "2\n");
    write(&d, "fresh.txt", "x\n");
    f.hook(&d, "pre-commit", "sleep 30");
    let svc = RepoService::new(
        "git".into(),
        f.env.clone(),
        f.db.clone(),
        Arc::new(SnapshotStore::new(
            &f.tmp.path().join("d2"),
            GitRunner {
                program: "git".into(),
                env: f.env.clone(),
            },
        )),
    )
    .with_commit_timeout(Duration::from_millis(600));
    let t = std::time::Instant::now();
    let e = svc
        .commit(&d, "m", &s(&["a", "fresh.txt"]))
        .await
        .unwrap_err();
    assert_eq!(e, SourceError::CommitTimedOut);
    assert!(t.elapsed() < Duration::from_secs(15));
    assert!(
        !d.join(".git/index.lock").exists(),
        "git released its own lock on SIGTERM"
    );
    let st = status(&f, &d).await;
    assert_eq!(entry(&st, "fresh.txt").kind, EntryKind::Untracked);
}

#[tokio::test]
async fn held_index_lock_is_reported_and_never_deleted() {
    let f = fx();
    let d = f.repo("l", &[("a", "1\n")]);
    write(&d, "a", "2\n");
    write(&d, "fresh.txt", "x\n");
    let lock = d.join(".git/index.lock");
    fs::write(&lock, "").unwrap();
    assert_eq!(
        f.svc.commit(&d, "m", &s(&["a"])).await.unwrap_err(),
        SourceError::RepoLocked
    );
    assert_eq!(
        f.svc.commit(&d, "m", &s(&["fresh.txt"])).await.unwrap_err(),
        SourceError::RepoLocked
    );
    assert!(lock.exists(), "the lock file is never deleted");
    // Reading status still works while the lock is held.
    assert_eq!(status(&f, &d).await.total, 2);
}

#[tokio::test]
async fn missing_identity_is_typed_and_set_identity_writes_local_config_only() {
    let f = fx_with(false);
    let d = f.repo("i", &[]);
    write(&d, "a", "1\n");
    assert_eq!(
        f.svc.commit(&d, "m", &s(&["a"])).await.unwrap_err(),
        SourceError::IdentityMissing
    );
    assert_eq!(
        f.svc.identity(&d).await.unwrap(),
        IdentityInfo {
            name: None,
            email: None
        }
    );
    assert!(matches!(
        f.svc
            .set_identity(
                &d,
                &Identity {
                    name: "".into(),
                    email: "x@y".into()
                }
            )
            .await
            .unwrap_err(),
        SourceError::Invalid { .. }
    ));
    f.svc
        .set_identity(
            &d,
            &Identity {
                name: "Ada L".into(),
                email: "ada@example.com".into(),
            },
        )
        .await
        .unwrap();
    assert!(
        fs::read_to_string(d.join(".git/config"))
            .unwrap()
            .contains("ada@example.com")
    );
    assert!(
        fs::read_to_string(f.tmp.path().join("gitconfig"))
            .unwrap()
            .is_empty(),
        "global config untouched"
    );
    let out = f.svc.commit(&d, "m", &s(&["a"])).await.unwrap();
    assert_eq!(out.commit.author, "Ada L");
}

#[tokio::test]
async fn conflicts_are_reported_and_block_commit() {
    let f = fx();
    let d = f.repo("m", &[("a", "base\n"), ("b", "1\n")]);
    f.git(&d, &["checkout", "-q", "-b", "side"]);
    write(&d, "a", "side\n");
    f.git(&d, &["commit", "-qam", "side"]);
    f.git(&d, &["checkout", "-q", "main"]);
    write(&d, "a", "main\n");
    f.git(&d, &["commit", "-qam", "main"]);
    let _ = f.git_raw(&d, &["merge", "side"]);
    write(&d, "b", "2\n");
    let st = status(&f, &d).await;
    assert_eq!(st.conflicts, 1);
    assert_eq!(entry(&st, "a").kind, EntryKind::Conflicted);
    assert_eq!(st.entries[0].path, "a", "conflicts sort first");
    assert_eq!(
        f.svc.commit(&d, "m", &s(&["b"])).await.unwrap_err(),
        SourceError::ConflictsPresent
    );
    assert_eq!(
        f.svc.restore("p1", &d, &s(&["a"])).await.unwrap_err(),
        SourceError::ConflictsPresent
    );
    assert_eq!(
        f.svc.switch_branch("p1", &d, "side").await.unwrap_err(),
        SourceError::ConflictsPresent
    );
}

// ---- secrets (M3-D17) -----------------------------------------------------

#[test]
fn secret_pattern_table() {
    let ghp = format!("ghp_{}", "a1B2".repeat(9));
    let pat = format!("github_pat_{}", "Ab1_".repeat(8));
    let ant = format!("sk-ant-api03-{}", "x9Y".repeat(10));
    let sk = format!("sk-{}", "a1B2c3D4".repeat(6));
    let positives = [
        ("-----BEGIN RSA PRIVATE KEY-----", "private key"),
        ("-----BEGIN OPENSSH PRIVATE KEY-----", "private key"),
        ("-----BEGIN PRIVATE KEY-----", "private key"),
        ("key = AKIAIOSFODNN7EXAMPLE", "AWS access key"),
        (&format!("token: {ghp}"), "GitHub token"),
        (&format!("GITHUB={pat}"), "GitHub token"),
        (&format!("ANTHROPIC_API_KEY=\"{ant}\""), "Anthropic key"),
        (&format!("OPENAI={sk}"), "API key"),
        ("slack xoxb-123456789012-abcdefghij", "Slack token"),
        (
            "g = \"AIzaSyA1234567890abcdefghijklmnopqrstuv\"",
            "Google API key",
        ),
    ];
    for (line, kind) in positives {
        let hits = scan_line(line);
        assert!(hits.iter().any(|(k, _)| k == kind), "{line} -> {hits:?}");
    }
    let negatives = [
        "a task-force-assessment-2024-with-a-long-hyphenated-name",
        "risk-assessmentabcdefghijklmnopqrstuvwxyz0123",
        "const desk = 'sk-learn'",
        "AKIA_NOT_A_KEY and AKIAIOSFODNN7EXAMPLEX",
        "ghp_tooshort",
        "github_pat_short",
        "xoxo-hugs-and-kisses",
        "-----BEGIN CERTIFICATE-----",
        "the AIza prefix alone",
        "skill-name-that-is-long-enough-to-trip-a-naive-check-abcdef",
        "let x = 'sk-' + id;",
    ];
    for line in negatives {
        assert!(
            scan_line(line).is_empty(),
            "{line} -> {:?}",
            scan_line(line)
        );
    }
    // Matches are masked, the key header is not a secret by itself.
    let (_, masked) = &scan_line(&format!("x={ghp}"))[0];
    assert!(
        !masked.contains(&ghp) && masked.starts_with("ghp_"),
        "{masked}"
    );
}

#[tokio::test]
async fn secret_check_finds_files_and_added_lines_without_blocking() {
    let f = fx();
    let d = f.repo("sc", &[("a.txt", "fine\n"), ("conf.txt", "old\n")]);
    write(&d, "a.txt", "fine\nAWS=AKIAIOSFODNN7EXAMPLE\n");
    write(&d, ".env", "X=1\n");
    write(&d, "new.txt", "line1\n-----BEGIN RSA PRIVATE KEY-----\n");
    write(&d, "conf.txt", "old\nclean\n");
    let found = f
        .svc
        .secret_check(&d, &s(&["a.txt", ".env", "new.txt", "conf.txt"]))
        .await
        .unwrap();
    let key = |p: &str, l: Option<u32>| found.iter().any(|x| x.path == p && x.line == l);
    assert!(key("a.txt", Some(2)), "{found:?}");
    assert!(key(".env", None));
    assert!(key("new.txt", Some(2)));
    assert!(!found.iter().any(|x| x.path == "conf.txt"));
    assert!(
        found
            .iter()
            .find(|x| x.path == "a.txt")
            .unwrap()
            .masked
            .starts_with("AKIA")
    );
    // Only ticked files are scanned, and old (unchanged) lines are not.
    write(&d, "a.txt", "fine\nAWS=AKIAIOSFODNN7EXAMPLE\nmore\n");
    assert!(
        f.svc
            .secret_check(&d, &s(&["conf.txt"]))
            .await
            .unwrap()
            .is_empty()
    );
}

// ---- branches (M3-D2) -----------------------------------------------------

#[tokio::test]
async fn create_branch_keeps_files_and_switch_needs_a_clean_tree() {
    let f = fx();
    let d = f.repo("br", &[("a", "1\n")]);
    write(&d, "a", "2\n");
    let b = f.svc.create_branch(&d, "feature/x").await.unwrap();
    assert_eq!(
        b.head,
        HeadState::Branch {
            name: "feature/x".into()
        }
    );
    assert_eq!(b.branches, ["feature/x", "main"]);
    assert_eq!(
        fs::read_to_string(d.join("a")).unwrap(),
        "2\n",
        "files kept"
    );
    assert_eq!(
        f.svc.create_branch(&d, "main").await.unwrap_err(),
        SourceError::BranchExists
    );
    for bad in ["", "-x", "a b", "x..y", "x.lock"] {
        assert_eq!(
            f.svc.create_branch(&d, bad).await.unwrap_err(),
            SourceError::InvalidBranchName,
            "{bad}"
        );
    }
    assert_eq!(
        f.svc.switch_branch("p1", &d, "main").await.unwrap_err(),
        SourceError::DirtyTree { count: 1 }
    );
    assert_eq!(
        f.svc.switch_branch("p1", &d, "nope").await.unwrap_err(),
        SourceError::BranchNotFound
    );
    f.svc.commit(&d, "c", &s(&["a"])).await.unwrap();
    let b = f.svc.switch_branch("p1", &d, "main").await.unwrap();
    assert_eq!(
        b.head,
        HeadState::Branch {
            name: "main".into()
        }
    );
    assert_eq!(fs::read_to_string(d.join("a")).unwrap(), "1\n");
}

#[tokio::test]
async fn studio_refs_are_never_listed_and_unborn_can_branch() {
    let f = fx();
    let d = f.repo("sr", &[("a", "1\n")]);
    f.git(&d, &["update-ref", "refs/studio/restore/last", "HEAD"]);
    assert_eq!(f.svc.branches(&d).await.unwrap().branches, ["main"]);
    let u = f.repo("sr2", &[]);
    let b = f.svc.create_branch(&u, "dev").await.unwrap();
    assert_eq!(b.head, HeadState::Unborn { name: "dev".into() });
}

#[tokio::test]
async fn history_is_the_last_50_commits() {
    let f = fx();
    let d = f.repo("hist", &[("a", "0\n")]);
    for i in 1..=54 {
        write(&d, "a", &format!("{i}\n"));
        f.git(&d, &["commit", "-qam", &format!("c{i}")]);
    }
    let h = f.svc.history(&d).await.unwrap();
    assert_eq!(h.len(), 50);
    assert_eq!(
        (h[0].subject.as_str(), h[0].author.as_str()),
        ("c54", "Fixture")
    );
    assert_eq!(h[0].short.len(), 7);
}

// ---- restore (M3-D11, D31) ------------------------------------------------

#[tokio::test]
async fn restore_snapshots_first_and_undo_brings_everything_back() {
    let f = fx();
    let d = f.repo(
        "rs",
        &[("a", "orig\n"), ("gone", "keep me\n"), ("other", "o\n")],
    );
    f.add_project("p1", &d);
    write(&d, "a", "edited\n");
    write(&d, "other", "edited too\n");
    fs::remove_file(d.join("gone")).unwrap();
    write(&d, "fresh.txt", "brand new\n");
    let rep = f
        .svc
        .restore("p1", &d, &s(&["a", "gone", "fresh.txt"]))
        .await
        .unwrap();
    assert_eq!(rep.restored.len(), 3);
    assert!(rep.cannot_undo.is_empty());
    assert_eq!(fs::read_to_string(d.join("a")).unwrap(), "orig\n");
    assert_eq!(fs::read_to_string(d.join("gone")).unwrap(), "keep me\n");
    assert!(!d.join("fresh.txt").exists());
    assert_eq!(
        fs::read_to_string(d.join("other")).unwrap(),
        "edited too\n",
        "unticked file untouched"
    );
    assert!(status(&f, &d).await.undo_restore_available);

    f.svc.undo_restore("p1", &d).await.unwrap();
    assert_eq!(fs::read_to_string(d.join("a")).unwrap(), "edited\n");
    assert!(!d.join("gone").exists());
    assert_eq!(
        fs::read_to_string(d.join("fresh.txt")).unwrap(),
        "brand new\n"
    );
    assert!(!status(&f, &d).await.undo_restore_available);
    assert_eq!(
        f.svc.undo_restore("p1", &d).await.unwrap_err(),
        SourceError::NoUndo
    );
}

#[tokio::test]
async fn only_the_latest_restore_can_be_undone() {
    let f = fx();
    let d = f.repo("keep1", &[("a", "orig\n"), ("b", "orig\n")]);
    f.add_project("p1", &d);
    write(&d, "a", "A1\n");
    write(&d, "b", "B1\n");
    f.svc.restore("p1", &d, &s(&["a"])).await.unwrap();
    f.svc.restore("p1", &d, &s(&["b"])).await.unwrap();
    f.svc.undo_restore("p1", &d).await.unwrap();
    assert_eq!(fs::read_to_string(d.join("b")).unwrap(), "B1\n");
    assert_eq!(
        fs::read_to_string(d.join("a")).unwrap(),
        "orig\n",
        "the first restore is gone for good"
    );
}

#[tokio::test]
async fn restore_reports_what_undo_cannot_bring_back() {
    let f = fx();
    let d = f.repo("lost", &[("a", "1\n"), ("staged", "1\n")]);
    f.add_project("p1", &d);
    write(&d, "a", "2\n");
    write(&d, ".env", "SECRET=1\n");
    write(&d, "staged", "2\n");
    f.git(&d, &["add", "staged"]);
    write(&d, "huge.bin", "x");
    let f2 = fs::File::options()
        .write(true)
        .open(d.join("huge.bin"))
        .unwrap();
    f2.set_len(51 * 1024 * 1024).unwrap();
    let paths = s(&["a", ".env", "staged", "huge.bin"]);
    let preview = f.svc.restore_preview(&d, &paths).await.unwrap();
    assert_eq!(
        fs::read_to_string(d.join("a")).unwrap(),
        "2\n",
        "preview changes nothing"
    );
    let rep = f.svc.restore("p1", &d, &paths).await.unwrap();
    assert_eq!(preview, rep.cannot_undo);
    let has = |p: &str, r: LostReason| rep.cannot_undo.iter().any(|c| c.path == p && c.reason == r);
    assert!(has(".env", LostReason::Secret), "{:?}", rep.cannot_undo);
    assert!(has("huge.bin", LostReason::TooLarge));
    assert!(has("staged", LostReason::StagedState));
    assert!(!rep.cannot_undo.iter().any(|c| c.path == "a"));
    assert_eq!(
        f.git(&d, &["diff", "--cached", "--name-only"]).trim(),
        "",
        "index restored too"
    );
}

#[tokio::test]
async fn restore_handles_staged_new_files_and_renames() {
    let f = fx();
    let d = f.repo("rr", &[("old.txt", "x\n")]);
    f.add_project("p1", &d);
    f.git(&d, &["mv", "old.txt", "new.txt"]);
    write(&d, "added.txt", "a\n");
    f.git(&d, &["add", "added.txt"]);
    f.svc
        .restore("p1", &d, &s(&["new.txt", "added.txt"]))
        .await
        .unwrap();
    assert!(
        d.join("old.txt").exists() && !d.join("new.txt").exists() && !d.join("added.txt").exists()
    );
    assert!(status(&f, &d).await.entries.is_empty());
}

#[tokio::test]
async fn restore_in_a_subfolder_project() {
    let f = fx();
    let top = f.repo("rsub", &[("app/a", "1\n")]);
    let app = top.join("app");
    f.add_project("p1", &app);
    write(&top, "app/a", "2\n");
    write(&top, "app/n/new", "n\n");
    f.svc
        .restore("p1", &app, &s(&["a", "n/new"]))
        .await
        .unwrap();
    assert_eq!(fs::read_to_string(app.join("a")).unwrap(), "1\n");
    f.svc.undo_restore("p1", &app).await.unwrap();
    assert_eq!(fs::read_to_string(app.join("n/new")).unwrap(), "n\n");
}

// ---- run lock (M3-D9, D30) ------------------------------------------------

#[tokio::test]
async fn a_live_changes_run_blocks_restore_switch_and_init_but_not_commit_or_branch() {
    let f = fx();
    let d = f.repo("lock", &[("a", "1\n")]);
    f.live_changes_run("p1");
    write(&d, "a", "2\n");
    assert_eq!(
        f.svc.restore("p1", &d, &s(&["a"])).await.unwrap_err(),
        SourceError::RunInProgress
    );
    assert_eq!(
        f.svc.undo_restore("p1", &d).await.unwrap_err(),
        SourceError::RunInProgress
    );
    assert_eq!(
        f.svc.switch_branch("p1", &d, "main").await.unwrap_err(),
        SourceError::RunInProgress
    );
    let plain = f.dir("plain-lock");
    let req = InitRequest {
        identity: None,
        confirm_secrets: false,
    };
    assert_eq!(
        f.svc.init_repo("p1", &plain, &req).await.unwrap_err(),
        SourceError::RunInProgress
    );
    assert!(!plain.join(".git").exists());
    f.svc.create_branch(&d, "side").await.unwrap();
    f.svc.commit(&d, "m", &s(&["a"])).await.unwrap();
}

// ---- initialize git (M3-D26) ----------------------------------------------

#[tokio::test]
async fn init_commits_current_files_with_the_given_identity() {
    let f = fx_with(false);
    let d = f.dir("init1");
    write(&d, "a.txt", "1\n");
    write(&d, "src/b.txt", "2\n");
    f.add_project("p1", &d);
    let none = InitRequest {
        identity: None,
        confirm_secrets: false,
    };
    assert_eq!(
        f.svc.init_repo("p1", &d, &none).await.unwrap_err(),
        SourceError::IdentityMissing
    );
    assert!(
        !d.join(".git").exists(),
        "nothing was created before the identity is known"
    );
    let req = InitRequest {
        identity: Some(Identity {
            name: "Eve".into(),
            email: "eve@example.com".into(),
        }),
        confirm_secrets: false,
    };
    let out = f.svc.init_repo("p1", &d, &req).await.unwrap();
    assert_eq!(out.commit.as_ref().unwrap().subject, "Initial commit");
    assert_eq!(out.commit.unwrap().author, "Eve");
    assert_eq!(tracked(&f, &d), ["a.txt", "src/b.txt"]);
    assert_eq!(
        f.git(&d, &["config", "--local", "user.email"]).trim(),
        "eve@example.com"
    );
    assert_eq!(
        status(&f, &d).await.head,
        HeadState::Branch {
            name: "main".into()
        }
    );
}

#[tokio::test]
async fn init_with_own_identity_and_an_empty_folder() {
    let f = fx();
    let d = f.dir("init-empty");
    let out = f
        .svc
        .init_repo(
            "p1",
            &d,
            &InitRequest {
                identity: None,
                confirm_secrets: false,
            },
        )
        .await
        .unwrap();
    assert!(out.commit.is_none());
    assert!(d.join(".git").exists());
}

#[tokio::test]
async fn init_is_refused_inside_another_repo() {
    let f = fx();
    let top = f.repo("outer-r", &[("a", "1\n")]);
    let sub = top.join("inner");
    fs::create_dir_all(&sub).unwrap();
    let req = InitRequest {
        identity: None,
        confirm_secrets: false,
    };
    let e = f.svc.init_repo("p1", &sub, &req).await.unwrap_err();
    let SourceError::AlreadyInRepo { top: t } = &e else {
        panic!("{e:?}")
    };
    assert!(Path::new(t).ends_with("outer-r"));
    assert!(
        e.to_string()
            .starts_with("This folder is already inside the Git repository at ")
    );
    assert!(!sub.join(".git").exists());
    // Also refused for a folder that is itself a repo root.
    assert!(matches!(
        f.svc.init_repo("p1", &top, &req).await.unwrap_err(),
        SourceError::AlreadyInRepo { .. }
    ));
}

#[tokio::test]
async fn init_warns_about_secrets_and_leaves_no_repo_until_confirmed() {
    let f = fx();
    let d = f.dir("init-secret");
    write(&d, "a.txt", "1\n");
    write(&d, ".env", "KEY=1\n");
    let req = InitRequest {
        identity: None,
        confirm_secrets: false,
    };
    let e = f.svc.init_repo("p1", &d, &req).await.unwrap_err();
    assert_eq!(
        e,
        SourceError::SecretsFound {
            paths: s(&[".env"])
        }
    );
    assert!(
        !d.join(".git").exists(),
        "the repo made for the check is removed again"
    );
    // An ignored .env is not a secret to warn about.
    write(&d, ".gitignore", ".env\n");
    f.svc.init_repo("p1", &d, &req).await.unwrap();
    assert!(!tracked(&f, &d).contains(&".env".to_owned()));

    let d2 = f.dir("init-secret2");
    write(&d2, ".env", "KEY=1\n");
    let ok = InitRequest {
        identity: None,
        confirm_secrets: true,
    };
    f.svc.init_repo("p1", &d2, &ok).await.unwrap();
    assert_eq!(tracked(&f, &d2), [".env"]);
}

#[tokio::test]
async fn init_failure_rolls_the_new_repo_back() {
    let f = fx();
    let d = f.dir("init-hook");
    write(&d, "a.txt", "1\n");
    // A template hook that rejects every commit makes the initial commit fail.
    let tpl = f.dir("tpl/hooks");
    let hook = tpl.join("pre-commit");
    fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut env = f.env.clone();
    env.push((
        "GIT_TEMPLATE_DIR".into(),
        f.tmp.path().join("tpl").display().to_string(),
    ));
    let svc = RepoService::new(
        "git".into(),
        env.clone(),
        f.db.clone(),
        Arc::new(SnapshotStore::new(
            &f.tmp.path().join("d3"),
            GitRunner {
                program: "git".into(),
                env,
            },
        )),
    );
    let e = svc
        .init_repo(
            "p1",
            &d,
            &InitRequest {
                identity: None,
                confirm_secrets: false,
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            e,
            SourceError::HookRejected { .. } | SourceError::Git { .. }
        ),
        "{e:?}"
    );
    assert!(!d.join(".git").exists());
}
