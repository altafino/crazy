//! RepoService: the user's own Git repository (PRD M3 steps 3 and 5, M3-D2/D9/D11/D12/
//! D13/D15/D16/D17/D21/D26/D30/D31).
//!
//! Unlike `SnapshotStore` (shadow repo, Studio author) this runs plain git against the
//! project's repo with a clean environment. The project folder may be the repo root or
//! a subfolder of a bigger repo, so every pathspec is scoped to the folder: git runs in
//! the repo top-level with root-relative paths (`prefix + project-relative path`), and
//! all paths the UI sees are project-relative.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{
    FileDiff, MAX_DIFF_LINES, MAX_FILE_BYTES, MAX_FULL_DIFF_LINES, SnapshotStore, is_secret, lossy,
    nul_split, parse_unified, write_atomic,
};
use crate::persistence::Db;
use crate::platform::process::spawn_group;

const STATUS_TIMEOUT: Duration = Duration::from_secs(30);
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Hooks and signing may be slow (D13); 2 minutes by default.
pub(crate) const COMMIT_TIMEOUT: Duration = Duration::from_secs(120);
const KILL_GRACE: Duration = Duration::from_secs(3);
const MAX_ENTRIES: usize = 5000;
const HISTORY: u32 = 50;
const RESTORE_REF: &str = "refs/studio/restore/last";
const RESTORE_MARKER: &str = "studio-restore: ";
const MAX_SCAN_BYTES: u64 = 5 * 1024 * 1024;
const MAX_FINDINGS: usize = 200;

// ---- types ----------------------------------------------------------------

/// Typed errors the UI maps to its states (design review: clean tree, not a repo,
/// conflicts, locked repo, hook output, identity form).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type, thiserror::Error)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum SourceError {
    #[error("This folder is not a Git repository.")]
    NotARepo,
    #[error("This folder is already inside the Git repository at {top}")]
    AlreadyInRepo { top: String },
    #[error("Git does not know your name and email yet.")]
    IdentityMissing,
    #[error("Another Git process is using this repository (index.lock exists).")]
    RepoLocked,
    #[error("A Git hook rejected the commit.")]
    HookRejected { output: String },
    #[error("The commit took too long and was stopped.")]
    CommitTimedOut,
    #[error("Reading the Git status took too long.")]
    StatusTimedOut,
    #[error("An agent run that changes files is still active.")]
    RunInProgress,
    #[error("The repository has merge conflicts.")]
    ConflictsPresent,
    #[error("Your files have uncommitted changes.")]
    DirtyTree { count: u32 },
    #[error("There is nothing to commit.")]
    NothingToCommit,
    #[error("A branch with this name already exists.")]
    BranchExists,
    #[error("That branch does not exist.")]
    BranchNotFound,
    #[error("That is not a valid branch name.")]
    InvalidBranchName,
    #[error("Some files look like secrets.")]
    SecretsFound { paths: Vec<String> },
    #[error("There is no restore to undo.")]
    NoUndo,
    #[error("{message}")]
    Invalid { message: String },
    #[error("{message}")]
    Git { message: String },
}
type Result<T> = std::result::Result<T, SourceError>;

fn invalid(m: impl Into<String>) -> SourceError {
    SourceError::Invalid { message: m.into() }
}
fn git_err(m: impl Into<String>) -> SourceError {
    SourceError::Git { message: m.into() }
}
impl From<std::io::Error> for SourceError {
    fn from(e: std::io::Error) -> Self {
        git_err(format!("io: {e}"))
    }
}
impl From<crate::persistence::DbError> for SourceError {
    fn from(e: crate::persistence::DbError) -> Self {
        git_err(e.to_string())
    }
}
impl From<super::GitError> for SourceError {
    fn from(e: super::GitError) -> Self {
        git_err(e.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepoLocation {
    pub top: String,
    /// Project folder relative to `top`, "" or "sub/dir/".
    pub prefix: String,
    pub common_dir: String,
    /// The project is a subfolder of a larger repo (monorepo).
    pub is_subfolder: bool,
    /// `top` is a linked worktree or submodule (its `.git` is a file).
    pub is_linked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum HeadState {
    Branch {
        name: String,
    },
    Detached {
        short: String,
    },
    /// No commit yet; `name` is the branch the first commit will create.
    Unborn {
        name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum EntryKind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StatusEntry {
    /// Project-relative, lossy when the real name is not UTF-8 (`non_utf8`).
    pub path: String,
    pub old_path: Option<String>,
    pub kind: EntryKind,
    /// Something is staged for this path.
    pub staged: bool,
    pub unstaged: bool,
    pub submodule: bool,
    pub non_utf8: bool,
    pub binary: bool,
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
    pub secret: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommitInfo {
    pub hash: String,
    pub short: String,
    pub subject: String,
    pub author: String,
    /// Unix seconds; the UI renders "3 hours ago".
    pub time: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepoStatus {
    pub location: RepoLocation,
    pub head: HeadState,
    pub entries: Vec<StatusEntry>,
    /// Entries before the 5,000 cap.
    pub total: u32,
    pub truncated: bool,
    pub conflicts: u32,
    /// Staged entries not in the ticked set; `commit --only` leaves them staged.
    pub staged_outside: u32,
    pub last_commit: Option<CommitInfo>,
    pub undo_restore_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommitOutcome {
    pub commit: CommitInfo,
    pub files: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Identity {
    pub name: String,
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IdentityInfo {
    pub name: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SecretFinding {
    pub path: String,
    /// 1-based line of an added line; None for a secret-looking file name.
    pub line: Option<u32>,
    pub kind: String,
    pub masked: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchList {
    pub head: HeadState,
    pub branches: Vec<String>,
    /// Switching changes the whole repo, not just this folder.
    pub affects_whole_repo: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum LostReason {
    Secret,
    TooLarge,
    /// The staged state is not brought back by Undo (file contents are).
    StagedState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CannotUndo {
    pub path: String,
    pub reason: LostReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RestoreReport {
    pub restored: Vec<String>,
    pub cannot_undo: Vec<CannotUndo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InitRequest {
    /// Used (and saved locally) only when git has no identity of its own.
    pub identity: Option<Identity>,
    /// The user saw the secret warning and wants the commit anyway (D17).
    pub confirm_secrets: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InitOutcome {
    pub top: String,
    /// False for an empty folder: the repo exists but has no commit.
    pub commit: Option<CommitInfo>,
}

// ---- service --------------------------------------------------------------

pub(crate) struct RepoService {
    program: PathBuf,
    env: Vec<(String, String)>,
    db: Arc<Db>,
    snapshots: Arc<SnapshotStore>,
    commit_timeout: Duration,
}

struct Out {
    ok: bool,
    stdout: Vec<u8>,
    stderr: String,
    timed_out: bool,
}
impl Out {
    fn text(&self) -> String {
        lossy(&self.stdout).trim().to_owned()
    }
    /// stdout and stderr joined, for hook output shown to the user.
    fn combined(&self) -> String {
        format!("{}\n{}", lossy(&self.stdout), self.stderr)
            .trim()
            .to_owned()
    }
}

impl RepoService {
    pub(crate) fn new(
        program: PathBuf,
        env: Vec<(String, String)>,
        db: Arc<Db>,
        snapshots: Arc<SnapshotStore>,
    ) -> Self {
        Self {
            program,
            env,
            db,
            snapshots,
            commit_timeout: COMMIT_TIMEOUT,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_commit_timeout(mut self, t: Duration) -> Self {
        self.commit_timeout = t;
        self
    }

    // ---- process plumbing

    async fn exec(
        &self,
        cwd: &Path,
        args: &[&str],
        stdin: Option<Vec<u8>>,
        limit: Duration,
        read_only: bool,
    ) -> Result<Out> {
        let mut env = self.env.clone();
        env.retain(|(k, _)| {
            !["LC_ALL", "GIT_LITERAL_PATHSPECS", "GIT_TERMINAL_PROMPT"].contains(&k.as_str())
        });
        // English messages are matched below; literal pathspecs keep odd names inert.
        env.push(("LC_ALL".into(), "C".into()));
        env.push(("GIT_LITERAL_PATHSPECS".into(), "1".into()));
        env.push(("GIT_TERMINAL_PROMPT".into(), "0".into()));
        if read_only {
            // Status must not take index.lock or fight a running commit.
            env.push(("GIT_OPTIONAL_LOCKS".into(), "0".into()));
        }
        let mut g = spawn_group(
            &self.program,
            args,
            &env,
            Some(cwd),
            if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            },
        )
        .await
        .map_err(|e| git_err(format!("{}: {e}", self.program.display())))?;
        let (mut so, mut se) = (
            g.child.stdout.take().expect("piped"),
            g.child.stderr.take().expect("piped"),
        );
        let mut si = g.child.stdin.take();
        let (mut ob, mut eb) = (Vec::new(), Vec::new());
        let work = async {
            let w = async {
                if let (Some(data), Some(si)) = (stdin, si.as_mut()) {
                    let _ = si.write_all(&data).await;
                }
                drop(si.take());
            };
            let (_, _, _, st) = tokio::join!(
                w,
                so.read_to_end(&mut ob),
                se.read_to_end(&mut eb),
                g.child.wait()
            );
            st
        };
        match tokio::time::timeout(limit, work).await {
            Ok(st) => Ok(Out {
                ok: st?.success(),
                stdout: ob,
                stderr: lossy(&eb),
                timed_out: false,
            }),
            Err(_) => {
                // SIGTERM first so git removes its own index.lock; never delete it ourselves.
                let _ = g.terminate_group(KILL_GRACE).await;
                Ok(Out {
                    ok: false,
                    stdout: ob,
                    stderr: lossy(&eb),
                    timed_out: true,
                })
            }
        }
    }

    /// Runs git, failing with its stderr on a non-zero exit.
    async fn git(&self, cwd: &Path, args: &[&str]) -> Result<Out> {
        let o = self.exec(cwd, args, None, READ_TIMEOUT, false).await?;
        if o.timed_out {
            return Err(git_err("git took too long"));
        }
        if o.ok { Ok(o) } else { Err(classify(&o)) }
    }

    /// `args` with `--pathspec-from-file=-`, so thousands of paths cannot overflow argv.
    async fn git_paths(&self, cwd: &Path, args: &[&str], paths: &[String]) -> Result<Out> {
        let mut a = args.to_vec();
        a.extend(["--pathspec-from-file=-", "--pathspec-file-nul"]);
        let o = self
            .exec(cwd, &a, Some(nul_join(paths)), self.commit_timeout, false)
            .await?;
        if o.timed_out {
            return Err(SourceError::CommitTimedOut);
        }
        if o.ok { Ok(o) } else { Err(classify(&o)) }
    }

    // ---- location (M3-D26)

    pub(crate) async fn locate(&self, project: &Path) -> Result<Loc> {
        if !project.is_dir() {
            return Err(invalid("The project folder does not exist."));
        }
        let o = self
            .exec(
                project,
                &[
                    "rev-parse",
                    "--show-toplevel",
                    "--git-common-dir",
                    "--show-prefix",
                ],
                None,
                READ_TIMEOUT,
                true,
            )
            .await?;
        if !o.ok {
            return if o.stderr.contains("not a git repository") {
                Err(SourceError::NotARepo)
            } else {
                Err(git_err(o.stderr.trim()))
            };
        }
        let text = lossy(&o.stdout);
        let mut lines = text.split('\n');
        let top = PathBuf::from(lines.next().unwrap_or_default());
        let common = lines.next().unwrap_or_default();
        let prefix = lines.next().unwrap_or_default().to_owned();
        if top.as_os_str().is_empty() {
            return Err(git_err("bare repositories are not supported"));
        }
        let common = if Path::new(common).is_absolute() {
            PathBuf::from(common)
        } else {
            project.join(common)
        };
        Ok(Loc {
            is_linked: top.join(".git").is_file(),
            top,
            prefix,
            common,
        })
    }

    fn require_idle(&self, project_id: &str) -> Result<()> {
        if self.db.has_running_modifying_job(project_id)? {
            Err(SourceError::RunInProgress)
        } else {
            Ok(())
        }
    }

    // ---- identity (M3-D12)

    pub(crate) async fn identity(&self, cwd: &Path) -> Result<IdentityInfo> {
        let get = |key: &'static str| async move {
            let o = self
                .exec(cwd, &["config", "--get", key], None, READ_TIMEOUT, true)
                .await?;
            Ok::<_, SourceError>(Some(o.text()).filter(|v| o.ok && !v.is_empty()))
        };
        Ok(IdentityInfo {
            name: get("user.name").await?,
            email: get("user.email").await?,
        })
    }

    async fn require_identity(&self, cwd: &Path) -> Result<()> {
        let i = self.identity(cwd).await?;
        if i.name.is_some() && i.email.is_some() {
            Ok(())
        } else {
            Err(SourceError::IdentityMissing)
        }
    }

    /// Writes `--local` config only; the user's global config is never touched.
    pub(crate) async fn set_identity(&self, project: &Path, id: &Identity) -> Result<()> {
        check_identity(id)?;
        let loc = self.locate(project).await?;
        self.git(
            &loc.top,
            &["config", "--local", "user.name", id.name.trim()],
        )
        .await?;
        self.git(
            &loc.top,
            &["config", "--local", "user.email", id.email.trim()],
        )
        .await?;
        Ok(())
    }

    // ---- status (M3-D21)

    async fn has_head(&self, loc: &Loc) -> Result<bool> {
        let o = self
            .exec(
                &loc.top,
                &["rev-parse", "--verify", "-q", "HEAD"],
                None,
                READ_TIMEOUT,
                true,
            )
            .await?;
        Ok(o.ok)
    }

    async fn raw_status(&self, loc: &Loc) -> Result<Parsed> {
        let mut args = vec![
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=all",
        ];
        if let Some(p) = loc.pathspec() {
            args.extend(["--", p]);
        }
        let o = self
            .exec(&loc.top, &args, None, STATUS_TIMEOUT, true)
            .await?;
        if o.timed_out {
            return Err(SourceError::StatusTimedOut);
        }
        if !o.ok {
            return Err(classify(&o));
        }
        Ok(parse_status(&o.stdout))
    }

    pub(crate) async fn status(
        &self,
        project_id: &str,
        project: &Path,
        ticked: &[String],
    ) -> Result<RepoStatus> {
        let loc = self.locate(project).await?;
        let parsed = self.raw_status(&loc).await?;
        let has_head = !matches!(parsed.head, HeadState::Unborn { .. });
        let nums = self.numstat(&loc, has_head).await?;
        let ticked: HashSet<&str> = ticked.iter().map(String::as_str).collect();

        let mut staged_outside = 0u32;
        let mut conflicts = 0u32;
        let mut entries: Vec<StatusEntry> = Vec::new();
        for (i, raw) in parsed.entries.iter().enumerate() {
            let e = raw.to_entry(&loc.prefix);
            if e.kind == EntryKind::Conflicted {
                conflicts += 1;
            }
            if e.staged && !ticked.contains(e.path.as_str()) {
                staged_outside += 1;
            }
            if i < MAX_ENTRIES {
                entries.push(e);
            }
        }
        for e in &mut entries {
            if e.kind == EntryKind::Untracked {
                let (a, binary) = count_lines(&project_file(project, &e.path));
                e.additions = a;
                e.binary = binary;
            } else if let Some(&(a, d)) = nums.get(&e.path) {
                e.additions = a;
                e.deletions = d;
                e.binary = a.is_none();
            }
        }
        entries.sort_by(|a, b| {
            (a.kind != EntryKind::Conflicted, &a.path)
                .cmp(&(b.kind != EntryKind::Conflicted, &b.path))
        });
        let total = u32::try_from(parsed.entries.len()).unwrap_or(u32::MAX);
        Ok(RepoStatus {
            location: loc.info(),
            head: parsed.head,
            entries,
            total,
            truncated: parsed.entries.len() > MAX_ENTRIES,
            conflicts,
            staged_outside,
            last_commit: if has_head {
                self.log(&loc, 1).await?.into_iter().next()
            } else {
                None
            },
            undo_restore_available: self.undo_available(project_id).await,
        })
    }

    /// project-relative path -> (additions, deletions); None counts = binary.
    async fn numstat(
        &self,
        loc: &Loc,
        has_head: bool,
    ) -> Result<HashMap<String, (Option<u32>, Option<u32>)>> {
        let mut args = vec!["diff", "--numstat", "-z", "-M", "--no-ext-diff"];
        args.push(if has_head { "HEAD" } else { "--cached" });
        if let Some(p) = loc.pathspec() {
            args.extend(["--", p]);
        }
        let o = self
            .exec(&loc.top, &args, None, STATUS_TIMEOUT, true)
            .await?;
        if o.timed_out {
            return Err(SourceError::StatusTimedOut);
        }
        if !o.ok {
            return Err(classify(&o));
        }
        Ok(parse_numstat(&o.stdout, &loc.prefix))
    }

    async fn log(&self, loc: &Loc, n: u32) -> Result<Vec<CommitInfo>> {
        let n = format!("-{n}");
        let mut args = vec!["log", &n, "-z", "--format=%H%x1f%an%x1f%at%x1f%s"];
        if let Some(p) = loc.pathspec() {
            args.extend(["--", p]);
        }
        let o = self.git(&loc.top, &args).await?;
        Ok(nul_split(&o.stdout).filter_map(parse_commit).collect())
    }

    // ---- history (step 3)

    pub(crate) async fn history(&self, project: &Path) -> Result<Vec<CommitInfo>> {
        let loc = self.locate(project).await?;
        if !self.has_head(&loc).await? {
            return Ok(Vec::new());
        }
        self.log(&loc, HISTORY).await
    }

    // ---- file diff (M3 step 3)

    pub(crate) async fn file_diff(
        &self,
        project: &Path,
        path: &str,
        old_path: Option<&str>,
        full: bool,
    ) -> Result<FileDiff> {
        let loc = self.locate(project).await?;
        let rel = loc.root_rel(path)?;
        let old = old_path.map(|p| loc.root_rel(p)).transpose()?;
        let has_head = self.has_head(&loc).await?;
        let tracked = !self
            .git(&loc.top, &["ls-files", "-z", "--", &rel])
            .await?
            .stdout
            .is_empty();
        let mut args = vec![
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "-U3",
        ];
        if !tracked {
            if !project.join(path).exists() {
                return Err(invalid("That file does not exist."));
            }
            args.extend(["--no-index", "--", "/dev/null", &rel]);
        } else {
            args.extend(["-M", if has_head { "HEAD" } else { "--cached" }, "--", &rel]);
            if let Some(o) = &old {
                args.push(o);
            }
        }
        let o = self.exec(&loc.top, &args, None, READ_TIMEOUT, true).await?;
        // `--no-index` exits 1 when the files differ.
        if o.timed_out || (!o.ok && tracked) || (!o.ok && !o.stdout.starts_with(b"diff ")) {
            return Err(classify(&o));
        }
        let mut d = parse_unified(
            &lossy(&o.stdout),
            if full {
                MAX_FULL_DIFF_LINES
            } else {
                MAX_DIFF_LINES
            },
        );
        if d.binary {
            d.new_size = std::fs::metadata(project.join(path))
                .ok()
                .and_then(|m| u32::try_from(m.len()).ok());
            if tracked && has_head {
                let spec = format!("HEAD:{}", old.as_deref().unwrap_or(&rel));
                let s = self
                    .exec(
                        &loc.top,
                        &["cat-file", "-s", &spec],
                        None,
                        READ_TIMEOUT,
                        true,
                    )
                    .await?;
                d.old_size = s.text().parse().ok().filter(|_| s.ok);
            }
        }
        Ok(d)
    }

    // ---- commit (M3-D13, D15, D16, D17)

    async fn conflicts_in_scope(&self, loc: &Loc) -> Result<bool> {
        let mut args = vec!["ls-files", "-u", "-z"];
        if let Some(p) = loc.pathspec() {
            args.extend(["--", p]);
        }
        Ok(!self.git(&loc.top, &args).await?.stdout.is_empty())
    }

    /// Ticked paths -> their status entries (plus rename origins), project-relative.
    fn pick<'a>(&self, parsed: &'a Parsed, loc: &Loc, ticked: &[String]) -> Result<Vec<&'a Raw>> {
        if ticked.is_empty() {
            return Err(invalid("Nothing is ticked."));
        }
        let mut out: Vec<&Raw> = Vec::new();
        for t in ticked {
            loc.root_rel(t)?;
            let want = format!("{}{t}", loc.prefix);
            let Some(raw) = parsed.entries.iter().find(|e| e.path == want.as_bytes()) else {
                return Err(invalid(format!("{t} has no changes.")));
            };
            if raw.non_utf8() {
                return Err(invalid(format!(
                    "{t}: file names that are not UTF-8 are not supported."
                )));
            }
            if !out.iter().any(|o| std::ptr::eq(*o, raw)) {
                out.push(raw);
            }
        }
        Ok(out)
    }

    pub(crate) async fn commit(
        &self,
        project: &Path,
        message: &str,
        ticked: &[String],
    ) -> Result<CommitOutcome> {
        let message = message.trim();
        if message.is_empty() {
            return Err(invalid("Write a commit message."));
        }
        let loc = self.locate(project).await?;
        self.require_identity(&loc.top).await?;
        if self.conflicts_in_scope(&loc).await? {
            return Err(SourceError::ConflictsPresent);
        }
        let parsed = self.raw_status(&loc).await?;
        let picked = self.pick(&parsed, &loc, ticked)?;
        if picked.iter().any(|r| r.kind() == EntryKind::Conflicted) {
            return Err(SourceError::ConflictsPresent);
        }
        let mut all: Vec<String> = Vec::new();
        let mut to_add: Vec<String> = Vec::new();
        for r in &picked {
            let p = lossy(&r.path);
            if r.kind() == EntryKind::Untracked {
                to_add.push(p.clone());
            }
            all.push(p);
            all.extend(r.old.as_deref().map(lossy));
        }
        if !to_add.is_empty()
            && let Err(e) = self.git_paths(&loc.top, &["add"], &to_add).await
        {
            self.unstage(&loc, &to_add).await;
            return Err(e);
        }
        let res = self
            .exec(
                &loc.top,
                &[
                    "commit",
                    "--only",
                    "-m",
                    message,
                    "--pathspec-from-file=-",
                    "--pathspec-file-nul",
                ],
                Some(nul_join(&all)),
                self.commit_timeout,
                false,
            )
            .await;
        match res {
            Ok(o) if o.ok => (),
            Ok(o) => {
                self.unstage(&loc, &to_add).await;
                return Err(if o.timed_out {
                    SourceError::CommitTimedOut
                } else {
                    self.classify_commit(&loc, &o).await
                });
            }
            Err(e) => {
                self.unstage(&loc, &to_add).await;
                return Err(e);
            }
        }
        let commit = self
            .log(&loc, 1)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| git_err("the commit was not found afterwards"))?;
        Ok(CommitOutcome {
            commit,
            files: u32::try_from(picked.len()).unwrap_or(u32::MAX),
        })
    }

    /// Undo only what we staged ourselves (M3-D16); never touches the lock file.
    async fn unstage(&self, loc: &Loc, added: &[String]) {
        if !added.is_empty() {
            let _ = self.git_paths(&loc.top, &["reset", "-q"], added).await;
        }
    }

    async fn classify_commit(&self, loc: &Loc, o: &Out) -> SourceError {
        let e = classify(o);
        if !matches!(&e, SourceError::Git { .. }) {
            return e;
        }
        if self.has_commit_hooks(loc).await {
            return SourceError::HookRejected {
                output: o.combined(),
            };
        }
        e
    }

    async fn has_commit_hooks(&self, loc: &Loc) -> bool {
        let Ok(o) = self
            .exec(
                &loc.top,
                &["rev-parse", "--git-path", "hooks"],
                None,
                READ_TIMEOUT,
                true,
            )
            .await
        else {
            return false;
        };
        let dir = loc.top.join(o.text());
        [
            "pre-commit",
            "prepare-commit-msg",
            "commit-msg",
            "post-commit",
        ]
        .iter()
        .any(|h| dir.join(h).is_file())
    }

    // ---- secret check (M3-D17)

    pub(crate) async fn secret_check(
        &self,
        project: &Path,
        ticked: &[String],
    ) -> Result<Vec<SecretFinding>> {
        let loc = self.locate(project).await?;
        let has_head = self.has_head(&loc).await?;
        let parsed = self.raw_status(&loc).await?;
        let mut out = Vec::new();
        for r in self.pick(&parsed, &loc, ticked)? {
            let path = r.to_entry(&loc.prefix).path;
            if is_secret(&path) {
                out.push(SecretFinding {
                    path: path.clone(),
                    line: None,
                    kind: "secret file".into(),
                    masked: path.clone(),
                });
            }
            let added: Vec<(u32, String)> = match r.kind() {
                EntryKind::Deleted | EntryKind::Conflicted => continue,
                EntryKind::Untracked => read_lines(&project_file(project, &path)),
                _ => {
                    let rel = lossy(&r.path);
                    let base = if has_head { "HEAD" } else { "--cached" };
                    let o = self
                        .git(
                            &loc.top,
                            &[
                                "diff",
                                "-U0",
                                "--no-color",
                                "--no-ext-diff",
                                "--no-textconv",
                                base,
                                "--",
                                &rel,
                            ],
                        )
                        .await?;
                    added_lines(&lossy(&o.stdout))
                }
            };
            for (line, text) in added {
                for (kind, masked) in scan_line(&text) {
                    if out.len() >= MAX_FINDINGS {
                        return Ok(out);
                    }
                    out.push(SecretFinding {
                        path: path.clone(),
                        line: Some(line),
                        kind,
                        masked,
                    });
                }
            }
        }
        Ok(out)
    }

    // ---- branches (M3-D2)

    pub(crate) async fn branches(&self, project: &Path) -> Result<BranchList> {
        let loc = self.locate(project).await?;
        let head = self.raw_status(&loc).await?.head;
        Ok(BranchList {
            head,
            branches: self.local_branches(&loc).await?,
            affects_whole_repo: loc.is_subfolder(),
        })
    }

    async fn local_branches(&self, loc: &Loc) -> Result<Vec<String>> {
        let o = self
            .git(
                &loc.top,
                &["for-each-ref", "--format=%(refname:short)", "refs/heads/"],
            )
            .await?;
        Ok(lossy(&o.stdout).lines().map(str::to_owned).collect())
    }

    pub(crate) async fn create_branch(&self, project: &Path, name: &str) -> Result<BranchList> {
        let loc = self.locate(project).await?;
        let name = name.trim();
        if name.is_empty() || name.starts_with('-') {
            return Err(SourceError::InvalidBranchName);
        }
        let ok = self
            .exec(
                &loc.top,
                &["check-ref-format", "--branch", name],
                None,
                READ_TIMEOUT,
                true,
            )
            .await?;
        if !ok.ok {
            return Err(SourceError::InvalidBranchName);
        }
        if self.local_branches(&loc).await?.iter().any(|b| b == name) {
            return Err(SourceError::BranchExists);
        }
        self.git(&loc.top, &["switch", "-c", name]).await?;
        self.branches(project).await
    }

    pub(crate) async fn switch_branch(
        &self,
        project_id: &str,
        project: &Path,
        name: &str,
    ) -> Result<BranchList> {
        self.require_idle(project_id)?;
        let loc = self.locate(project).await?;
        if !self.local_branches(&loc).await?.iter().any(|b| b == name) {
            return Err(SourceError::BranchNotFound);
        }
        // The whole repo is affected, so the whole repo must be clean (M3-D2).
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
        if !dirty.is_empty() {
            return Err(SourceError::DirtyTree {
                count: u32::try_from(dirty.len()).unwrap_or(u32::MAX),
            });
        }
        self.git(&loc.top, &["switch", name]).await?;
        self.branches(project).await
    }

    // ---- restore (M3-D11, D31)

    fn lost_report(&self, project: &Path, loc: &Loc, picked: &[&Raw]) -> Vec<CannotUndo> {
        let mut out = Vec::new();
        for r in picked {
            let e = r.to_entry(&loc.prefix);
            let on_disk = std::fs::symlink_metadata(project_file(project, &e.path)).ok();
            if let Some(m) = on_disk {
                if is_secret(&e.path) {
                    out.push(CannotUndo {
                        path: e.path.clone(),
                        reason: LostReason::Secret,
                    });
                } else if m.is_file() && m.len() > MAX_FILE_BYTES {
                    out.push(CannotUndo {
                        path: e.path.clone(),
                        reason: LostReason::TooLarge,
                    });
                }
            }
            if e.staged {
                out.push(CannotUndo {
                    path: e.path,
                    reason: LostReason::StagedState,
                });
            }
        }
        out
    }

    /// What Restore could not bring back with Undo, before anything is changed.
    pub(crate) async fn restore_preview(
        &self,
        project: &Path,
        ticked: &[String],
    ) -> Result<Vec<CannotUndo>> {
        let loc = self.locate(project).await?;
        let parsed = self.raw_status(&loc).await?;
        let picked = self.pick(&parsed, &loc, ticked)?;
        Ok(self.lost_report(project, &loc, &picked))
    }

    pub(crate) async fn restore(
        &self,
        project_id: &str,
        project: &Path,
        ticked: &[String],
    ) -> Result<RestoreReport> {
        self.require_idle(project_id)?;
        let loc = self.locate(project).await?;
        let parsed = self.raw_status(&loc).await?;
        let picked = self.pick(&parsed, &loc, ticked)?;
        if picked.iter().any(|r| r.kind() == EntryKind::Conflicted) {
            return Err(SourceError::ConflictsPresent);
        }
        let cannot_undo = self.lost_report(project, &loc, &picked);

        // New files (untracked or only in the index) are removed; everything else
        // comes back from HEAD. A staged rename is a new name plus an old one.
        let (mut from_head, mut index_new, mut untracked) = (Vec::new(), Vec::new(), Vec::new());
        for r in &picked {
            let p = lossy(&r.path);
            match (r.kind(), r.xy[0]) {
                (EntryKind::Untracked, _) => untracked.push(p),
                (_, b'A' | b'C') => index_new.push(p),
                (_, b'R') => {
                    index_new.push(p);
                    from_head.extend(r.old.as_deref().map(lossy));
                }
                _ => from_head.push(p),
            }
        }
        let shown: Vec<String> = from_head
            .iter()
            .chain(&index_new)
            .chain(&untracked)
            .map(|p| p.strip_prefix(&loc.prefix).unwrap_or(p).to_owned())
            .collect();

        // Keep-1 safety net: shadow snapshot of the files as they are now.
        let label = format!(
            "restore {RESTORE_MARKER}{}",
            serde_json::to_string(&shown).map_err(|e| git_err(e.to_string()))?
        );
        self.snapshots
            .snapshot_inner(project_id, project, &label, Some(RESTORE_REF.into()))?;

        if !index_new.is_empty() {
            self.git_paths(&loc.top, &["reset", "-q"], &index_new)
                .await?;
        }
        if !from_head.is_empty() {
            self.git_paths(
                &loc.top,
                &["restore", "--source=HEAD", "--staged", "--worktree"],
                &from_head,
            )
            .await?;
        }
        for p in index_new.iter().chain(&untracked) {
            let f = loc.top.join(p);
            if std::fs::symlink_metadata(&f).is_ok_and(|m| !m.is_dir()) {
                std::fs::remove_file(&f)?;
            }
        }
        Ok(RestoreReport {
            restored: shown,
            cannot_undo,
        })
    }

    async fn undo_available(&self, project_id: &str) -> bool {
        let Ok(ctx) = self.snapshots.ctx(project_id) else {
            return false;
        };
        self.snapshots
            .git
            .run(&ctx, &["rev-parse", "--verify", "-q", RESTORE_REF], None)
            .is_ok()
    }

    /// Puts the files back as they were before the latest Restore (only that one).
    pub(crate) async fn undo_restore(&self, project_id: &str, project: &Path) -> Result<()> {
        self.require_idle(project_id)?;
        let ctx = self.snapshots.ctx(project_id)?;
        let g = &self.snapshots.git;
        let commit = g
            .run(&ctx, &["rev-parse", "--verify", "-q", RESTORE_REF], None)
            .map(|b| lossy(&b).trim().to_owned())
            .map_err(|_| SourceError::NoUndo)?;
        let body = g.text(&ctx, &["cat-file", "commit", &commit])?;
        let paths: Vec<String> = body
            .lines()
            .find_map(|l| l.strip_prefix(&format!("restore {RESTORE_MARKER}")))
            .and_then(|j| serde_json::from_str(j).ok())
            .ok_or(SourceError::NoUndo)?;
        let skipped = self.snapshots.skipped(project_id, &commit)?;
        let work = Context {
            shadow: ctx.shadow.clone(),
            work: Some(project.to_owned()),
            index: None,
        };
        for p in paths {
            if super::is_skipped(&p, &skipped).is_some() {
                continue; // never captured, so there is nothing to put back
            }
            let target = project.join(&p);
            match self.snapshots.tree_entry(&work, &commit, &p)? {
                Some((mode, oid)) => {
                    let blob = g.run(&work, &["cat-file", "blob", &oid], None)?;
                    write_atomic(&target, &blob, &mode)?;
                }
                None => {
                    if std::fs::symlink_metadata(&target).is_ok_and(|m| !m.is_dir()) {
                        std::fs::remove_file(&target)?;
                    }
                }
            }
        }
        g.run(&ctx, &["update-ref", "-d", RESTORE_REF], None)?;
        Ok(())
    }

    // ---- initialize git (M3-D26, D12, D17)

    pub(crate) async fn init_repo(
        &self,
        project_id: &str,
        project: &Path,
        req: &InitRequest,
    ) -> Result<InitOutcome> {
        self.require_idle(project_id)?;
        match self.locate(project).await {
            Ok(loc) => {
                return Err(SourceError::AlreadyInRepo {
                    top: loc.top.display().to_string(),
                });
            }
            Err(SourceError::NotARepo) => {}
            Err(e) => return Err(e),
        }
        let have = self.identity(project).await?;
        let own = have.name.is_some() && have.email.is_some();
        let given = match (&req.identity, own) {
            (_, true) => None,
            (Some(i), false) => {
                check_identity(i)?;
                Some(i)
            }
            (None, false) => return Err(SourceError::IdentityMissing),
        };

        self.git(project, &["init", "-q", "--initial-branch=main"])
            .await?;
        // We just created it (there was no repo), so on failure it is ours to remove.
        let undo = |e: SourceError| {
            let _ = std::fs::remove_dir_all(project.join(".git"));
            e
        };
        let files = self
            .git(
                project,
                &["ls-files", "--others", "--exclude-standard", "-z"],
            )
            .await
            .map_err(undo)?;
        let files: Vec<String> = nul_split(&files.stdout).map(lossy).collect();
        let secrets: Vec<String> = files.iter().filter(|f| is_secret(f)).cloned().collect();
        if !secrets.is_empty() && !req.confirm_secrets {
            return Err(undo(SourceError::SecretsFound { paths: secrets }));
        }
        let top = project
            .canonicalize()
            .unwrap_or_else(|_| project.to_owned());
        if files.is_empty() {
            return Ok(InitOutcome {
                top: top.display().to_string(),
                commit: None,
            });
        }
        self.git(project, &["add", "-A"]).await.map_err(undo)?;
        let mut args: Vec<String> = Vec::new();
        if let Some(i) = given {
            args.extend([
                "-c".into(),
                format!("user.name={}", i.name.trim()),
                "-c".into(),
                format!("user.email={}", i.email.trim()),
            ]);
        }
        args.extend([
            "commit".into(),
            "-q".into(),
            "-m".into(),
            "Initial commit".into(),
        ]);
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        let o = self
            .exec(project, &a, None, self.commit_timeout, false)
            .await
            .map_err(undo)?;
        if !o.ok {
            return Err(undo(if o.timed_out {
                SourceError::CommitTimedOut
            } else {
                classify(&o)
            }));
        }
        if let Some(i) = given {
            self.set_identity(project, i).await?;
        }
        let loc = self.locate(project).await?;
        let commit = self.log(&loc, 1).await?.into_iter().next();
        Ok(InitOutcome {
            top: loc.top.display().to_string(),
            commit,
        })
    }
}

// ---- helpers --------------------------------------------------------------

use super::Ctx as Context;

pub(crate) struct Loc {
    top: PathBuf,
    prefix: String,
    common: PathBuf,
    is_linked: bool,
}

impl Loc {
    fn is_subfolder(&self) -> bool {
        !self.prefix.is_empty()
    }
    /// Pathspec limiting a command to the project folder; None = whole repo.
    fn pathspec(&self) -> Option<&str> {
        self.is_subfolder()
            .then(|| self.prefix.trim_end_matches('/'))
    }
    fn info(&self) -> RepoLocation {
        RepoLocation {
            top: self.top.display().to_string(),
            prefix: self.prefix.clone(),
            common_dir: self.common.display().to_string(),
            is_subfolder: self.is_subfolder(),
            is_linked: self.is_linked,
        }
    }
    /// Validates a project-relative path and returns it root-relative.
    fn root_rel(&self, p: &str) -> Result<String> {
        let bad = p.is_empty()
            || p.contains('\0')
            || p.starts_with('/')
            || p.contains('\\')
            || p.split('/')
                .any(|c| c == ".." || c == "." || c.is_empty() || c == ".git");
        if bad {
            return Err(invalid(format!("not a project file path: {p}")));
        }
        Ok(format!("{}{p}", self.prefix))
    }
}

fn nul_join(paths: &[String]) -> Vec<u8> {
    let mut v = Vec::new();
    for p in paths {
        v.extend_from_slice(p.as_bytes());
        v.push(0);
    }
    v
}

fn project_file(project: &Path, rel: &str) -> PathBuf {
    project.join(rel)
}

fn check_identity(i: &Identity) -> Result<()> {
    let bad = |s: &str| s.trim().is_empty() || s.contains(['\n', '\r', '\0']);
    if bad(&i.name) || bad(&i.email) || !i.email.contains('@') {
        return Err(invalid("Enter a name and a valid email."));
    }
    Ok(())
}

fn classify(o: &Out) -> SourceError {
    let m = format!("{}\n{}", o.stderr, lossy(&o.stdout));
    if m.contains("index.lock") && (m.contains("File exists") || m.contains("Unable to create")) {
        SourceError::RepoLocked
    } else if m.contains("nothing to commit") || m.contains("no changes added to commit") {
        SourceError::NothingToCommit
    } else if m.contains("Author identity unknown") || m.contains("empty ident name") {
        SourceError::IdentityMissing
    } else if m.contains("not a git repository") {
        SourceError::NotARepo
    } else {
        git_err(m.trim())
    }
}

fn parse_commit(rec: &[u8]) -> Option<CommitInfo> {
    let s = lossy(rec);
    let mut f = s.splitn(4, '\x1f');
    let hash = f.next()?.to_owned();
    let author = f.next()?.to_owned();
    let time = f.next()?.parse().ok()?;
    let subject = f.next()?.to_owned();
    Some(CommitInfo {
        short: hash.chars().take(7).collect(),
        hash,
        subject,
        author,
        time,
    })
}

// ---- status parsing -------------------------------------------------------

#[derive(Debug)]
pub(crate) struct Raw {
    /// Root-relative raw bytes.
    path: Vec<u8>,
    old: Option<Vec<u8>>,
    xy: [u8; 2],
    unmerged: bool,
    untracked: bool,
    submodule: bool,
}

impl Raw {
    fn non_utf8(&self) -> bool {
        std::str::from_utf8(&self.path).is_err()
            || self
                .old
                .as_deref()
                .is_some_and(|o| std::str::from_utf8(o).is_err())
    }
    fn kind(&self) -> EntryKind {
        let [x, y] = self.xy;
        if self.unmerged {
            EntryKind::Conflicted
        } else if self.untracked {
            EntryKind::Untracked
        } else if x == b'R' || y == b'R' {
            EntryKind::Renamed
        } else if x == b'A' || x == b'C' {
            EntryKind::Added
        } else if x == b'D' || y == b'D' {
            EntryKind::Deleted
        } else {
            EntryKind::Modified
        }
    }
    pub(crate) fn to_entry(&self, prefix: &str) -> StatusEntry {
        let rel = |b: &[u8]| {
            let s = lossy(b);
            s.strip_prefix(prefix).map(str::to_owned).unwrap_or(s)
        };
        let path = rel(&self.path);
        StatusEntry {
            secret: is_secret(&path),
            path,
            old_path: self.old.as_deref().map(rel),
            kind: self.kind(),
            staged: !self.untracked && !self.unmerged && self.xy[0] != b'.',
            unstaged: !self.untracked && !self.unmerged && self.xy[1] != b'.',
            submodule: self.submodule,
            non_utf8: self.non_utf8(),
            binary: false,
            additions: None,
            deletions: None,
        }
    }
}

pub(crate) struct Parsed {
    pub(crate) head: HeadState,
    pub(crate) entries: Vec<Raw>,
}

/// `git status --porcelain=v2 -z --branch` (M3-D21).
pub(crate) fn parse_status(b: &[u8]) -> Parsed {
    let recs: Vec<&[u8]> = b.split(|c| *c == 0).collect();
    let (mut oid, mut head) = (Vec::new(), Vec::new());
    let mut entries = Vec::new();
    let mut i = 0;
    while i < recs.len() {
        let rec = recs[i];
        i += 1;
        if let Some(h) = rec.strip_prefix(b"# branch.oid ") {
            oid = h.to_vec();
        } else if let Some(h) = rec.strip_prefix(b"# branch.head ") {
            head = h.to_vec();
        } else if let Some(p) = rec.strip_prefix(b"? ") {
            entries.push(Raw {
                path: p.to_vec(),
                old: None,
                xy: *b"??",
                unmerged: false,
                untracked: true,
                submodule: false,
            });
        } else if rec.len() > 2 && matches!(rec[0], b'1' | b'2' | b'u') && rec[1] == b' ' {
            // Fixed leading fields, then the path (which may hold spaces).
            let n = match rec[0] {
                b'1' => 9,
                b'2' => 10,
                _ => 11,
            };
            let f: Vec<&[u8]> = rec.splitn(n, |c| *c == b' ').collect();
            if f.len() < n {
                continue;
            }
            let xy = [
                f[1].first().copied().unwrap_or(b'.'),
                f[1].get(1).copied().unwrap_or(b'.'),
            ];
            let old = if rec[0] == b'2' {
                let o = recs.get(i).map(|o| o.to_vec());
                i += 1;
                o
            } else {
                None
            };
            entries.push(Raw {
                path: f[n - 1].to_vec(),
                old,
                xy,
                unmerged: rec[0] == b'u',
                untracked: false,
                submodule: f[2].first() == Some(&b'S'),
            });
        }
    }
    let head = if oid == b"(initial)" {
        HeadState::Unborn { name: lossy(&head) }
    } else if head == b"(detached)" {
        HeadState::Detached {
            short: lossy(&oid).chars().take(7).collect(),
        }
    } else {
        HeadState::Branch { name: lossy(&head) }
    };
    Parsed { head, entries }
}

fn parse_numstat(b: &[u8], prefix: &str) -> HashMap<String, (Option<u32>, Option<u32>)> {
    let recs: Vec<&[u8]> = b.split(|c| *c == 0).collect();
    let mut map = HashMap::new();
    let mut i = 0;
    while i < recs.len() {
        let mut f = recs[i].splitn(3, |c| *c == b'\t');
        i += 1;
        let (Some(a), Some(d), Some(p)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let path = if p.is_empty() {
            i += 1; // rename: old, then new
            let new = recs.get(i).copied().unwrap_or_default();
            i += 1;
            new
        } else {
            p
        };
        let num = |x: &[u8]| std::str::from_utf8(x).ok().and_then(|s| s.parse().ok());
        let path = lossy(path);
        map.insert(
            path.strip_prefix(prefix).map(str::to_owned).unwrap_or(path),
            (num(a), num(d)),
        );
    }
    map
}

/// (added lines, is binary) of an untracked file; cheap for typical source files.
fn count_lines(p: &Path) -> (Option<u32>, bool) {
    let Ok(m) = std::fs::symlink_metadata(p) else {
        return (None, false);
    };
    if !m.is_file() || m.len() > MAX_SCAN_BYTES {
        return (None, false);
    }
    match std::fs::read(p) {
        Ok(b) if b.iter().take(8000).any(|c| *c == 0) => (None, true),
        Ok(b) => {
            let n = b.iter().filter(|c| **c == b'\n').count()
                + usize::from(b.last().is_some_and(|c| *c != b'\n'));
            (u32::try_from(n).ok(), false)
        }
        Err(_) => (None, false),
    }
}

// ---- secret patterns ------------------------------------------------------

/// (kind, regex, capture group holding the token).
static PATTERNS: LazyLock<Vec<(&'static str, Regex, usize)>> = LazyLock::new(|| {
    [
        (
            "private key",
            r"-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)?-----",
            0,
        ),
        (
            "AWS access key",
            r"(?:^|[^A-Za-z0-9])(AKIA[0-9A-Z]{16})(?:[^A-Za-z0-9]|$)",
            1,
        ),
        (
            "GitHub token",
            r"(?:^|[^A-Za-z0-9_])(gh[pousr]_[A-Za-z0-9]{36,})",
            1,
        ),
        (
            "GitHub token",
            r"(?:^|[^A-Za-z0-9_])(github_pat_[A-Za-z0-9_]{22,})",
            1,
        ),
        (
            "Anthropic key",
            r"(?:^|[^A-Za-z0-9])(sk-ant-[A-Za-z0-9_-]{20,})",
            1,
        ),
        (
            "API key",
            r"(?:^|[^A-Za-z0-9_-])(sk-(?:proj-)?[A-Za-z0-9_]{32,})",
            1,
        ),
        (
            "Slack token",
            r"(?:^|[^A-Za-z0-9])(xox[baprs]-[A-Za-z0-9-]{10,})",
            1,
        ),
        (
            "Google API key",
            r"(?:^|[^A-Za-z0-9_-])(AIza[0-9A-Za-z_-]{35})(?:[^0-9A-Za-z_-]|$)",
            1,
        ),
    ]
    .into_iter()
    .map(|(k, r, g)| (k, Regex::new(r).expect("static pattern"), g))
    .collect()
});

fn mask(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    if c.len() <= 10 {
        return "*".repeat(c.len());
    }
    format!(
        "{}…{}",
        c[..4].iter().collect::<String>(),
        c[c.len() - 2..].iter().collect::<String>()
    )
}

/// (kind, masked match) for every secret-looking token on one line.
pub(crate) fn scan_line(line: &str) -> Vec<(String, String)> {
    let line = &line[..line.floor_char_boundary(10_000)];
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (kind, re, g) in PATTERNS.iter() {
        for c in re.captures_iter(line) {
            let Some(m) = c.get(*g) else { continue };
            if seen.insert(m.start()) {
                let shown = if *g == 0 {
                    m.as_str().to_owned()
                } else {
                    mask(m.as_str())
                };
                out.push(((*kind).to_owned(), shown));
            }
        }
    }
    out
}

fn read_lines(p: &Path) -> Vec<(u32, String)> {
    let Ok(m) = std::fs::symlink_metadata(p) else {
        return vec![];
    };
    if !m.is_file() || m.len() > MAX_SCAN_BYTES {
        return vec![];
    }
    let Ok(b) = std::fs::read(p) else {
        return vec![];
    };
    if b.iter().take(8000).any(|c| *c == 0) {
        return vec![];
    }
    lossy(&b)
        .lines()
        .zip(1u32..)
        .map(|(l, n)| (n, l.to_owned()))
        .collect()
}

/// (new-file line number, text) of every added line in a `-U0` diff.
fn added_lines(diff: &str) -> Vec<(u32, String)> {
    let d = parse_unified(diff, usize::MAX);
    let mut out = Vec::new();
    for h in d.hunks {
        let mut n = h.new_start;
        for l in h.lines {
            match l.kind {
                super::LineKind::Added => {
                    out.push((n, l.text));
                    n += 1;
                }
                super::LineKind::Context => n += 1,
                super::LineKind::Removed => {}
            }
        }
    }
    out
}
