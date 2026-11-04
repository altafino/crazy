//! Spawning short-lived host tools with a timeout, an output cap and group kill (D14).
use crate::platform::process::spawn_group;
use std::{
    ffi::OsStr,
    io,
    path::Path,
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::io::AsyncReadExt;

pub(crate) const OUTPUT_CAP: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum RunError {
    #[error("could not start: {0}")]
    Spawn(#[from] io::Error),
    #[error("timed out after {}s", .0.as_secs_f32())]
    TimedOut(Duration),
}

pub(crate) struct Output {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `program` with exactly `env` (cleared first). On timeout the whole process group
/// is SIGKILLed and reaped, so shells that fork helpers leave nothing behind.
pub(crate) async fn run_capped<S: AsRef<OsStr>>(
    program: impl AsRef<OsStr>,
    args: &[S],
    env: &[(String, String)],
    timeout: Duration,
) -> Result<Output, RunError> {
    run_capped_in(program, args, env, None, timeout).await
}

/// [`run_capped`] with a working directory.
pub(crate) async fn run_capped_in<S: AsRef<OsStr>>(
    program: impl AsRef<OsStr>,
    args: &[S],
    env: &[(String, String)],
    cwd: Option<&Path>,
    timeout: Duration,
) -> Result<Output, RunError> {
    let mut group = spawn_group(program, args, env, cwd, Stdio::null()).await?;
    let (out, err) = (group.child.stdout.take(), group.child.stderr.take());

    let work = async { tokio::join!(group.child.wait(), read_capped(out), read_capped(err)) };
    match tokio::time::timeout(timeout, work).await {
        Ok((status, stdout, stderr)) => Ok(Output {
            status: status?,
            stdout,
            stderr,
        }),
        Err(_) => {
            let _ = group.kill_group().await;
            Err(RunError::TimedOut(timeout))
        }
    }
}

/// Keeps the first `OUTPUT_CAP` bytes and drains the rest so the child never blocks on a full pipe.
pub(crate) async fn read_capped(pipe: Option<impl tokio::io::AsyncRead + Unpin>) -> String {
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
