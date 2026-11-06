//! Per-project shadow git repo for before/after run snapshots (PRD M2 step 4,
//! D28, D35, D37, D38, D39). The user's own `.git`, index, branch and files are
//! never touched: every git call runs with `GIT_DIR` = the shadow repo,
//! `GIT_WORK_TREE` = the project dir and a throw-away `GIT_INDEX_FILE`.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use specta::Type;

pub(crate) mod checkpoint;
pub(crate) mod net;
pub(crate) mod repo;
#[cfg(test)]
mod repo_tests;
#[cfg(test)]
mod tests;

/// Newest runs whose snapshots are kept per project (D39).
pub(crate) const KEEP_RUNS: usize = 20;
const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;
const MAX_DIFF_LINES: usize = 5000;
/// Hard cap for "Show full diff", so a huge file can't freeze the UI.
const MAX_FULL_DIFF_LINES: usize = 100_000;
const IGNORED_DIRS: [&str; 5] = ["node_modules", "target", "dist", ".git", ".venv"];
const SKIPPED_MARKER: &str = "studio-skipped: ";

#[derive(Debug, thiserror::Error)]
pub(crate) enum GitError {
    #[error("git: {0}")]
    Git(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid: {0}")]
    Invalid(String),
}
pub(crate) type Result<T> = std::result::Result<T, GitError>;

// ---- UI types -------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SkipReason {
    IgnoredDir,
    Secret,
    TooLarge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkippedPath {
    pub path: String,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SnapshotRef {
    pub commit: String,
    pub skipped: Vec<SkippedPath>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Binary,
    NotCaptured,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub additions: u32,
    pub deletions: u32,
    pub skipped_reason: Option<SkipReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Line {
    pub kind: LineKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileDiff {
    pub hunks: Vec<Hunk>,
    pub binary: bool,
    pub truncated: bool,
    /// Blob sizes in bytes, filled for binary files; None when that side is absent.
    pub old_size: Option<u32>,
    pub new_size: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RevertSkipReason {
    EditedSince,
    NotCaptured,
    NotSelected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RevertSkip {
    pub path: String,
    pub reason: RevertSkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RevertReport {
    pub restored: Vec<String>,
    pub skipped: Vec<RevertSkip>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Before,
    After,
}

// ---- git runner -----------------------------------------------------------

/// Like projects' `GitCmd`: explicit program, `env_clear` plus a complete env.
pub(crate) struct GitRunner {
    pub program: PathBuf,
    pub env: Vec<(String, String)>,
}

#[derive(Default)]
struct Ctx {
    shadow: PathBuf,
    work: Option<PathBuf>,
    index: Option<PathBuf>,
}

impl GitRunner {
    fn run(&self, ctx: &Ctx, args: &[&str], stdin: Option<&[u8]>) -> Result<Vec<u8>> {
        let mut cmd = Command::new(&self.program);
        cmd.args(["-c", "core.autocrlf=false"])
            .args(args)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .env("GIT_DIR", &ctx.shadow)
            .env("GIT_LITERAL_PATHSPECS", "1")
            .env("GIT_AUTHOR_NAME", "AI App Studio")
            .env("GIT_AUTHOR_EMAIL", "studio@localhost")
            .env("GIT_COMMITTER_NAME", "AI App Studio")
            .env("GIT_COMMITTER_EMAIL", "studio@localhost")
            .current_dir(ctx.work.as_deref().unwrap_or(&ctx.shadow))
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(w) = &ctx.work {
            cmd.env("GIT_WORK_TREE", w);
        }
        if let Some(i) = &ctx.index {
            cmd.env("GIT_INDEX_FILE", i);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| GitError::Git(format!("{}: {e}", self.program.display())))?;
        if let (Some(data), Some(mut si)) = (stdin, child.stdin.take()) {
            // Inputs are path lists / tiny blobs and git prints little while reading.
            si.write_all(data)?;
        }
        let out = child.wait_with_output()?;
        if out.status.success() {
            Ok(out.stdout)
        } else {
            Err(GitError::Git(
                String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            ))
        }
    }

    fn text(&self, ctx: &Ctx, args: &[&str]) -> Result<String> {
        Ok(lossy(&self.run(ctx, args, None)?).trim().to_owned())
    }
}

// ---- helpers --------------------------------------------------------------

struct TempFile(PathBuf);
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn check_id(kind: &str, id: &str) -> Result<()> {
    let ok = !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(GitError::Invalid(format!("bad {kind} id")))
    }
}

fn check_commit(c: &str) -> Result<()> {
    if !c.is_empty() && c.len() <= 64 && c.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(GitError::Invalid("bad commit id".into()))
    }
}

pub(crate) fn is_secret(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    (name == ".env" || (name.starts_with(".env.") && name != ".env.example"))
        || [".pem", ".key", ".p12"].iter().any(|s| name.ends_with(s))
        || ["id_rsa", "id_ed25519", "id_ecdsa"]
            .iter()
            .any(|p| name.starts_with(p))
        || name == ".npmrc"
        || name == ".netrc"
        || name.contains("credentials")
}

/// Why `path` (or a folder containing it) was not captured, if it was not.
fn is_skipped(path: &str, skipped: &[SkippedPath]) -> Option<SkipReason> {
    skipped
        .iter()
        .find(|s| s.path == path || (s.path.ends_with('/') && path.starts_with(&s.path)))
        .map(|s| s.reason)
}

fn nul_split(b: &[u8]) -> impl Iterator<Item = &[u8]> {
    b.split(|c| *c == 0).filter(|s| !s.is_empty())
}

/// Like `nul_split` but keeps empty fields (numstat rename records).
fn nul_split_keep_empty(b: &[u8]) -> impl Iterator<Item = &[u8]> {
    b.strip_suffix(&[0]).unwrap_or(b).split(|c| *c == 0)
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn tmp_sibling(target: &Path) -> PathBuf {
    target.with_file_name(format!(".studio-restore-{}", uuid::Uuid::new_v4()))
}

fn write_atomic(target: &Path, data: &[u8], git_mode: &str) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = tmp_sibling(target);
    #[cfg(unix)]
    if git_mode == "120000" {
        std::os::unix::fs::symlink(OsStr::new(&lossy(data)), &tmp)?;
        std::fs::rename(&tmp, target)?;
        return Ok(());
    }
    let res = (|| -> Result<()> {
        std::fs::write(&tmp, data)?;
        // Keep the current file's mode; for a deleted file use the recorded one.
        match std::fs::metadata(target) {
            Ok(m) => std::fs::set_permissions(&tmp, m.permissions())?,
            #[cfg(unix)]
            Err(_) => {
                use std::os::unix::fs::PermissionsExt;
                let mode = if git_mode == "100755" { 0o755 } else { 0o644 };
                std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))?;
            }
            #[cfg(not(unix))]
            Err(_) => {}
        }
        std::fs::rename(&tmp, target)?;
        Ok(())
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

fn parse_unified(text: &str, max_lines: usize) -> FileDiff {
    let mut d = FileDiff {
        hunks: Vec::new(),
        binary: false,
        truncated: false,
        old_size: None,
        new_size: None,
    };
    let mut total = 0usize;
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("@@ ") {
            if let Some(hunk) = parse_header(h) {
                d.hunks.push(hunk);
            }
            continue;
        }
        let Some(hunk) = d.hunks.last_mut() else {
            if line.starts_with("Binary files ") || line == "GIT binary patch" {
                d.binary = true;
            }
            continue;
        };
        let (kind, text) = match line.as_bytes().first() {
            Some(b'+') => (LineKind::Added, &line[1..]),
            Some(b'-') => (LineKind::Removed, &line[1..]),
            Some(b' ') => (LineKind::Context, &line[1..]),
            _ => continue, // "\ No newline at end of file"
        };
        if total >= max_lines {
            d.truncated = true;
            break;
        }
        total += 1;
        hunk.lines.push(Line {
            kind,
            text: text.to_owned(),
        });
    }
    d
}

/// "-1,3 +1,4 @@ ..." -> Hunk (a count is 1 when omitted).
fn parse_header(h: &str) -> Option<Hunk> {
    let mut parts = h.split(' ');
    let range = |s: &str, sign: char| -> Option<(u32, u32)> {
        let s = s.strip_prefix(sign)?;
        match s.split_once(',') {
            Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
            None => Some((s.parse().ok()?, 1)),
        }
    };
    let (old_start, old_lines) = range(parts.next()?, '-')?;
    let (new_start, new_lines) = range(parts.next()?, '+')?;
    Some(Hunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        lines: Vec::new(),
    })
}

// ---- store ----------------------------------------------------------------

pub(crate) struct SnapshotStore {
    root: PathBuf,
    git: GitRunner,
}

impl SnapshotStore {
    pub(crate) fn new(studio_data_dir: &Path, git: GitRunner) -> Self {
        Self {
            root: studio_data_dir.join("snapshots"),
            git,
        }
    }

    pub(crate) fn shadow(&self, project_id: &str) -> Result<PathBuf> {
        check_id("project", project_id)?;
        Ok(self.root.join(format!("{project_id}.git")))
    }

    fn ctx(&self, project_id: &str) -> Result<Ctx> {
        Ok(Ctx {
            shadow: self.shadow(project_id)?,
            ..Ctx::default()
        })
    }

    fn ensure(&self, project_id: &str) -> Result<Ctx> {
        let ctx = self.ctx(project_id)?;
        if !ctx.shadow.join("HEAD").exists() {
            std::fs::create_dir_all(&self.root)?;
            let boot = Ctx {
                shadow: self.root.clone(),
                ..Ctx::default()
            };
            let shadow = ctx.shadow.to_string_lossy();
            self.git
                .run(&boot, &["init", "--bare", "-q", &shadow], None)?;
        }
        let info = ctx.shadow.join("info");
        std::fs::create_dir_all(&info)?;
        let ex: String = IGNORED_DIRS.iter().map(|d| format!("{d}/\n")).collect();
        std::fs::write(info.join("exclude"), ex)?;
        // Highest-precedence attributes: never convert, blobs are the raw bytes.
        std::fs::write(info.join("attributes"), "* -text -filter -ident\n")?;
        Ok(ctx)
    }

    /// Snapshots `project_dir` and points `refs/studio/runs/<run_id>/<phase>` at it.
    pub(crate) fn snapshot(
        &self,
        project_id: &str,
        project_dir: &Path,
        run_id: &str,
        phase: Phase,
        label: &str,
    ) -> Result<SnapshotRef> {
        check_id("run", run_id)?;
        let p = match phase {
            Phase::Before => "before",
            Phase::After => "after",
        };
        self.snapshot_inner(
            project_id,
            project_dir,
            label,
            Some(format!("refs/studio/runs/{run_id}/{p}")),
        )
    }

    /// Snapshots `project_dir` for a release build (M4-D3) under `refs/studio/builds/<id>`,
    /// outside the 20-run pruning. Returns the snapshot and the ref name.
    pub(crate) fn snapshot_build(
        &self,
        project_id: &str,
        project_dir: &Path,
        release_id: &str,
        label: &str,
    ) -> Result<(SnapshotRef, String)> {
        check_id("release", release_id)?;
        let name = checkpoint::build_ref(release_id);
        let snap = self.snapshot_inner(project_id, project_dir, label, Some(name.clone()))?;
        Ok((snap, name))
    }

    /// Deletes a release's build ref from the shadow repo; a missing one is fine.
    pub(crate) fn delete_build_ref(&self, project_id: &str, release_id: &str) -> Result<()> {
        check_id("release", release_id)?;
        let ctx = self.ctx(project_id)?;
        if !ctx.shadow.exists() {
            return Ok(());
        }
        self.git.run(
            &ctx,
            &["update-ref", "-d", &checkpoint::build_ref(release_id)],
            None,
        )?;
        Ok(())
    }

    /// Extracts the saved source of a release (`refs/studio/builds/<id>`, M4-D3) into `dest`,
    /// an existing empty folder (F22, D7): `git archive` into a tar next to `dest`, then
    /// `tar -x`. The user's project folder is never read.
    pub(crate) fn archive_build(
        &self,
        project_id: &str,
        snapshot_ref: &str,
        dest: &Path,
    ) -> Result<()> {
        if !snapshot_ref.starts_with("refs/studio/builds/") || snapshot_ref.contains("..") {
            return Err(GitError::Invalid(format!(
                "not a build ref: {snapshot_ref}"
            )));
        }
        let ctx = self.ctx(project_id)?;
        if !ctx.shadow.join("HEAD").exists() {
            return Err(GitError::Invalid("the saved source is gone".into()));
        }
        let mut tar_path = dest.as_os_str().to_owned();
        tar_path.push(".tar");
        let tar_path = PathBuf::from(tar_path);
        let result = (|| {
            self.git.run(
                &ctx,
                &[
                    "archive",
                    "--format=tar",
                    "-o",
                    &tar_path.to_string_lossy(),
                    snapshot_ref,
                ],
                None,
            )?;
            let out = Command::new("tar")
                .arg("-xf")
                .arg(&tar_path)
                .arg("-C")
                .arg(dest)
                .env_clear()
                .envs(self.git.env.iter().map(|(k, v)| (k, v)))
                .stdin(Stdio::null())
                .output()?;
            if out.status.success() {
                Ok(())
            } else {
                Err(GitError::Git(format!(
                    "tar: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                )))
            }
        })();
        let _ = std::fs::remove_file(&tar_path);
        result
    }

    fn snapshot_inner(
        &self,
        project_id: &str,
        project_dir: &Path,
        label: &str,
        refname: Option<String>,
    ) -> Result<SnapshotRef> {
        let mut ctx = self.ensure(project_id)?;
        let index = ctx.shadow.join(format!("index.{}", uuid::Uuid::new_v4()));
        let _guard = TempFile(index.clone());
        ctx.work = Some(project_dir.to_owned());
        ctx.index = Some(index);
        let g = &self.git;

        // List first, filter, then stage only the keepers, so secrets and huge
        // files are never read into the shadow store. Equivalent to `add -A`
        // on a fresh index.
        let listing = g.run(
            &ctx,
            &["ls-files", "--others", "--exclude-standard", "-z"],
            None,
        )?;
        let mut skipped = Vec::new();
        let mut keep = Vec::new();
        for raw in nul_split(&listing) {
            let path = lossy(raw);
            if path.ends_with('/') {
                continue; // nested repository
            }
            let reason = if is_secret(&path) {
                Some(SkipReason::Secret)
            } else if std::fs::symlink_metadata(project_dir.join(&path))
                .is_ok_and(|m| m.is_file() && m.len() > MAX_FILE_BYTES)
            {
                Some(SkipReason::TooLarge)
            } else {
                None
            };
            if let Some(reason) = reason {
                skipped.push(SkippedPath { path, reason });
            } else {
                keep.extend_from_slice(raw);
                keep.push(0);
            }
        }
        g.run(
            &ctx,
            &["update-index", "--add", "-z", "--stdin"],
            Some(&keep),
        )?;

        let ignored = g.run(
            &ctx,
            &[
                "ls-files",
                "--others",
                "--ignored",
                "--exclude-standard",
                "--directory",
                "-z",
            ],
            None,
        )?;
        for raw in nul_split(&ignored) {
            let path = lossy(raw);
            if let Some(dir) = path.strip_suffix('/')
                && IGNORED_DIRS.contains(&dir.rsplit('/').next().unwrap_or(dir))
            {
                skipped.push(SkippedPath {
                    path,
                    reason: SkipReason::IgnoredDir,
                });
            }
        }

        let tree = g.text(&ctx, &["write-tree"])?;
        let json = serde_json::to_string(&skipped).map_err(|e| GitError::Git(e.to_string()))?;
        let label = label.replace(['\n', '\r'], " ");
        let msg = format!("{label}\n\n{SKIPPED_MARKER}{json}\n");
        let commit = g.text(&ctx, &["commit-tree", &tree, "-m", &msg])?;
        if let Some(r) = refname {
            g.run(&ctx, &["update-ref", &r, &commit], None)?;
        }
        Ok(SnapshotRef { commit, skipped })
    }

    /// Skipped list recorded in a snapshot commit's message.
    pub(crate) fn skipped(&self, project_id: &str, commit: &str) -> Result<Vec<SkippedPath>> {
        check_commit(commit)?;
        let ctx = self.ctx(project_id)?;
        let body = self.git.text(&ctx, &["cat-file", "commit", commit])?;
        let Some(json) = body
            .lines()
            .rev()
            .find_map(|l| l.strip_prefix(SKIPPED_MARKER))
        else {
            return Ok(Vec::new());
        };
        serde_json::from_str(json).map_err(|e| GitError::Git(e.to_string()))
    }

    pub(crate) fn diff(
        &self,
        project_id: &str,
        before: &str,
        after: &str,
    ) -> Result<Vec<FileChange>> {
        check_commit(before)?;
        check_commit(after)?;
        let ctx = self.ctx(project_id)?;
        let g = &self.git;
        let ns = g.run(
            &ctx,
            &["diff", "--name-status", "-z", "-M", before, after],
            None,
        )?;
        let num = g.run(
            &ctx,
            &["diff", "--numstat", "-z", "-M", before, after],
            None,
        )?;

        // numstat -z: "<a>\t<d>\t<path>\0", or for renames "<a>\t<d>\t\0<old>\0<new>\0".
        let mut counts: HashMap<String, Option<(u32, u32)>> = HashMap::new();
        let mut it = nul_split_keep_empty(&num);
        while let Some(rec) = it.next() {
            let rec = lossy(rec);
            let mut f = rec.splitn(3, '\t');
            let (Some(a), Some(d), Some(p)) = (f.next(), f.next(), f.next()) else {
                continue;
            };
            let path = if p.is_empty() {
                it.next();
                it.next().map(lossy).unwrap_or_default()
            } else {
                p.to_owned()
            };
            counts.insert(path, a.parse().ok().zip(d.parse().ok()));
        }

        let mut out = Vec::new();
        let mut it = nul_split(&ns);
        while let Some(st) = it.next() {
            let st = lossy(st);
            let Some(first) = it.next().map(lossy) else {
                break;
            };
            let (status, old_path, path) = match st.chars().next() {
                Some('R' | 'C') => {
                    let new = it.next().map(lossy).unwrap_or_default();
                    (FileStatus::Renamed, Some(first), new)
                }
                Some('A') => (FileStatus::Added, None, first),
                Some('D') => (FileStatus::Deleted, None, first),
                _ => (FileStatus::Modified, None, first),
            };
            let (status, additions, deletions) = match counts.get(&path) {
                Some(Some((a, d))) => (status, *a, *d),
                Some(None) => (FileStatus::Binary, 0, 0),
                None => (status, 0, 0),
            };
            out.push(FileChange {
                path,
                old_path,
                status,
                additions,
                deletions,
                skipped_reason: None,
            });
        }

        // ponytail: the whole union of both snapshots' skipped paths is reported,
        // not only those that look changed; narrow it if the UI gets noisy.
        let mut seen = HashSet::new();
        let all = self
            .skipped(project_id, before)?
            .into_iter()
            .chain(self.skipped(project_id, after)?);
        for s in all {
            if seen.insert(s.path.clone()) {
                out.push(FileChange {
                    path: s.path,
                    old_path: None,
                    status: FileStatus::NotCaptured,
                    additions: 0,
                    deletions: 0,
                    skipped_reason: Some(s.reason),
                });
            }
        }
        Ok(out)
    }

    pub(crate) fn file_diff(
        &self,
        project_id: &str,
        before: &str,
        after: &str,
        path: &str,
        full: bool,
    ) -> Result<FileDiff> {
        check_commit(before)?;
        check_commit(after)?;
        let ctx = self.ctx(project_id)?;
        let raw = self.git.run(
            &ctx,
            &[
                "diff",
                "--no-color",
                "--no-ext-diff",
                "-U3",
                before,
                after,
                "--",
                path,
            ],
            None,
        )?;
        let mut d = parse_unified(
            &lossy(&raw),
            if full {
                MAX_FULL_DIFF_LINES
            } else {
                MAX_DIFF_LINES
            },
        );
        if d.binary {
            let size = |commit: &str| -> Result<Option<u32>> {
                let Some((_, oid)) = self.tree_entry(&ctx, commit, path)? else {
                    return Ok(None);
                };
                Ok(self.git.text(&ctx, &["cat-file", "-s", &oid])?.parse().ok())
            };
            d.old_size = size(before)?;
            d.new_size = size(after)?;
        }
        Ok(d)
    }

    /// Commits that any run ref still points at; one call per project. A session whose
    /// BEFORE commit is not in here was pruned (or its snapshots deleted).
    pub(crate) fn live_commits(&self, project_id: &str) -> Result<HashSet<String>> {
        let ctx = self.ctx(project_id)?;
        if !ctx.shadow.exists() {
            return Ok(HashSet::new());
        }
        let out = self.git.text(
            &ctx,
            &[
                "for-each-ref",
                "--format=%(objectname)",
                "refs/studio/runs/",
            ],
        )?;
        Ok(out.lines().map(str::to_owned).collect())
    }

    /// Restore the run's changes (BEFORE -> AFTER) for `selected` paths.
    pub(crate) fn revert(
        &self,
        project_id: &str,
        project_dir: &Path,
        before: &str,
        after: &str,
        selected: &[String],
    ) -> Result<RevertReport> {
        self.revert_between(project_id, project_dir, before, after, selected)
    }

    /// Interrupted run: no AFTER, so a fresh snapshot of the work tree stands in.
    pub(crate) fn revert_interrupted(
        &self,
        project_id: &str,
        project_dir: &Path,
        before: &str,
        selected: &[String],
    ) -> Result<RevertReport> {
        check_commit(before)?;
        let now = self.snapshot_inner(project_id, project_dir, "interrupted revert", None)?;
        self.revert_between(project_id, project_dir, before, &now.commit, selected)
    }

    fn revert_between(
        &self,
        project_id: &str,
        project_dir: &Path,
        before: &str,
        after: &str,
        selected: &[String],
    ) -> Result<RevertReport> {
        let changes = self.diff(project_id, before, after)?;
        let mut not_captured = self.skipped(project_id, before)?;
        not_captured.extend(self.skipped(project_id, after)?);
        let ctx = Ctx {
            work: Some(project_dir.to_owned()),
            ..self.ctx(project_id)?
        };
        let sel: HashSet<&str> = selected.iter().map(String::as_str).collect();
        let mut report = RevertReport {
            restored: Vec::new(),
            skipped: Vec::new(),
        };
        let skip = |r: &mut RevertReport, path: String, reason| {
            r.skipped.push(RevertSkip { path, reason });
        };
        for ch in changes {
            if ch.status == FileStatus::NotCaptured {
                skip(&mut report, ch.path, RevertSkipReason::NotCaptured);
                continue;
            }
            let mut paths = vec![ch.path.clone()];
            paths.extend(ch.old_path.clone()); // a rename touches both names
            if !paths.iter().any(|p| sel.contains(p.as_str())) {
                skip(&mut report, ch.path, RevertSkipReason::NotSelected);
                continue;
            }
            for p in paths {
                if is_skipped(&p, &not_captured).is_some() {
                    skip(&mut report, p, RevertSkipReason::NotCaptured);
                } else if self.restore_one(&ctx, project_dir, before, after, &p)? {
                    report.restored.push(p);
                } else {
                    skip(&mut report, p, RevertSkipReason::EditedSince);
                }
            }
        }
        Ok(report)
    }

    /// Blob hash of the file now on disk (`None` if absent).
    fn current_hash(&self, ctx: &Ctx, target: &Path) -> Result<Option<Option<String>>> {
        let h = match std::fs::symlink_metadata(target) {
            Err(_) => None,
            Ok(m) if m.file_type().is_symlink() => {
                let t = std::fs::read_link(target)?.to_string_lossy().into_owned();
                let out = self
                    .git
                    .run(ctx, &["hash-object", "--stdin"], Some(t.as_bytes()))?;
                Some(lossy(&out).trim().to_owned())
            }
            Ok(m) if m.is_file() => Some(self.git.text(
                ctx,
                &[
                    "hash-object",
                    "--no-filters",
                    "--",
                    &target.to_string_lossy(),
                ],
            )?),
            Ok(_) => return Ok(None), // a directory is in the way
        };
        Ok(Some(h))
    }

    /// Ok(false) when the file no longer matches its AFTER content.
    fn restore_one(
        &self,
        ctx: &Ctx,
        dir: &Path,
        before: &str,
        after: &str,
        rel: &str,
    ) -> Result<bool> {
        let target = dir.join(rel);
        let Some(cur) = self.current_hash(ctx, &target)? else {
            return Ok(false);
        };
        if cur != self.tree_entry(ctx, after, rel)?.map(|e| e.1) {
            return Ok(false);
        }
        match self.tree_entry(ctx, before, rel)? {
            None => {
                if cur.is_some() {
                    std::fs::remove_file(&target)?;
                }
            }
            Some((mode, oid)) => {
                let blob = self.git.run(ctx, &["cat-file", "blob", &oid], None)?;
                write_atomic(&target, &blob, &mode)?;
            }
        }
        Ok(true)
    }

    /// (mode, oid) of `path` in a commit's tree.
    fn tree_entry(&self, ctx: &Ctx, commit: &str, path: &str) -> Result<Option<(String, String)>> {
        let out = self
            .git
            .run(ctx, &["ls-tree", "-z", commit, "--", path], None)?;
        Ok(nul_split(&out).next().and_then(|rec| {
            let rec = lossy(rec);
            let mut f = rec.split('\t').next()?.split(' ');
            let mode = f.next()?.to_owned();
            f.next()?;
            Some((mode, f.next()?.to_owned()))
        }))
    }

    /// Keeps the newest `keep_runs` runs (by commit time), drops older refs, gc.
    pub(crate) fn prune(&self, project_id: &str, keep_runs: usize) -> Result<()> {
        let ctx = self.ctx(project_id)?;
        if !ctx.shadow.exists() {
            return Ok(());
        }
        let out = self.git.text(
            &ctx,
            &[
                "for-each-ref",
                "--format=%(committerdate:unix) %(refname)",
                "refs/studio/runs/",
            ],
        )?;
        let mut runs: BTreeMap<String, (i64, Vec<String>)> = BTreeMap::new();
        for line in out.lines() {
            let Some((t, name)) = line.split_once(' ') else {
                continue;
            };
            let Some(run) = name
                .strip_prefix("refs/studio/runs/")
                .and_then(|r| r.split('/').next())
            else {
                continue;
            };
            let e = runs.entry(run.to_owned()).or_default();
            e.0 = e.0.max(t.parse().unwrap_or(0));
            e.1.push(name.to_owned());
        }
        let mut order: Vec<_> = runs.into_iter().collect();
        order.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| b.0.cmp(&a.0)));
        for (_, (_, refs)) in order.into_iter().skip(keep_runs) {
            for r in refs {
                self.git.run(&ctx, &["update-ref", "-d", &r], None)?;
            }
        }
        self.git
            .run(&ctx, &["reflog", "expire", "--expire=now", "--all"], None)?;
        self.git
            .run(&ctx, &["gc", "--prune=now", "--quiet"], None)?;
        Ok(())
    }

    pub(crate) fn delete_all(&self, project_id: &str) -> Result<()> {
        let shadow = self.shadow(project_id)?;
        if shadow.exists() {
            std::fs::remove_dir_all(shadow)?;
        }
        Ok(())
    }
}
