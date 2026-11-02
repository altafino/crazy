//! Spawning short-lived host tools with a timeout, an output cap and group kill (D14).
use std::{ffi::OsStr, io, process::ExitStatus, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};

pub(super) const OUTPUT_CAP: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub(super) enum RunError {
    #[error("could not start: {0}")]
    Spawn(#[from] io::Error),
    #[error("timed out after {}s", .0.as_secs_f32())]
    TimedOut(Duration),
}

pub(super) struct Output {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `program` with exactly `env` (cleared first). On timeout the whole process group
/// is SIGKILLed and reaped, so shells that fork helpers leave nothing behind.
pub(super) async fn run_capped<S: AsRef<OsStr>>(
    program: impl AsRef<OsStr>,
    args: &[S],
    env: &[(String, String)],
    timeout: Duration,
) -> Result<Output, RunError> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
    let mut child = spawn_retrying(&mut cmd).await?;
    let pid = child.id().and_then(|p| i32::try_from(p).ok());
    let (out, err) = (child.stdout.take(), child.stderr.take());

    let work = async { tokio::join!(child.wait(), read_capped(out), read_capped(err)) };
    match tokio::time::timeout(timeout, work).await {
        Ok((status, stdout, stderr)) => Ok(Output {
            status: status?,
            stdout,
            stderr,
        }),
        Err(_) => {
            if let Some(pgid) = pid {
                // SAFETY: plain syscall; pgid is our own child's group (process_group(0)).
                unsafe { libc::killpg(pgid, libc::SIGKILL) };
            }
            let _ = child.wait().await;
            Err(RunError::TimedOut(timeout))
        }
    }
}

/// Freshly written scripts can briefly fail with ETXTBSY when another thread forks.
async fn spawn_retrying(cmd: &mut Command) -> io::Result<tokio::process::Child> {
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

/// Keeps the first `OUTPUT_CAP` bytes and drains the rest so the child never blocks on a full pipe.
async fn read_capped(pipe: Option<impl tokio::io::AsyncRead + Unpin>) -> String {
    let Some(mut pipe) = pipe else {
        return String::new();
    };
    let (mut buf, mut chunk) = (Vec::new(), [0u8; 8192]);
    while let Ok(n) = pipe.read(&mut chunk).await {
        if n == 0 {
            break;
        }
        let room = OUTPUT_CAP - buf.len();
        buf.extend_from_slice(&chunk[..n.min(room)]);
    }
    String::from_utf8_lossy(&buf).into_owned()
}
