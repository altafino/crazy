//! Remote operations against bare repos (PRD M5 step 3, DR7). The "remote" is a
//! `git init --bare` folder; a second clone makes it move.
use super::*;
use crate::persistence::SyncOp;

/// A project repo "work" on main with a.txt, pushed to a bare "remote" (upstream set).
struct Linked {
    work: PathBuf,
    bare: PathBuf,
}

fn linked(f: &Fx) -> Linked {
    let work = f.repo("work", &[("a.txt", "one\n")]);
    let bare = f.dir("remote.git");
    f.git(&bare, &["init", "-q", "--bare", "--initial-branch=main"]);
    f.git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
    f.git(&work, &["push", "-q", "-u", "origin", "main"]);
    f.add_project("p1", &work);
    Linked { work, bare }
}

/// Another clone of the remote; commits `file` and pushes it.
fn remote_moves(f: &Fx, l: &Linked, file: &str, content: &str) -> PathBuf {
    let o = f.tmp.path().join("other");
    if !o.exists() {
        f.git(
            f.tmp.path(),
            &["clone", "-q", l.bare.to_str().unwrap(), "other"],
        );
    }
    f.git(&o, &["pull", "-q", "--ff-only"]);
    write(&o, file, content);
    f.git(&o, &["add", "-A"]);
    f.git(&o, &["commit", "-q", "-m", &format!("remote {file}")]);
    f.git(&o, &["push", "-q"]);
    o
}

fn commit_local(f: &Fx, l: &Linked, file: &str, content: &str) {
    write(&l.work, file, content);
    f.git(&l.work, &["add", "-A"]);
    f.git(&l.work, &["commit", "-q", "-m", &format!("local {file}")]);
}

async fn remote_of(f: &Fx, l: &Linked) -> RemoteInfo {
    status(f, &l.work).await.remote.expect("remote")
}

fn live_build_elsewhere(f: &Fx, id: &str) {
    f.add_project(id, &f.dir(&format!("proj-{id}")));
    f.db.insert_job_of_kind("jb", id, "build", crate::persistence::JobState::Running)
        .unwrap();
}

fn head(f: &Fx, d: &Path) -> String {
    f.git(d, &["rev-parse", "HEAD"]).trim().to_owned()
}

/// A RepoService whose git is `script` (the body runs before real git is exec'd).
fn fake_git(f: &Fx, body: &str, env: &[(&str, &str)]) -> Arc<RepoService> {
    fake_git_shared(f, body, env, None)
}

fn fake_git_shared(
    f: &Fx,
    body: &str,
    env: &[(&str, &str)],
    syncs: Option<crate::services::git::repo::SyncRegistry>,
) -> Arc<RepoService> {
    let p = f.tmp.path().join("fakegit.sh");
    fs::write(&p, format!("#!/bin/sh\n{body}\nexec git \"$@\"\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut e = f.env.clone();
    e.extend(env.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())));
    let store = Arc::new(SnapshotStore::new(
        &f.tmp.path().join("data2"),
        GitRunner {
            program: "git".into(),
            env: e.clone(),
        },
    ));
    let svc = RepoService::new(p, e, f.db.clone(), store);
    Arc::new(match syncs {
        Some(r) => svc.with_syncs(r),
        None => svc,
    })
}

// ---- status ---------------------------------------------------------------

#[tokio::test]
async fn upstream_ahead_behind_table() {
    let f = fx();
    // none: no remote at all
    let solo = f.repo("solo", &[("a.txt", "x\n")]);
    let st = f.svc.status("p0", &solo, &[]).await.unwrap();
    assert!(st.remote.is_none() && !st.merging);

    // an `origin` but never pushed: remote, no upstream
    let work = f.repo("work", &[("a.txt", "one\n")]);
    let bare = f.dir("remote.git");
    f.git(&bare, &["init", "-q", "--bare", "--initial-branch=main"]);
    f.git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
    let st = status(&f, &work).await;
    let r = st.remote.unwrap();
    assert_eq!((r.name.as_str(), r.upstream.clone()), ("origin", None));
    assert_eq!((r.ahead, r.behind, r.fetched_at.clone()), (0, 0, None));

    f.git(&work, &["push", "-q", "-u", "origin", "main"]);
    f.add_project("p1", &work);
    let l = Linked { work, bare };
    let r = remote_of(&f, &l).await;
    assert_eq!(r.upstream.as_deref(), Some("origin/main"));
    assert_eq!((r.ahead, r.behind), (0, 0));

    commit_local(&f, &l, "b.txt", "b\n");
    let r = remote_of(&f, &l).await;
    assert_eq!((r.ahead, r.behind), (1, 0));

    remote_moves(&f, &l, "c.txt", "c\n");
    f.svc.fetch("p1", &l.work).await.unwrap();
    let r = remote_of(&f, &l).await;
    assert_eq!((r.ahead, r.behind), (1, 1));
    assert!(r.fetched_at.unwrap().ends_with('Z'));

    // gone upstream: the branch was deleted on the remote and pruned
    f.git(&l.work, &["switch", "-q", "-c", "feature"]);
    f.git(&l.work, &["push", "-q", "-u", "origin", "feature"]);
    f.git(&l.work, &["fetch", "-q"]);
    f.git(&l.bare, &["branch", "-q", "-D", "feature"]);
    f.svc.fetch("p1", &l.work).await.unwrap();
    let r = remote_of(&f, &l).await;
    assert_eq!(r.upstream.as_deref(), Some("origin/feature"));
    assert_eq!((r.ahead, r.behind), (0, 0));

    // detached HEAD: still reports origin, no upstream
    f.git(&l.work, &["checkout", "-q", "--detach"]);
    let r = remote_of(&f, &l).await;
    assert_eq!((r.name.as_str(), r.upstream), ("origin", None));
}

#[tokio::test]
async fn remote_url_is_redacted_and_github_parsed() {
    let f = fx();
    let l = linked(&f);
    f.git(
        &l.work,
        &[
            "remote",
            "set-url",
            "origin",
            "https://user:s3cret@github.com/octo/app.git",
        ],
    );
    let r = remote_of(&f, &l).await;
    assert!(!r.url.contains("s3cret"), "{}", r.url);
    assert_eq!(
        r.github,
        Some(RemoteGithub {
            owner: "octo".into(),
            name: "app".into()
        })
    );
}

#[tokio::test]
async fn sync_commits_lists_both_directions() {
    let f = fx();
    let l = linked(&f);
    commit_local(&f, &l, "b.txt", "b\n");
    commit_local(&f, &l, "b2.txt", "b\n");
    remote_moves(&f, &l, "c.txt", "c\n");
    f.svc.fetch("p1", &l.work).await.unwrap();
    let sc = f.svc.sync_commits(&l.work).await.unwrap();
    assert_eq!(
        sc.outgoing
            .iter()
            .map(|c| c.subject.as_str())
            .collect::<Vec<_>>(),
        ["local b2.txt", "local b.txt"]
    );
    assert_eq!(sc.incoming.len(), 1);
    assert_eq!(sc.incoming[0].subject, "remote c.txt");

    // a branch never pushed lists everything no remote has
    f.git(&l.work, &["switch", "-q", "-c", "feature"]);
    let sc = f.svc.sync_commits(&l.work).await.unwrap();
    assert_eq!(sc.outgoing.len(), 2);
    assert!(sc.incoming.is_empty());

    let solo = f.repo("solo", &[("a.txt", "x\n")]);
    assert_eq!(f.svc.sync_commits(&solo).await, Err(SourceError::NoRemote));
}

// ---- fetch and push -------------------------------------------------------

#[tokio::test]
async fn fetch_reports_new_commits_and_stamps_fetch_time() {
    let f = fx();
    let l = linked(&f);
    assert_eq!(remote_of(&f, &l).await.fetched_at, None);
    remote_moves(&f, &l, "c.txt", "c\n");
    let o = f.svc.fetch("p1", &l.work).await.unwrap();
    assert_eq!((o.operation, o.commits), (SyncOp::Fetch, 1));
    let r = remote_of(&f, &l).await;
    assert_eq!(r.behind, 1);
    assert!(r.fetched_at.is_some());
    let row = f.db.get_sync("p1").unwrap().unwrap();
    assert!(row.ok && row.operation == SyncOp::Fetch);
    assert_eq!(f.svc.fetch("p1", &l.work).await.unwrap().commits, 0);
}

#[tokio::test]
async fn first_push_sets_upstream_then_plain_push() {
    let f = fx();
    let l = linked(&f);
    f.git(&l.work, &["switch", "-q", "-c", "feature"]);
    commit_local(&f, &l, "b.txt", "b\n");
    assert!(remote_of(&f, &l).await.upstream.is_none());
    let o = f.svc.push("p1", &l.work).await.unwrap();
    assert_eq!((o.operation, o.commits), (SyncOp::Push, 1));
    assert_eq!(
        remote_of(&f, &l).await.upstream.as_deref(),
        Some("origin/feature")
    );
    assert_eq!(
        f.git(&l.bare, &["rev-parse", "feature"]).trim(),
        head(&f, &l.work)
    );

    commit_local(&f, &l, "c.txt", "c\n");
    let o = f.svc.push("p1", &l.work).await.unwrap();
    assert_eq!(o.commits, 1);
    assert_eq!(
        f.git(&l.bare, &["rev-parse", "feature"]).trim(),
        head(&f, &l.work)
    );
    assert_eq!(f.svc.push("p1", &l.work).await.unwrap().commits, 0);
}

#[tokio::test]
async fn push_is_rejected_when_the_remote_is_ahead_and_never_forced() {
    let f = fx();
    let l = linked(&f);
    let other = remote_moves(&f, &l, "c.txt", "c\n");
    commit_local(&f, &l, "b.txt", "b\n");
    assert_eq!(f.svc.push("p1", &l.work).await, Err(SourceError::Rejected));
    assert_eq!(
        f.git(&l.bare, &["rev-parse", "main"]).trim(),
        head(&f, &other)
    );
    let row = f.db.get_sync("p1").unwrap().unwrap();
    assert!(!row.ok && row.operation == SyncOp::Push);
}

#[tokio::test]
async fn push_refusals() {
    let f = fx();
    let solo = f.repo("solo", &[("a.txt", "x\n")]);
    assert_eq!(f.svc.push("p0", &solo).await, Err(SourceError::NoRemote));
    let l = linked(&f);
    f.git(&l.work, &["checkout", "-q", "--detach"]);
    assert_eq!(
        f.svc.push("p1", &l.work).await,
        Err(SourceError::DetachedHead)
    );
    // refusals are not sync results
    assert!(f.db.get_sync("p1").unwrap().is_none());
}

// ---- pull -----------------------------------------------------------------

#[tokio::test]
async fn pull_fast_forwards() {
    let f = fx();
    let l = linked(&f);
    let other = remote_moves(&f, &l, "c.txt", "c\n");
    let o = f.svc.pull("p1", &l.work).await.unwrap();
    assert_eq!((o.operation, o.commits), (SyncOp::Pull, 1));
    assert_eq!(head(&f, &l.work), head(&f, &other));
    assert!(f.db.get_sync("p1").unwrap().unwrap().ok);
    assert_eq!(f.svc.pull("p1", &l.work).await.unwrap().commits, 0);
}

#[tokio::test]
async fn pull_refusals() {
    let f = fx();
    let l = linked(&f);
    remote_moves(&f, &l, "c.txt", "c\n");
    write(&l.work, "a.txt", "dirty\n");
    assert_eq!(
        f.svc.pull("p1", &l.work).await,
        Err(SourceError::DirtyTree { count: 1 })
    );
    f.git(&l.work, &["checkout", "-q", "--", "a.txt"]);

    f.live_changes_run("p2");
    assert_eq!(
        f.svc.pull("p2", &l.work).await,
        Err(SourceError::RunInProgress)
    );
    live_build_elsewhere(&f, "p3");
    assert_eq!(
        f.svc.pull("p3", &l.work).await,
        Err(SourceError::BuildRunning)
    );

    // diverged
    commit_local(&f, &l, "b.txt", "b\n");
    assert_eq!(f.svc.pull("p1", &l.work).await, Err(SourceError::Diverged));
    // never pushed branch
    f.git(&l.work, &["switch", "-q", "-c", "feature"]);
    assert_eq!(
        f.svc.pull("p1", &l.work).await,
        Err(SourceError::NoUpstream)
    );
}

// ---- merge ----------------------------------------------------------------

#[tokio::test]
async fn clean_merge_makes_a_merge_commit() {
    let f = fx();
    let l = linked(&f);
    remote_moves(&f, &l, "c.txt", "c\n");
    commit_local(&f, &l, "b.txt", "b\n");
    f.svc.fetch("p1", &l.work).await.unwrap();
    let o = f.svc.merge_upstream("p1", &l.work).await.unwrap();
    assert!(!o.conflicted && o.commits >= 2, "{o:?}");
    let parents = f.git(&l.work, &["rev-list", "--parents", "-n1", "HEAD"]);
    assert_eq!(parents.split_whitespace().count(), 3);
    assert!(!status(&f, &l.work).await.merging);
    assert_eq!(f.svc.push("p1", &l.work).await.unwrap().commits, 2);
}

#[tokio::test]
async fn conflicted_merge_resolve_commit_push() {
    let f = fx();
    let l = linked(&f);
    remote_moves(&f, &l, "a.txt", "theirs\n");
    commit_local(&f, &l, "a.txt", "mine\n");
    f.svc.fetch("p1", &l.work).await.unwrap();
    let o = f.svc.merge_upstream("p1", &l.work).await.unwrap();
    assert!(o.conflicted);
    let st = status(&f, &l.work).await;
    assert!(st.merging);
    assert_eq!(st.conflicts, 1);

    // commit refused while conflicts remain
    assert_eq!(
        f.svc.commit(&l.work, "Merge", &[]).await,
        Err(SourceError::ConflictsPresent)
    );
    // markers remain: refused unless forced
    assert_eq!(
        f.svc.mark_resolved(&l.work, "a.txt", false).await,
        Err(SourceError::MarkersRemain {
            paths: vec!["a.txt".into()]
        })
    );
    assert_eq!(
        f.svc.mark_resolved(&l.work, "nope.txt", false).await,
        Err(SourceError::Invalid {
            message: "That file has no conflict.".into()
        })
    );
    f.svc.mark_resolved(&l.work, "a.txt", true).await.unwrap();
    assert_eq!(status(&f, &l.work).await.conflicts, 0);

    // commit the merge: two parents, staged markers and all (forced earlier)
    write(&l.work, "a.txt", "both\n");
    f.git(&l.work, &["add", "a.txt"]);
    let c = f
        .svc
        .commit(&l.work, "Merge GitHub's changes", &[])
        .await
        .unwrap();
    assert_eq!(c.commit.subject, "Merge GitHub's changes");
    let parents = f.git(&l.work, &["rev-list", "--parents", "-n1", "HEAD"]);
    assert_eq!(parents.split_whitespace().count(), 3);
    assert!(!status(&f, &l.work).await.merging);
    let p = f.svc.push("p1", &l.work).await.unwrap();
    assert_eq!(p.commits, 2);
    assert_eq!(
        f.git(&l.bare, &["rev-parse", "main"]).trim(),
        head(&f, &l.work)
    );
}

#[tokio::test]
async fn resolved_file_without_markers_needs_no_force() {
    let f = fx();
    let l = linked(&f);
    remote_moves(&f, &l, "a.txt", "theirs\n");
    commit_local(&f, &l, "a.txt", "mine\n");
    f.svc.fetch("p1", &l.work).await.unwrap();
    f.svc.merge_upstream("p1", &l.work).await.unwrap();
    write(&l.work, "a.txt", "fixed\n");
    f.svc.mark_resolved(&l.work, "a.txt", false).await.unwrap();
    assert_eq!(status(&f, &l.work).await.conflicts, 0);
}

#[tokio::test]
async fn abort_restores_the_exact_pre_merge_state() {
    let f = fx();
    let l = linked(&f);
    remote_moves(&f, &l, "a.txt", "theirs\n");
    commit_local(&f, &l, "a.txt", "mine\n");
    f.svc.fetch("p1", &l.work).await.unwrap();
    let before = head(&f, &l.work);
    assert!(
        f.svc
            .merge_upstream("p1", &l.work)
            .await
            .unwrap()
            .conflicted
    );
    f.svc.abort_merge("p1", &l.work).await.unwrap();
    assert_eq!(head(&f, &l.work), before);
    assert_eq!(fs::read_to_string(l.work.join("a.txt")).unwrap(), "mine\n");
    let st = status(&f, &l.work).await;
    assert!(!st.merging && st.entries.is_empty());
    assert!(matches!(
        f.svc.abort_merge("p1", &l.work).await,
        Err(SourceError::Invalid { .. })
    ));
}

#[tokio::test]
async fn merge_and_abort_refusals() {
    let f = fx();
    let l = linked(&f);
    remote_moves(&f, &l, "c.txt", "c\n");
    f.svc.fetch("p1", &l.work).await.unwrap();
    write(&l.work, "a.txt", "dirty\n");
    assert_eq!(
        f.svc.merge_upstream("p1", &l.work).await,
        Err(SourceError::DirtyTree { count: 1 })
    );
    f.live_changes_run("p2");
    assert_eq!(
        f.svc.merge_upstream("p2", &l.work).await,
        Err(SourceError::RunInProgress)
    );
    live_build_elsewhere(&f, "p3");
    assert_eq!(
        f.svc.abort_merge("p3", &l.work).await,
        Err(SourceError::BuildRunning)
    );
}

// ---- one operation, stop, command line, errors -----------------------------

#[tokio::test]
async fn second_operation_is_refused_and_stop_kills_the_process_group() {
    let f = fx();
    let l = linked(&f);
    let pid = f.tmp.path().join("pid");
    let svc = fake_git(
        &f,
        &format!(
            "case \"$*\" in *fetch*) echo $$ > {}; exec sleep 60;; esac",
            pid.display()
        ),
        &[],
    );
    let (s2, w2) = (svc.clone(), l.work.clone());
    let first = tokio::spawn(async move { s2.fetch("p1", &w2).await });
    for _ in 0..200 {
        if pid.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(pid.exists(), "fetch never started");
    assert_eq!(
        svc.fetch("p1", &l.work).await,
        Err(SourceError::SyncRunning)
    );
    assert_eq!(svc.push("p1", &l.work).await, Err(SourceError::SyncRunning));
    // a different project is not blocked
    f.add_project("other", &f.dir("o"));
    let o = svc.push("other", &l.work).await.unwrap();
    assert_eq!(o.commits, 0);

    assert!(svc.stop_sync("p1"));
    assert_eq!(first.await.unwrap(), Err(SourceError::Cancelled));
    let n: i32 = fs::read_to_string(&pid).unwrap().trim().parse().unwrap();
    // SAFETY: signal 0 only checks whether the pid exists.
    let alive = unsafe { libc::kill(n, 0) } == 0;
    assert!(!alive, "the fetch process is still running");
    assert!(!svc.stop_sync("p1"));
    let row = f.db.get_sync("p1").unwrap().unwrap();
    assert!(!row.ok);
}

/// The commands build a RepoService per call; `with_syncs` is what lets `source_stop_sync`
/// (another instance) reach the operation another command started.
#[tokio::test]
async fn stop_sync_reaches_an_operation_started_by_another_instance() {
    let f = fx();
    let l = linked(&f);
    let pid = f.tmp.path().join("pid2");
    let reg = crate::services::git::repo::SyncRegistry::default();
    let script = format!(
        "case \"$*\" in *fetch*) echo $$ > {}; exec sleep 60;; esac",
        pid.display()
    );
    let first = fake_git_shared(&f, &script, &[], Some(reg.clone()));
    let shared = fake_git_shared(&f, &script, &[], Some(reg));
    let lone = fake_git_shared(&f, &script, &[], None);
    let (s2, w2) = (first.clone(), l.work.clone());
    let running = tokio::spawn(async move { s2.fetch("p1", &w2).await });
    for _ in 0..200 {
        if pid.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(pid.exists(), "fetch never started");
    assert!(!lone.stop_sync("p1"), "an unshared registry sees nothing");
    assert!(shared.stop_sync("p1"));
    assert_eq!(running.await.unwrap(), Err(SourceError::Cancelled));
}

#[tokio::test]
async fn network_commands_are_batch_mode_with_the_gh_helper() {
    let f = fx();
    let l = linked(&f);
    let log = f.tmp.path().join("log");
    let svc = fake_git(
        &f,
        &format!(
            "case \"$*\" in *fetch*) echo \"$* | $GIT_SSH_COMMAND | $GIT_TERMINAL_PROMPT\" >> {}; esac",
            log.display()
        ),
        &[],
    );
    let svc = Arc::try_unwrap(svc)
        .ok()
        .unwrap()
        .with_gh(Some(PathBuf::from("/opt/gh")));
    svc.fetch("p1", &l.work).await.unwrap();
    let line = fs::read_to_string(&log).unwrap();
    assert!(
        line.contains("credential.https://github.com.helper=!'/opt/gh' auth git-credential"),
        "{line}"
    );
    assert!(line.contains("http.lowSpeedLimit=1000") && line.contains("fetch --prune origin"));
    assert!(line.contains("BatchMode=yes"), "{line}");
    assert!(line.trim_end().ends_with("| 0"), "{line}");
}

#[tokio::test]
async fn auth_failure_is_typed_and_stored_redacted() {
    let f = fx();
    let l = linked(&f);
    let ssh = f.tmp.path().join("fakessh.sh");
    fs::write(
        &ssh,
        "#!/bin/sh\necho 'git@github.com: Permission denied (publickey). see https://user:s3cret@example.com/x' >&2\nexit 255\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&ssh, fs::Permissions::from_mode(0o755)).unwrap();
    }
    f.git(
        &l.work,
        &[
            "remote",
            "set-url",
            "origin",
            "ssh://git@example.com/o/n.git",
        ],
    );
    let svc = fake_git(&f, "", &[("GIT_SSH_COMMAND", ssh.to_str().unwrap())]);
    assert_eq!(
        svc.fetch("p1", &l.work).await,
        Err(SourceError::AuthRequired)
    );
    let row = f.db.get_sync("p1").unwrap().unwrap();
    assert!(!row.ok && row.operation == SyncOp::Fetch);
    let m = row.message.unwrap();
    assert!(m.contains("Permission denied"), "{m}");
    assert!(!m.contains("s3cret"), "{m}");
}
