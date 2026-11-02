//! Process-group spawning shared by the doctor and the agent runner (D31).
//!
//! Children get a clean environment, their own process group (so the whole tree can be
//! signalled), `kill_on_drop`, and on Linux a parent-death SIGTERM.
use std::{
    ffi::OsStr,
    io,
    path::Path,
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::process::{Child, Command};

pub struct GroupChild {
    pub child: Child,
    /// Process group id; equals the child's pid because of `process_group(0)`.
    pub pgid: i32,
}

/// Spawns `program` with exactly `env` (cleared first), stdout/stderr piped.
pub async fn spawn_group<S: AsRef<OsStr>>(
    program: impl AsRef<OsStr>,
    args: &[S],
    env: &[(String, String)],
    cwd: Option<&Path>,
    stdin: Stdio,
) -> io::Result<GroupChild> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    #[cfg(target_os = "linux")]
    // SAFETY: prctl is async-signal-safe and only touches the forked child.
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    let child = spawn_retrying(&mut cmd).await?;
    let pgid = child
        .id()
        .and_then(|p| i32::try_from(p).ok())
        .ok_or_else(|| io::Error::other("child exited before its pid was read"))?;
    Ok(GroupChild { child, pgid })
}

/// Freshly written scripts can briefly fail with ETXTBSY when another thread forks.
async fn spawn_retrying(cmd: &mut Command) -> io::Result<Child> {
    for _ in 0..5 {
        match cmd.spawn() {
            Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            r => return r,
        }
    }
    cmd.spawn()
}

impl GroupChild {
    fn signal(&self, sig: i32) {
        // SAFETY: plain syscall on our own child's group. Errors (ESRCH) are fine.
        unsafe { libc::killpg(self.pgid, sig) };
    }

    /// SIGKILL to the whole group, then reap the leader.
    pub async fn kill_group(&mut self) -> io::Result<ExitStatus> {
        self.signal(libc::SIGKILL);
        self.child.wait().await
    }

    // used by M2 lane A / job supervisor
    #[allow(dead_code)]
    /// SIGTERM, wait up to `grace` for the leader, then SIGKILL the group and reap.
    pub async fn terminate_group(&mut self, grace: Duration) -> io::Result<ExitStatus> {
        self.signal(libc::SIGTERM);
        match tokio::time::timeout(grace, self.child.wait()).await {
            Ok(status) => {
                // Leader is gone; sweep any stragglers left in the group.
                self.signal(libc::SIGKILL);
                status
            }
            Err(_) => self.kill_group().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, BufReader};

    fn env() -> Vec<(String, String)> {
        vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())]
    }

    fn alive(pid: i32) -> bool {
        // SAFETY: signal 0 only checks existence.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[tokio::test]
    async fn streams_lines_before_exit() {
        let mut g = spawn_group(
            "sh",
            &["-c", "for i in 1 2 3; do echo line$i; sleep 0.3; done"],
            &env(),
            None,
            Stdio::null(),
        )
        .await
        .unwrap();
        let mut lines = BufReader::new(g.child.stdout.take().unwrap()).lines();
        assert_eq!(lines.next_line().await.unwrap().unwrap(), "line1");
        assert!(g.child.try_wait().unwrap().is_none(), "still running");
        while lines.next_line().await.unwrap().is_some() {}
        assert!(g.child.wait().await.unwrap().success());
    }

    async fn gone(pid: i32) -> bool {
        for _ in 0..50 {
            if !alive(pid) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    #[tokio::test]
    async fn kill_group_takes_the_grandchild_too() {
        let mut g = spawn_group(
            "sh",
            &["-c", "sleep 30 & echo $!; wait"],
            &env(),
            None,
            Stdio::null(),
        )
        .await
        .unwrap();
        let mut lines = BufReader::new(g.child.stdout.take().unwrap()).lines();
        let gc: i32 = lines.next_line().await.unwrap().unwrap().parse().unwrap();
        assert!(alive(gc));
        g.kill_group().await.unwrap();
        // The orphaned grandchild is reparented and reaped by init; poll briefly.
        assert!(gone(gc).await, "grandchild survived");
    }

    #[tokio::test]
    async fn terminate_group_escalates_past_ignored_sigterm() {
        let mut g = spawn_group(
            "sh",
            &["-c", "trap '' TERM; echo ready; while :; do sleep 1; done"],
            &env(),
            None,
            Stdio::null(),
        )
        .await
        .unwrap();
        let mut lines = BufReader::new(g.child.stdout.take().unwrap()).lines();
        lines.next_line().await.unwrap(); // trap is installed
        let st = g.terminate_group(Duration::from_millis(200)).await.unwrap();
        assert!(!st.success());
    }

    // PR_SET_PDEATHSIG has no reliable black-box assertion: it is not exposed in
    // /proc/<pid>/status, and observing it needs the parent thread to die while the test
    // process lives. Deliberately untested rather than flaky.
}
