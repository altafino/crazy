use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use super::*;

struct Fx {
    _tmp: TempDir,
    data: PathBuf,
    proj: PathBuf,
    store: SnapshotStore,
}

fn env(home: &Path) -> Vec<(String, String)> {
    vec![
        ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
        ("HOME".into(), home.to_string_lossy().into_owned()),
        ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
        ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
    ]
}

fn fx() -> Fx {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("data");
    let proj = tmp.path().join("proj");
    fs::create_dir_all(&proj).unwrap();
    let store = SnapshotStore::new(
        &data,
        GitRunner {
            program: "git".into(),
            env: env(tmp.path()),
        },
    );
    Fx {
        _tmp: tmp,
        data,
        proj,
        store,
    }
}

fn user_git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn w(dir: &Path, rel: &str, body: &str) {
    let p = dir.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

fn snap(f: &Fx, run: &str, phase: Phase) -> String {
    f.store
        .snapshot("p1", &f.proj, run, phase, "t")
        .unwrap()
        .commit
}

fn find<'a>(v: &'a [FileChange], path: &str) -> &'a FileChange {
    v.iter()
        .find(|c| c.path == path)
        .unwrap_or_else(|| panic!("no {path}"))
}

#[test]
fn non_git_folder_snapshot_leaves_no_dot_git() {
    let f = fx();
    w(&f.proj, "a.txt", "hi\n");
    snap(&f, "r1", Phase::Before);
    assert!(!f.proj.join(".git").exists());
    assert!(f.data.join("snapshots/p1.git/HEAD").exists());
    // temp index is cleaned up
    let leftovers = fs::read_dir(f.data.join("snapshots/p1.git"))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("index")
        })
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
fn user_repo_is_untouched() {
    let f = fx();
    user_git(&f.proj, &["init", "-q", "-b", "main"]);
    w(&f.proj, "a.txt", "1\n");
    user_git(&f.proj, &["add", "."]);
    user_git(&f.proj, &["commit", "-qm", "init"]);
    w(&f.proj, "a.txt", "2\n");
    w(&f.proj, "new.txt", "n\n");
    user_git(&f.proj, &["add", "a.txt"]);
    let head = user_git(&f.proj, &["rev-parse", "HEAD"]);
    let status = user_git(&f.proj, &["status", "--porcelain"]);
    let branches = user_git(&f.proj, &["branch"]);
    let index = fs::read(f.proj.join(".git/index")).unwrap();
    snap(&f, "r1", Phase::Before);
    assert_eq!(head, user_git(&f.proj, &["rev-parse", "HEAD"]));
    assert_eq!(status, user_git(&f.proj, &["status", "--porcelain"]));
    assert_eq!(branches, user_git(&f.proj, &["branch"]));
    assert_eq!(index, fs::read(f.proj.join(".git/index")).unwrap());
}

#[test]
fn skips_are_recorded() {
    let f = fx();
    w(&f.proj, ".gitignore", "ignored.log\nbuild_out/\n");
    w(&f.proj, "ignored.log", "x");
    w(&f.proj, "build_out/x", "x");
    w(&f.proj, "node_modules/pkg/i.js", "x");
    w(&f.proj, "web/node_modules/pkg/i.js", "x");
    w(&f.proj, ".env", "SECRET=1");
    w(&f.proj, ".env.example", "SECRET=");
    w(&f.proj, "keys/id_rsa", "k");
    w(&f.proj, "src/main.rs", "fn main(){}");
    let big = fs::File::create(f.proj.join("big.bin")).unwrap();
    big.set_len(51 * 1024 * 1024).unwrap();
    let s = f
        .store
        .snapshot("p1", &f.proj, "r1", Phase::Before, "t")
        .unwrap();
    let has = |p: &str, r: SkipReason| s.skipped.iter().any(|x| x.path == p && x.reason == r);
    assert!(
        has("node_modules/", SkipReason::IgnoredDir),
        "{:?}",
        s.skipped
    );
    assert!(has("web/node_modules/", SkipReason::IgnoredDir));
    assert!(has(".env", SkipReason::Secret));
    assert!(has("keys/id_rsa", SkipReason::Secret));
    assert!(has("big.bin", SkipReason::TooLarge));
    assert!(!s.skipped.iter().any(|x| x.path == ".env.example"));
    let ctx = f.store.ctx("p1").unwrap();
    let tree = f
        .store
        .git
        .text(&ctx, &["ls-tree", "-r", "--name-only", &s.commit])
        .unwrap();
    let files: Vec<_> = tree.lines().collect();
    assert_eq!(
        files,
        vec![".env.example", ".gitignore", "src/main.rs"],
        "ignored file, build dir, secrets and big file must not be stored"
    );
    // readable back from the commit alone
    assert_eq!(f.store.skipped("p1", &s.commit).unwrap(), s.skipped);
}

#[test]
fn diff_edit_add_delete_rename_with_dirty_tree() {
    let f = fx();
    user_git(&f.proj, &["init", "-q"]);
    w(&f.proj, "edit.txt", "a\nb\nc\n");
    w(&f.proj, "del.txt", "gone\n");
    w(
        &f.proj,
        "old_name.txt",
        "line1\nline2\nline3\nline4\nline5\n",
    );
    w(&f.proj, "dirty.txt", "user wip\n"); // dirty before the run, untouched by it
    let before = snap(&f, "r1", Phase::Before);
    w(&f.proj, "edit.txt", "a\nB\nc\nd\n");
    w(&f.proj, "added.txt", "x\ny\n");
    fs::remove_file(f.proj.join("del.txt")).unwrap();
    fs::rename(f.proj.join("old_name.txt"), f.proj.join("new_name.txt")).unwrap();
    let after = snap(&f, "r1", Phase::After);
    let d = f.store.diff("p1", &before, &after).unwrap();
    assert_eq!(d.len(), 4, "{d:?}");
    let e = find(&d, "edit.txt");
    assert_eq!(
        (e.status, e.additions, e.deletions),
        (FileStatus::Modified, 2, 1)
    );
    let a = find(&d, "added.txt");
    assert_eq!(
        (a.status, a.additions, a.deletions),
        (FileStatus::Added, 2, 0)
    );
    let x = find(&d, "del.txt");
    assert_eq!(
        (x.status, x.additions, x.deletions),
        (FileStatus::Deleted, 0, 1)
    );
    let r = find(&d, "new_name.txt");
    assert_eq!(r.status, FileStatus::Renamed);
    assert_eq!(r.old_path.as_deref(), Some("old_name.txt"));
}

#[test]
fn file_diff_hunks_and_binary() {
    let f = fx();
    w(&f.proj, "a.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n");
    fs::write(f.proj.join("img.bin"), [0u8, 1, 2, 0, 3]).unwrap();
    let before = snap(&f, "r1", Phase::Before);
    w(&f.proj, "a.txt", "1\n2\n3\n4\nFIVE\n6\n7\n8\n9\n");
    fs::write(f.proj.join("img.bin"), [0u8, 9, 9, 0, 3, 0]).unwrap();
    let after = snap(&f, "r1", Phase::After);
    let d = f.store.file_diff("p1", &before, &after, "a.txt").unwrap();
    assert!(!d.binary && !d.truncated);
    assert_eq!(d.hunks.len(), 1);
    let h = &d.hunks[0];
    assert_eq!(
        (h.old_start, h.old_lines, h.new_start, h.new_lines),
        (2, 7, 2, 7)
    );
    let kinds: Vec<_> = h
        .lines
        .iter()
        .filter(|l| l.kind != LineKind::Context)
        .collect();
    assert_eq!(kinds.len(), 2);
    assert_eq!(
        (kinds[0].kind, kinds[0].text.as_str()),
        (LineKind::Removed, "5")
    );
    assert_eq!(
        (kinds[1].kind, kinds[1].text.as_str()),
        (LineKind::Added, "FIVE")
    );
    let b = f.store.file_diff("p1", &before, &after, "img.bin").unwrap();
    assert!(b.binary && b.hunks.is_empty());
    let c = f.store.diff("p1", &before, &after).unwrap();
    assert_eq!(find(&c, "img.bin").status, FileStatus::Binary);
}

#[test]
fn revert_selected_edited_since_added_and_not_captured() {
    let f = fx();
    w(&f.proj, "a.txt", "a0\n");
    w(&f.proj, "b.txt", "b0\n");
    w(&f.proj, "c.txt", "c0\n");
    w(&f.proj, ".env", "SECRET=0");
    let before = snap(&f, "r1", Phase::Before);
    w(&f.proj, "a.txt", "a1\n");
    w(&f.proj, "b.txt", "b1\n");
    w(&f.proj, "c.txt", "c1\n");
    w(&f.proj, "new.txt", "n\n");
    w(&f.proj, ".env", "SECRET=1");
    let after = snap(&f, "r1", Phase::After);
    w(&f.proj, "b.txt", "b-user\n"); // edited after AFTER
    let sel: Vec<String> = ["a.txt", "b.txt", "new.txt", ".env"]
        .map(String::from)
        .into();
    let rep = f
        .store
        .revert("p1", &f.proj, &before, &after, &sel)
        .unwrap();
    assert_eq!(fs::read_to_string(f.proj.join("a.txt")).unwrap(), "a0\n");
    assert_eq!(
        fs::read_to_string(f.proj.join("b.txt")).unwrap(),
        "b-user\n"
    );
    assert_eq!(fs::read_to_string(f.proj.join("c.txt")).unwrap(), "c1\n");
    assert!(!f.proj.join("new.txt").exists());
    assert_eq!(fs::read_to_string(f.proj.join(".env")).unwrap(), "SECRET=1");
    let mut restored = rep.restored.clone();
    restored.sort();
    assert_eq!(restored, vec!["a.txt", "new.txt"]);
    let reason = |p: &str| rep.skipped.iter().find(|s| s.path == p).unwrap().reason;
    assert_eq!(reason("b.txt"), RevertSkipReason::EditedSince);
    assert_eq!(reason("c.txt"), RevertSkipReason::NotSelected);
    assert_eq!(reason(".env"), RevertSkipReason::NotCaptured);
}

#[cfg(unix)]
#[test]
fn revert_keeps_mode() {
    use std::os::unix::fs::PermissionsExt;
    let f = fx();
    w(&f.proj, "run.sh", "echo 0\n");
    fs::set_permissions(f.proj.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
    let before = snap(&f, "r1", Phase::Before);
    w(&f.proj, "run.sh", "echo 1\n");
    let after = snap(&f, "r1", Phase::After);
    f.store
        .revert("p1", &f.proj, &before, &after, &["run.sh".into()])
        .unwrap();
    let m = fs::metadata(f.proj.join("run.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(m & 0o777, 0o755);
    assert_eq!(
        fs::read_to_string(f.proj.join("run.sh")).unwrap(),
        "echo 0\n"
    );
}

#[test]
fn interrupted_revert_restores_selected() {
    let f = fx();
    w(&f.proj, "a.txt", "a0\n");
    w(&f.proj, "b.txt", "b0\n");
    let before = snap(&f, "r1", Phase::Before);
    w(&f.proj, "a.txt", "a1\n");
    w(&f.proj, "b.txt", "b1\n");
    w(&f.proj, "new.txt", "n\n");
    let sel: Vec<String> = ["a.txt", "new.txt"].map(String::from).into();
    let rep = f
        .store
        .revert_interrupted("p1", &f.proj, &before, &sel)
        .unwrap();
    assert_eq!(fs::read_to_string(f.proj.join("a.txt")).unwrap(), "a0\n");
    assert_eq!(fs::read_to_string(f.proj.join("b.txt")).unwrap(), "b1\n");
    assert!(!f.proj.join("new.txt").exists());
    assert_eq!(rep.restored.len(), 2);
}

#[test]
fn prune_keeps_20_runs_and_delete_all_removes_store() {
    let f = fx();
    w(&f.proj, "a.txt", "x\n");
    for i in 0..25 {
        let run = format!("run{i:02}");
        w(&f.proj, "a.txt", &format!("v{i}\n"));
        snap(&f, &run, Phase::Before);
        snap(&f, &run, Phase::After);
    }
    f.store.prune("p1", KEEP_RUNS).unwrap();
    let ctx = f.store.ctx("p1").unwrap();
    let refs = f
        .store
        .git
        .text(
            &ctx,
            &["for-each-ref", "--format=%(refname)", "refs/studio/runs/"],
        )
        .unwrap();
    let runs: HashSet<_> = refs.lines().map(|l| l.split('/').nth(3).unwrap()).collect();
    assert_eq!(runs.len(), 20);
    assert_eq!(refs.lines().count(), 40);
    f.store.delete_all("p1").unwrap();
    assert!(!f.data.join("snapshots/p1.git").exists());
    f.store.delete_all("p1").unwrap(); // idempotent
}

#[test]
fn rejects_bad_ids() {
    let f = fx();
    assert!(
        f.store
            .snapshot("../x", &f.proj, "r", Phase::Before, "t")
            .is_err()
    );
    assert!(
        f.store
            .snapshot("p1", &f.proj, "a/../b", Phase::Before, "t")
            .is_err()
    );
}
