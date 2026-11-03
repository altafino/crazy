//! Git checkpoints (PRD M3 step 4; M3-D1, M3-D22, M3-D5, M3-D25): before an agent run in a
//! Git project, the shadow BEFORE snapshot is fetched into the project's own repository as
//! `refs/studio/checkpoints/<run id>`.
//!
//! Nothing but objects and that one ref lands in the user's repo: no `info/exclude`, no
//! `info/attributes` (unlike `SnapshotStore::ensure`, which owns its GIT_DIR), no HEAD, index
//! or working-file change. The runner here is a plain "git on the project" runner with a clean
//! environment, not `GitRunner`, which forces the shadow GIT_DIR and the Studio author.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{GitError, Result};

/// Checkpoint refs kept per project (M3-D4).
pub(crate) const KEEP_CHECKPOINTS: usize = 20;
const NAMESPACE: &str = "refs/studio/checkpoints/";

/// The ref a run's checkpoint lives at.
pub(crate) fn checkpoint_ref(run_id: &str) -> String {
    format!("{NAMESPACE}{run_id}")
}

/// The ref a release build's source snapshot lives at, in the shadow repo and (for Git
/// projects) the project's own repo (M4-D3). Never touched by checkpoint pruning.
pub(crate) fn build_ref(release_id: &str) -> String {
    format!("refs/studio/builds/{release_id}")
}

pub(crate) struct CheckpointStore {
    pub program: PathBuf,
    /// The complete environment (the doctor's `spawn_env()`); everything else is cleared.
    pub env: Vec<(String, String)>,
}

impl CheckpointStore {
    fn run(&self, dir: &Path, args: &[&str]) -> Result<String> {
        let out = Command::new(&self.program)
            // No automatic gc or maintenance in the user's repository.
            .args(["-c", "gc.auto=0", "-c", "maintenance.auto=false"])
            .args(args)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .current_dir(dir)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| GitError::Git(format!("{}: {e}", self.program.display())))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
        } else {
            Err(GitError::Git(
                String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            ))
        }
    }

    /// The project's common Git dir (shared by linked worktrees), or `None` when the folder
    /// is not inside a Git repository.
    pub(crate) fn common_dir(&self, project_dir: &Path) -> Result<Option<PathBuf>> {
        match self.run(project_dir, &["rev-parse", "--git-common-dir"]) {
            Ok(out) => {
                let p = PathBuf::from(out);
                Ok(Some(if p.is_absolute() {
                    p
                } else {
                    project_dir.join(p)
                }))
            }
            Err(GitError::Git(e)) if e.contains("not a git repository") => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Fetches `refs/studio/runs/<run_id>/before` of the `shadow` repo into the project's
    /// repo and prunes old checkpoints. `Ok(None)`: not a Git project, nothing was done.
    pub(crate) fn capture(
        &self,
        project_dir: &Path,
        shadow: &Path,
        run_id: &str,
    ) -> Result<Option<String>> {
        let Some(common) = self.common_dir(project_dir)? else {
            return Ok(None);
        };
        let common_arg = format!("--git-dir={}", common.display());
        let refspec = format!(
            "refs/studio/runs/{run_id}/before:{}",
            checkpoint_ref(run_id)
        );
        self.run(
            project_dir,
            &[
                &common_arg,
                "fetch",
                "--no-tags",
                "--no-recurse-submodules",
                "--no-write-fetch-head",
                &shadow.to_string_lossy(),
                &refspec,
            ],
        )?;
        // The checkpoint exists; a failed prune must not turn it into a warning.
        if let Err(e) = self.prune(&common, KEEP_CHECKPOINTS) {
            eprintln!("checkpoint prune failed: {e}");
        }
        Ok(Some(checkpoint_ref(run_id)))
    }

    /// The project's HEAD commit; `None` outside Git or before the first commit.
    pub(crate) fn head_commit(&self, project_dir: &Path) -> Option<String> {
        self.run(project_dir, &["rev-parse", "--verify", "-q", "HEAD"])
            .ok()
            .filter(|c| !c.is_empty())
    }

    /// True when `git status --porcelain` lists anything (false outside Git).
    pub(crate) fn is_dirty(&self, project_dir: &Path) -> bool {
        self.run(project_dir, &["status", "--porcelain"])
            .is_ok_and(|o| !o.is_empty())
    }

    /// Fetches the shadow repo's build ref for `release_id` into the project's repo under the
    /// same name. `Ok(None)`: not a Git project, nothing was done.
    pub(crate) fn capture_build(
        &self,
        project_dir: &Path,
        shadow: &Path,
        release_id: &str,
    ) -> Result<Option<String>> {
        let Some(common) = self.common_dir(project_dir)? else {
            return Ok(None);
        };
        let name = build_ref(release_id);
        let common_arg = format!("--git-dir={}", common.display());
        self.run(
            project_dir,
            &[
                &common_arg,
                "fetch",
                "--no-tags",
                "--no-recurse-submodules",
                "--no-write-fetch-head",
                &shadow.to_string_lossy(),
                &format!("{name}:{name}"),
            ],
        )?;
        Ok(Some(name))
    }

    /// Deletes a release's build ref from the project's repo; missing is fine.
    pub(crate) fn delete_build_ref(&self, project_dir: &Path, release_id: &str) -> Result<()> {
        let Some(common) = self.common_dir(project_dir)? else {
            return Ok(());
        };
        let common_arg = format!("--git-dir={}", common.display());
        let name = build_ref(release_id);
        // `update-ref -d` on a missing ref succeeds.
        self.run(project_dir, &[&common_arg, "update-ref", "-d", &name])?;
        Ok(())
    }

    /// Keeps the newest `keep` checkpoint refs (by commit time), deletes the rest. Objects
    /// are left for the user's own gc.
    pub(crate) fn prune(&self, common_dir: &Path, keep: usize) -> Result<()> {
        let dir_arg = format!("--git-dir={}", common_dir.display());
        let out = self.run(
            common_dir,
            &[
                &dir_arg,
                "for-each-ref",
                "--sort=-committerdate",
                "--format=%(refname)",
                NAMESPACE,
            ],
        )?;
        for name in out.lines().skip(keep) {
            self.run(common_dir, &[&dir_arg, "update-ref", "-d", name])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use tempfile::TempDir;

    use super::*;
    use crate::services::git::{GitRunner, Phase, SnapshotStore};

    struct Fx {
        tmp: TempDir,
        proj: PathBuf,
        snaps: SnapshotStore,
        cps: CheckpointStore,
    }

    fn env(home: &Path) -> Vec<(String, String)> {
        vec![
            ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
            ("HOME".into(), home.display().to_string()),
            ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
        ]
    }

    fn fx() -> Fx {
        let tmp = TempDir::new().unwrap();
        let proj = tmp.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        let snaps = SnapshotStore::new(
            &tmp.path().join("data"),
            GitRunner {
                program: "git".into(),
                env: env(tmp.path()),
            },
        );
        let cps = CheckpointStore {
            program: "git".into(),
            env: env(tmp.path()),
        };
        Fx {
            tmp,
            proj,
            snaps,
            cps,
        }
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    impl Fx {
        fn init_repo(&self) {
            git(&self.proj, &["init", "-q", "-b", "main"]);
            fs::write(self.proj.join("a.txt"), "one\n").unwrap();
            git(&self.proj, &["add", "."]);
            git(&self.proj, &["commit", "-qm", "first"]);
            // Dirty edits that the checkpoint must capture without committing them.
            fs::write(self.proj.join("a.txt"), "two\n").unwrap();
            fs::write(self.proj.join("new.txt"), "new\n").unwrap();
        }

        fn before(&self, run: &str) -> String {
            self.snaps
                .snapshot("p1", &self.proj, run, Phase::Before, "before")
                .unwrap()
                .commit
        }

        fn capture(&self, run: &str) -> Result<Option<String>> {
            self.cps
                .capture(&self.proj, &self.snaps.shadow("p1").unwrap(), run)
        }
    }

    fn file_bytes(p: &Path) -> Option<Vec<u8>> {
        fs::read(p).ok()
    }

    #[test]
    fn checkpoint_tree_equals_the_before_tree_and_the_repo_is_untouched() {
        let f = fx();
        f.init_repo();
        let info = f.proj.join(".git/info");
        fs::create_dir_all(&info).unwrap();
        fs::write(info.join("exclude"), "# mine\n*.log\n").unwrap();
        fs::write(info.join("attributes"), "*.bin binary\n").unwrap();
        // `git status` refreshes the index's stat cache, so run it before reading the bytes.
        let (status, branch) = (
            git(&f.proj, &["status", "--porcelain=v1"]),
            git(&f.proj, &["branch", "--show-current"]),
        );
        let (head, index) = (
            git(&f.proj, &["rev-parse", "HEAD"]),
            fs::read(f.proj.join(".git/index")).unwrap(),
        );

        let before = f.before("run1");
        let r = f.capture("run1").unwrap().unwrap();
        assert_eq!(r, "refs/studio/checkpoints/run1");

        // Same commit, hence same tree, as the shadow BEFORE snapshot.
        assert_eq!(git(&f.proj, &["rev-parse", &r]), before);
        let shadow = f.snaps.shadow("p1").unwrap();
        let shadow_tree = git(&shadow, &["rev-parse", &format!("{before}^{{tree}}")]);
        assert_eq!(
            git(&f.proj, &["rev-parse", &format!("{r}^{{tree}}")]),
            shadow_tree
        );
        // The tree holds the dirty working state.
        assert_eq!(git(&f.proj, &["show", &format!("{r}:a.txt")]), "two");
        assert_eq!(git(&f.proj, &["show", &format!("{r}:new.txt")]), "new");

        // Hands off the user's repo.
        assert_eq!(
            file_bytes(&info.join("exclude")).unwrap(),
            b"# mine\n*.log\n"
        );
        assert_eq!(
            file_bytes(&info.join("attributes")).unwrap(),
            b"*.bin binary\n"
        );
        assert_eq!(git(&f.proj, &["rev-parse", "HEAD"]), head);
        assert_eq!(fs::read(f.proj.join(".git/index")).unwrap(), index);
        assert_eq!(git(&f.proj, &["status", "--porcelain=v1"]), status);
        assert_eq!(git(&f.proj, &["branch", "--show-current"]), branch);
        assert_eq!(fs::read_to_string(f.proj.join("a.txt")).unwrap(), "two\n");
        assert!(!f.proj.join(".git/FETCH_HEAD").exists());
        assert_eq!(git(&f.proj, &["branch", "--list"]), "* main");
    }

    #[test]
    fn non_git_project_gets_no_checkpoint() {
        let f = fx();
        fs::write(f.proj.join("a.txt"), "x").unwrap();
        f.before("run1");
        assert_eq!(f.capture("run1").unwrap(), None);
        assert!(!f.proj.join(".git").exists());
    }

    #[test]
    fn unborn_repo_and_subfolder_projects_work() {
        let f = fx();
        git(&f.proj, &["init", "-q"]);
        let sub = f.proj.join("apps/one");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("x.txt"), "x").unwrap();
        f.snaps
            .snapshot("p1", &sub, "r1", Phase::Before, "b")
            .unwrap();
        let r = f
            .cps
            .capture(&sub, &f.snaps.shadow("p1").unwrap(), "r1")
            .unwrap();
        assert_eq!(r.as_deref(), Some("refs/studio/checkpoints/r1"));
        git(
            &f.proj,
            &["rev-parse", "--verify", "refs/studio/checkpoints/r1"],
        );
    }

    #[test]
    fn read_only_git_dir_is_an_error_not_a_panic() {
        if unsafe { libc::geteuid() } == 0 {
            return; // root ignores directory permissions
        }
        let f = fx();
        f.init_repo();
        f.before("run1");
        let lock = |mode: u32| {
            for dir in [".git", ".git/objects", ".git/refs"] {
                let p = f.proj.join(dir);
                fs::set_permissions(&p, fs::Permissions::from_mode(mode)).unwrap();
            }
        };
        lock(0o555);
        let r = f.capture("run1");
        lock(0o755);
        let err = r.unwrap_err().to_string();
        assert!(!err.is_empty());
        assert!(
            git(&f.proj, &["for-each-ref", "refs/studio/"]).is_empty(),
            "no ref was created"
        );
        let _ = f.tmp.path();
    }

    #[test]
    fn keeps_the_newest_twenty_checkpoints() {
        let f = fx();
        f.init_repo();
        let tree = git(&f.proj, &["rev-parse", "HEAD^{tree}"]);
        for i in 0..25 {
            // Distinct commit times, oldest first.
            let date = format!("{} +0000", 1_700_000_000 + i * 100);
            let out = Command::new("git")
                .args(["commit-tree", &tree, "-m", "cp"])
                .current_dir(&f.proj)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .env("GIT_COMMITTER_DATE", &date)
                .env("GIT_AUTHOR_DATE", &date)
                .output()
                .unwrap();
            let commit = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            git(
                &f.proj,
                &[
                    "update-ref",
                    &format!("refs/studio/checkpoints/old{i:02}"),
                    &commit,
                ],
            );
        }
        // A real capture adds a 26th (newest) and prunes down to 20.
        f.before("fresh");
        f.capture("fresh").unwrap();
        let refs = git(
            &f.proj,
            &[
                "for-each-ref",
                "--format=%(refname:short)",
                "refs/studio/checkpoints/",
            ],
        );
        let names: Vec<_> = refs.lines().collect();
        assert_eq!(names.len(), KEEP_CHECKPOINTS);
        assert!(names.contains(&"studio/checkpoints/fresh"));
        assert!(names.contains(&"studio/checkpoints/old24"));
        assert!(!names.contains(&"studio/checkpoints/old00"));
        assert!(!names.contains(&"studio/checkpoints/old05"));
        assert!(names.contains(&"studio/checkpoints/old06"));
        // Branches and tags are never touched by pruning.
        assert_eq!(git(&f.proj, &["branch", "--list"]), "* main");
    }
}
