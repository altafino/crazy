//! Git remote operations of RepoService (PRD M5 step 3; M5-D5/D6/D8/D11/D13, DR3/DR5/DR7).
//!
//! A child module of `repo`, so it shares RepoService's private plumbing. Every network
//! command runs non-interactively (`net::network_env`, `net::network_config_args`), has no
//! overall timeout and can be stopped; one operation per project at a time.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use tokio::sync::Notify;

use super::*;
use crate::persistence::{SyncOp, SyncRecord};
use crate::platform::time::rfc3339;
use crate::services::agents::secrets::redact;

const SYNC_LIST_CAP: u32 = 50;
const FORMAT: &str = "--format=%H%x1f%an%x1f%at%x1f%s";

// ---- types ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteGithub {
    pub owner: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteInfo {
    pub name: String,
    /// Credentials removed, for display.
    pub url: String,
    /// e.g. "origin/main"; None when the branch was never pushed.
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// `.git/FETCH_HEAD` mtime, RFC 3339 UTC; None before the first fetch.
    pub fetched_at: Option<String>,
    pub github: Option<RemoteGithub>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncCommits {
    /// Not on the remote yet (`@{u}..HEAD`), newest first, at most 50.
    pub outgoing: Vec<CommitInfo>,
    /// On the remote only (`HEAD..@{u}`), at most 50.
    pub incoming: Vec<CommitInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncOutcome {
    pub operation: SyncOp,
    /// Commits pushed, pulled, merged in, or newly seen by a fetch.
    pub commits: u32,
    /// A merge stopped on conflicts; the repo is `merging`.
    pub conflicted: bool,
    /// Plain sentence, also stored in `git_sync`.
    pub message: String,
}

// ---- one operation per project (M5-D13) -----------------------------------

/// Shared by every `RepoService` the app builds, so `stop_sync` reaches an operation another
/// instance started (`RepoService::with_syncs`).
#[derive(Default, Clone)]
pub(crate) struct Registry(Arc<Mutex<HashMap<String, Arc<Notify>>>>);

pub(super) struct Net<'a> {
    pub(super) user_ssh: bool,
    pub(super) cancel: &'a Notify,
}

/// Holds the project's operation slot until dropped.
struct SyncGuard {
    reg: Registry,
    id: String,
    notify: Arc<Notify>,
    /// Redacted raw git output of the failure, kept for the stored message.
    detail: Mutex<String>,
}
impl Drop for SyncGuard {
    fn drop(&mut self) {
        if let Ok(mut m) = self.reg.0.lock() {
            m.remove(&self.id);
        }
    }
}

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl RepoService {
    fn begin(&self, project_id: &str) -> Result<SyncGuard> {
        let mut m = self
            .syncs
            .0
            .lock()
            .map_err(|_| git_err("sync registry poisoned"))?;
        if m.contains_key(project_id) {
            return Err(SourceError::SyncRunning);
        }
        let notify = Arc::new(Notify::new());
        m.insert(project_id.to_owned(), notify.clone());
        Ok(SyncGuard {
            reg: self.syncs.clone(),
            id: project_id.to_owned(),
            notify,
            detail: Mutex::new(String::new()),
        })
    }

    /// Stops the project's running operation (kills its process group). False when none runs.
    pub(crate) fn stop_sync(&self, project_id: &str) -> bool {
        let n = self
            .syncs
            .0
            .lock()
            .ok()
            .and_then(|m| m.get(project_id).cloned());
        n.inspect(|n| n.notify_one()).is_some()
    }

    /// Stores the outcome in `git_sync`. Refusals before git ran (dirty tree, busy, ...) are
    /// not sync failures and leave the row alone.
    fn finish(&self, g: &SyncGuard, op: SyncOp, r: Result<SyncOutcome>) -> Result<SyncOutcome> {
        let (ok, message) = match &r {
            Ok(o) => (true, o.message.clone()),
            Err(
                e @ (SourceError::AuthRequired
                | SourceError::Diverged
                | SourceError::Rejected
                | SourceError::Stalled
                | SourceError::Unreachable
                | SourceError::Cancelled
                | SourceError::Git { .. }),
            ) => {
                let detail = g.detail.lock().map(|d| d.clone()).unwrap_or_default();
                let text = e.to_string();
                (
                    false,
                    if detail.is_empty() || detail == text {
                        text
                    } else {
                        format!("{text}\n{detail}")
                    },
                )
            }
            Err(_) => return r,
        };
        let _ = self.db.upsert_sync(&SyncRecord {
            project_id: g.id.clone(),
            operation: op,
            ok,
            message: Some(redact(&message).into_owned()),
            finished_at: rfc3339(SystemTime::now()).unwrap_or_default(),
        });
        r
    }

    /// Runs git without a terminal and without an overall timeout; the guard's Notify stops it.
    async fn net_exec(&self, cwd: &Path, g: &SyncGuard, args: &[&str]) -> Result<Out> {
        let user_ssh = self.cfg(cwd, "core.sshCommand").await.is_some();
        let cfg = super::super::net::network_config_args(self.gh.as_deref());
        let mut full: Vec<&str> = cfg.iter().map(String::as_str).collect();
        full.extend(args);
        let o = self
            .exec_inner(
                cwd,
                &full,
                None,
                None,
                false,
                Some(Net {
                    user_ssh,
                    cancel: &g.notify,
                }),
            )
            .await?;
        if o.cancelled {
            return Err(SourceError::Cancelled);
        }
        Ok(o)
    }

    /// `net_exec`, failing with a typed error on a non-zero exit.
    async fn net_ok(&self, cwd: &Path, g: &SyncGuard, args: &[&str]) -> Result<Out> {
        let o = self.net_exec(cwd, g, args).await?;
        if o.ok {
            return Ok(o);
        }
        if let Ok(mut d) = g.detail.lock() {
            *d = redact(o.combined().trim()).into_owned();
        }
        Err(classify_net(&o))
    }

    // ---- small git reads

    async fn cfg(&self, cwd: &Path, key: &str) -> Option<String> {
        let o = self
            .exec(cwd, &["config", "--get", key], None, READ_TIMEOUT, true)
            .await
            .ok()?;
        Some(o.text()).filter(|v| o.ok && !v.is_empty())
    }

    async fn read_line(&self, loc: &Loc, args: &[&str]) -> Option<String> {
        let o = self
            .exec(&loc.top, args, None, READ_TIMEOUT, true)
            .await
            .ok()?;
        Some(o.text()).filter(|v| o.ok && !v.is_empty())
    }

    async fn git_path(&self, loc: &Loc, name: &str) -> Option<PathBuf> {
        let p = self
            .read_line(loc, &["rev-parse", "--git-path", name])
            .await?;
        Some(loc.top.join(p))
    }

    /// MERGE_HEAD's path when a merge is in progress.
    pub(super) async fn merge_head(&self, loc: &Loc) -> Option<PathBuf> {
        self.git_path(loc, "MERGE_HEAD")
            .await
            .filter(|p| p.exists())
    }

    async fn upstream_ref(&self, loc: &Loc) -> Option<String> {
        self.read_line(
            loc,
            &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
        )
        .await
    }

    async fn head_oid(&self, loc: &Loc) -> Option<String> {
        self.read_line(loc, &["rev-parse", "--verify", "-q", "HEAD"])
            .await
    }

    /// The checked-out branch; DetachedHead when there is none.
    async fn branch_name(&self, loc: &Loc) -> Result<String> {
        self.read_line(loc, &["symbolic-ref", "-q", "--short", "HEAD"])
            .await
            .ok_or(SourceError::DetachedHead)
    }

    async fn rev_count(&self, loc: &Loc, range: &[&str]) -> Result<u32> {
        let mut a = vec!["rev-list", "--count"];
        a.extend(range);
        Ok(self.git(&loc.top, &a).await?.text().parse().unwrap_or(0))
    }

    /// The branch's remote, else `origin`; its URL.
    async fn remote_of(&self, loc: &Loc, branch: Option<&str>) -> Option<(String, String)> {
        let configured = match branch {
            Some(b) => self
                .cfg(&loc.top, &format!("branch.{b}.remote"))
                .await
                .filter(|r| r != "."),
            None => None,
        };
        let name = configured.unwrap_or_else(|| "origin".to_owned());
        let url = self.read_line(loc, &["remote", "get-url", &name]).await?;
        Some((name, url))
    }

    async fn branch_or_none(&self, loc: &Loc) -> Option<String> {
        self.read_line(loc, &["symbolic-ref", "-q", "--short", "HEAD"])
            .await
    }

    pub(super) async fn remote_info(
        &self,
        loc: &Loc,
        parsed: &Parsed,
    ) -> Result<Option<RemoteInfo>> {
        let branch = match &parsed.head {
            HeadState::Branch { name } | HeadState::Unborn { name } => Some(name.as_str()),
            HeadState::Detached { .. } => None,
        };
        let Some((name, url)) = self.remote_of(loc, branch).await else {
            return Ok(None);
        };
        let fetched_at = self
            .git_path(loc, "FETCH_HEAD")
            .await
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok())
            .and_then(rfc3339);
        let (ahead, behind) = parsed.ahead_behind.unwrap_or((0, 0));
        Ok(Some(RemoteInfo {
            github: parse_github(&url),
            url: redact(&url).into_owned(),
            name,
            upstream: parsed.upstream.clone(),
            ahead,
            behind,
            fetched_at,
        }))
    }

    /// Whole-repo clean check for operations that move HEAD (like `switch_branch`).
    async fn require_clean(&self, loc: &Loc) -> Result<()> {
        let o = self
            .git(
                &loc.top,
                &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
            )
            .await?;
        let dirty = parse_status(&o.stdout).entries;
        if dirty.iter().any(|e| e.kind() == EntryKind::Conflicted) {
            return Err(SourceError::ConflictsPresent);
        }
        if dirty.is_empty() {
            Ok(())
        } else {
            Err(SourceError::DirtyTree {
                count: u32::try_from(dirty.len()).unwrap_or(u32::MAX),
            })
        }
    }

    async fn log_args(&self, loc: &Loc, range: &[&str]) -> Result<Vec<CommitInfo>> {
        let n = format!("-{SYNC_LIST_CAP}");
        let mut a = vec!["log", &n, "-z", FORMAT];
        a.extend(range);
        let o = self.git(&loc.top, &a).await?;
        Ok(nul_split(&o.stdout).filter_map(parse_commit).collect())
    }

    // ---- sync lists (DR5)

    /// Commits to push and commits to pull, whole repo. A branch never pushed lists every
    /// commit no remote ref has yet and nothing incoming.
    pub(crate) async fn sync_commits(&self, project: &Path) -> Result<SyncCommits> {
        let loc = self.locate(project).await?;
        let branch = self.branch_or_none(&loc).await;
        let Some((remote, _)) = self.remote_of(&loc, branch.as_deref()).await else {
            return Err(SourceError::NoRemote);
        };
        if self.head_oid(&loc).await.is_none() {
            return Ok(SyncCommits {
                outgoing: Vec::new(),
                incoming: Vec::new(),
            });
        }
        if self.upstream_ref(&loc).await.is_some() {
            Ok(SyncCommits {
                outgoing: self.log_args(&loc, &["@{u}..HEAD"]).await?,
                incoming: self.log_args(&loc, &["HEAD..@{u}"]).await?,
            })
        } else {
            let remotes = format!("--remotes={remote}");
            Ok(SyncCommits {
                outgoing: self.log_args(&loc, &["HEAD", "--not", &remotes]).await?,
                incoming: Vec::new(),
            })
        }
    }

    // ---- operations (D8, D13)

    pub(crate) async fn fetch(&self, project_id: &str, project: &Path) -> Result<SyncOutcome> {
        let g = self.begin(project_id)?;
        let r = self.fetch_inner(&g, project).await;
        self.finish(&g, SyncOp::Fetch, r)
    }

    async fn fetch_inner(&self, g: &SyncGuard, project: &Path) -> Result<SyncOutcome> {
        let loc = self.locate(project).await?;
        let branch = self.branch_or_none(&loc).await;
        let Some((remote, _)) = self.remote_of(&loc, branch.as_deref()).await else {
            return Err(SourceError::NoRemote);
        };
        let before = self
            .read_line(&loc, &["rev-parse", "--verify", "-q", "@{u}"])
            .await;
        self.net_ok(&loc.top, g, &["fetch", "--prune", &remote])
            .await?;
        let commits = match before {
            Some(b) => self
                .rev_count(&loc, &[&format!("{b}..@{{u}}")])
                .await
                .unwrap_or(0),
            None => 0,
        };
        Ok(SyncOutcome {
            operation: SyncOp::Fetch,
            commits,
            conflicted: false,
            message: if commits == 0 {
                "Fetched. Nothing new.".into()
            } else {
                format!("Fetched {}.", plural(commits, "new commit", "new commits"))
            },
        })
    }

    pub(crate) async fn pull(&self, project_id: &str, project: &Path) -> Result<SyncOutcome> {
        self.require_idle(project_id)?;
        let g = self.begin(project_id)?;
        let r = self.pull_inner(&g, project).await;
        self.finish(&g, SyncOp::Pull, r)
    }

    async fn pull_inner(&self, g: &SyncGuard, project: &Path) -> Result<SyncOutcome> {
        let loc = self.locate(project).await?;
        self.branch_name(&loc).await?;
        self.require_clean(&loc).await?;
        if self.upstream_ref(&loc).await.is_none() {
            return Err(SourceError::NoUpstream);
        }
        let before = self.head_oid(&loc).await;
        self.net_ok(&loc.top, g, &["pull", "--ff-only"]).await?;
        let commits = match before {
            Some(b) => self.rev_count(&loc, &[&format!("{b}..HEAD")]).await?,
            None => 0,
        };
        Ok(SyncOutcome {
            operation: SyncOp::Pull,
            commits,
            conflicted: false,
            message: if commits == 0 {
                "Already up to date.".into()
            } else {
                format!("Pulled {}.", plural(commits, "commit", "commits"))
            },
        })
    }

    /// Merges the upstream branch into the current one; conflicts leave the repo `merging`.
    pub(crate) async fn merge_upstream(
        &self,
        project_id: &str,
        project: &Path,
    ) -> Result<SyncOutcome> {
        self.require_idle(project_id)?;
        let g = self.begin(project_id)?;
        let r = self.merge_inner(&g, project).await;
        self.finish(&g, SyncOp::Merge, r)
    }

    async fn merge_inner(&self, g: &SyncGuard, project: &Path) -> Result<SyncOutcome> {
        let loc = self.locate(project).await?;
        self.branch_name(&loc).await?;
        self.require_clean(&loc).await?;
        self.require_identity(&loc.top).await?;
        if self.upstream_ref(&loc).await.is_none() {
            return Err(SourceError::NoUpstream);
        }
        let before = self.head_oid(&loc).await;
        let o = self
            .net_exec(&loc.top, g, &["merge", "--no-edit", "@{u}"])
            .await?;
        if !o.ok {
            if self.merge_head(&loc).await.is_some() {
                return Ok(SyncOutcome {
                    operation: SyncOp::Merge,
                    commits: 0,
                    conflicted: true,
                    message: "The merge stopped on conflicts.".into(),
                });
            }
            if let Ok(mut d) = g.detail.lock() {
                *d = redact(o.combined().trim()).into_owned();
            }
            return Err(classify_net(&o));
        }
        let commits = match before {
            Some(b) => self.rev_count(&loc, &[&format!("{b}..HEAD")]).await?,
            None => 0,
        };
        Ok(SyncOutcome {
            operation: SyncOp::Merge,
            commits,
            conflicted: false,
            message: if commits == 0 {
                "Already up to date.".into()
            } else {
                format!("Merged {}.", plural(commits, "commit", "commits"))
            },
        })
    }

    pub(crate) async fn abort_merge(&self, project_id: &str, project: &Path) -> Result<()> {
        self.require_idle(project_id)?;
        let _g = self.begin(project_id)?;
        let loc = self.locate(project).await?;
        if self.merge_head(&loc).await.is_none() {
            return Err(invalid("There is no merge to abort."));
        }
        self.git(&loc.top, &["merge", "--abort"]).await?;
        Ok(())
    }

    /// First push sets the upstream (`-u`); later ones are a plain push. Never forced.
    pub(crate) async fn push(&self, project_id: &str, project: &Path) -> Result<SyncOutcome> {
        let g = self.begin(project_id)?;
        let r = self.push_inner(&g, project).await;
        self.finish(&g, SyncOp::Push, r)
    }

    async fn push_inner(&self, g: &SyncGuard, project: &Path) -> Result<SyncOutcome> {
        let loc = self.locate(project).await?;
        let branch = self.branch_name(&loc).await?;
        if self.head_oid(&loc).await.is_none() {
            return Err(invalid("Commit something first."));
        }
        let Some((remote, _)) = self.remote_of(&loc, Some(&branch)).await else {
            return Err(SourceError::NoRemote);
        };
        let first = self.upstream_ref(&loc).await.is_none();
        let commits = if first {
            let remotes = format!("--remotes={remote}");
            self.rev_count(&loc, &["HEAD", "--not", &remotes]).await?
        } else {
            self.rev_count(&loc, &["@{u}..HEAD"]).await?
        };
        if first {
            let spec = format!("HEAD:refs/heads/{branch}");
            self.net_ok(&loc.top, g, &["push", "-u", &remote, &spec])
                .await?;
        } else {
            self.net_ok(&loc.top, g, &["push"]).await?;
        }
        Ok(SyncOutcome {
            operation: SyncOp::Push,
            commits,
            conflicted: false,
            message: if commits == 0 {
                "Already up to date.".into()
            } else {
                format!("Pushed {}.", plural(commits, "commit", "commits"))
            },
        })
    }

    // ---- merge resolution (DR7)

    /// Stages a conflicted file. Conflict markers still in it need `force`.
    pub(crate) async fn mark_resolved(
        &self,
        project: &Path,
        path: &str,
        force: bool,
    ) -> Result<()> {
        let loc = self.locate(project).await?;
        let rel = loc.root_rel(path)?;
        if self
            .git(&loc.top, &["ls-files", "-u", "-z", "--", &rel])
            .await?
            .stdout
            .is_empty()
        {
            return Err(invalid("That file has no conflict."));
        }
        if !force && has_markers(&std::fs::read(project.join(path)).unwrap_or_default()) {
            return Err(SourceError::MarkersRemain {
                paths: vec![path.to_owned()],
            });
        }
        self.git(&loc.top, &["add", "-A", "--", &rel]).await?;
        Ok(())
    }

    /// Commit during a merge: everything staged, no `--only` (git refuses a partial commit).
    pub(super) async fn commit_merge(&self, loc: &Loc, message: &str) -> Result<CommitOutcome> {
        if !self
            .git(&loc.top, &["ls-files", "-u", "-z"])
            .await?
            .stdout
            .is_empty()
        {
            return Err(SourceError::ConflictsPresent);
        }
        let o = self
            .exec(
                &loc.top,
                &["commit", "-m", message],
                None,
                self.commit_timeout,
                false,
            )
            .await?;
        if !o.ok {
            return Err(if o.timed_out {
                SourceError::CommitTimedOut
            } else {
                self.classify_commit(loc, &o).await
            });
        }
        let commit = self
            .log_args(loc, &["HEAD"])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| git_err("the commit was not found afterwards"))?;
        let files = self
            .git(&loc.top, &["diff", "--name-only", "HEAD^1", "HEAD"])
            .await?
            .text()
            .lines()
            .count();
        Ok(CommitOutcome {
            commit,
            files: u32::try_from(files).unwrap_or(u32::MAX),
        })
    }
}

// ---- helpers --------------------------------------------------------------

fn has_markers(b: &[u8]) -> bool {
    b.split(|c| *c == b'\n')
        .any(|l| l.starts_with(b"<<<<<<<") || l.starts_with(b">>>>>>>"))
}

/// Git's stderr (under `LC_ALL=C`) to a typed error. Stalls are checked before
/// "unable to access", which curl's low-speed abort also prints.
fn classify_net(o: &Out) -> SourceError {
    let m = format!("{}\n{}", o.stderr, lossy(&o.stdout));
    let any = |needles: &[&str]| needles.iter().any(|n| m.contains(n));
    if o.cancelled {
        SourceError::Cancelled
    } else if any(&[
        "Permission denied (publickey",
        "could not read Username",
        "could not read Password",
        "Authentication failed",
        "terminal prompts disabled",
    ]) {
        SourceError::AuthRequired
    } else if any(&["non-fast-forward", "(fetch first)", "Updates were rejected"]) {
        SourceError::Rejected
    } else if any(&[
        "Not possible to fast-forward",
        "diverging branches",
        "have diverged",
    ]) {
        SourceError::Diverged
    } else if any(&[
        "no tracking information",
        "no upstream configured",
        "has no upstream branch",
    ]) {
        SourceError::NoUpstream
    } else if o.timed_out
        || any(&[
            "Operation too slow",
            "bytes/sec transferred",
            "Timeout, server",
            "closed by remote host",
            "send disconnect",
        ])
        || (m.contains("Connection to ") && m.contains(" closed"))
    {
        SourceError::Stalled
    } else if any(&[
        "Could not resolve host",
        "Connection timed out",
        "Connection refused",
        "Network is unreachable",
        "unable to access",
    ]) {
        SourceError::Unreachable
    } else {
        let e = classify(o);
        match e {
            SourceError::Git { message } => git_err(redact(&message).into_owned()),
            e => e,
        }
    }
}

/// owner/name of a github.com remote (https, ssh:// and scp-style `git@github.com:o/n`).
fn parse_github(url: &str) -> Option<RemoteGithub> {
    let path = if let Some(rest) = ["https://", "http://", "ssh://", "git://"]
        .iter()
        .find_map(|p| url.strip_prefix(p))
    {
        let (host, path) = rest.split_once('/')?;
        let host = host.rsplit('@').next()?.split(':').next()?;
        (host == "github.com" || host == "www.github.com").then_some(path)?
    } else {
        let (host, path) = url.split_once(':')?;
        (host.rsplit('@').next()? == "github.com").then_some(path)?
    };
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path.split_once('/')?;
    (!owner.is_empty() && !name.is_empty() && !name.contains('/')).then(|| RemoteGithub {
        owner: owner.to_owned(),
        name: name.to_owned(),
    })
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn github_urls_in_every_form() {
        let g = |u: &str| parse_github(u).map(|r| format!("{}/{}", r.owner, r.name));
        for u in [
            "https://github.com/o/n.git",
            "https://tok:x@github.com/o/n",
            "git@github.com:o/n.git",
            "ssh://git@github.com/o/n.git",
            "https://github.com/o/n/",
        ] {
            assert_eq!(g(u).as_deref(), Some("o/n"), "{u}");
        }
        for u in [
            "https://gitlab.com/o/n.git",
            "/tmp/bare.git",
            "https://github.com/o",
            "git@github.com:o/n/x.git",
        ] {
            assert_eq!(g(u), None, "{u}");
        }
    }

    #[test]
    fn markers_need_an_open_or_close_line() {
        assert!(has_markers(b"a\n<<<<<<< HEAD\nx\n"));
        assert!(has_markers(b"x\n>>>>>>> theirs"));
        assert!(!has_markers(b"Title\n=======\nbody"));
        assert!(!has_markers(b"  <<<<<<< indented"));
    }

    fn out(stderr: &str) -> Out {
        Out {
            cancelled: false,
            ok: false,
            stdout: Vec::new(),
            stderr: stderr.into(),
            timed_out: false,
        }
    }

    #[test]
    fn stderr_maps_to_typed_errors() {
        let k = |s: &str| classify_net(&out(s));
        assert_eq!(
            k("git@github.com: Permission denied (publickey)."),
            SourceError::AuthRequired
        );
        assert_eq!(
            k("fatal: could not read Username for 'https://github.com'"),
            SourceError::AuthRequired
        );
        assert_eq!(
            k("remote: Invalid username or password.\nfatal: Authentication failed for 'x'"),
            SourceError::AuthRequired
        );
        assert_eq!(
            k(" ! [rejected] main -> main (fetch first)"),
            SourceError::Rejected
        );
        assert_eq!(
            k(" ! [rejected] main -> main (non-fast-forward)"),
            SourceError::Rejected
        );
        assert_eq!(
            k("fatal: Not possible to fast-forward, aborting."),
            SourceError::Diverged
        );
        assert_eq!(
            k(
                "fatal: unable to access 'https://x/': Operation too slow. Less than 1000 bytes/sec transferred the last 60 seconds"
            ),
            SourceError::Stalled
        );
        assert_eq!(
            k("Connection to host closed by remote host."),
            SourceError::Stalled
        );
        assert_eq!(
            k("ssh: Could not resolve hostname x"),
            SourceError::Unreachable
        );
        assert_eq!(
            k("fatal: unable to access 'https://x/': Could not resolve host: x"),
            SourceError::Unreachable
        );
        assert_eq!(
            k("ssh: connect to host x port 22: Connection timed out"),
            SourceError::Unreachable
        );
        assert_eq!(
            k("There is no tracking information for the current branch."),
            SourceError::NoUpstream
        );
        let mut o = out("");
        o.timed_out = true;
        assert_eq!(classify_net(&o), SourceError::Stalled);
    }
}
