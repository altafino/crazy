//! Runs one child process in its own process group and streams its output lines to a sink
//! (M3-D6, shared with builds by M4-D14). The caller decides what a line means; this module
//! only splits pipes into lines, batches flushes, and ends the process cleanly.

use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::sync::{mpsc, watch};

use crate::platform::process::spawn_group;

/// How often the sink is asked to flush.
pub(crate) const BATCH_EVERY: Duration = Duration::from_millis(100);
/// How long output that arrives after the process exited is still collected.
const DRAIN_AFTER_EXIT: Duration = Duration::from_millis(500);

/// How a process ended.
#[derive(Debug)]
pub(crate) enum ProcEnd {
    Exited(ExitStatus),
    Cancelled,
    SpawnFailed(String),
}

/// What to run.
pub(crate) struct ProcSpec<'a> {
    pub program: &'a Path,
    pub args: &'a [String],
    /// The complete environment (the child's is cleared first).
    pub env: &'a [(String, String)],
    pub cwd: &'a Path,
}

/// Receives a process's output.
pub(crate) trait LineSink {
    /// One raw line, without its newline (it may still hold `\r` redraws).
    fn line(&self, raw: &str);
    /// Called every `BATCH_EVERY` and once more at the end.
    fn flush(&self);
}

/// Runs `spec` until it exits or `cancel` turns true. On cancel the group gets SIGTERM and,
/// after `grace`, SIGKILL. After either end, stragglers' output is drained for 500 ms and then
/// the whole group is killed, so nothing outlives the job (a dev server keeping its port).
pub(crate) async fn run_proc(
    spec: ProcSpec<'_>,
    sink: &impl LineSink,
    cancel: &mut watch::Receiver<bool>,
    grace: Duration,
) -> ProcEnd {
    let mut group = match spawn_group(
        spec.program,
        spec.args,
        spec.env,
        Some(spec.cwd),
        Stdio::null(),
    )
    .await
    {
        Ok(g) => g,
        Err(e) => return ProcEnd::SpawnFailed(format!("{}: {e}", spec.program.display())),
    };
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    if let Some(out) = group.child.stdout.take() {
        tokio::spawn(pump(out, tx.clone()));
    }
    if let Some(err) = group.child.stderr.take() {
        tokio::spawn(pump(err, tx.clone()));
    }
    drop(tx);

    let mut tick = tokio::time::interval(BATCH_EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let end = loop {
        tokio::select! {
            () = async { let _ = cancel.wait_for(|c| *c).await; } => {
                let _ = group.terminate_group(grace).await;
                break ProcEnd::Cancelled;
            }
            st = group.child.wait() => match st {
                Ok(st) => break ProcEnd::Exited(st),
                Err(e) => break ProcEnd::SpawnFailed(e.to_string()),
            },
            Some(line) = rx.recv() => sink.line(&line),
            _ = tick.tick() => sink.flush(),
        }
    };
    let _ = tokio::time::timeout(DRAIN_AFTER_EXIT, async {
        while let Some(line) = rx.recv().await {
            sink.line(&line);
        }
    })
    .await;
    let _ = group.kill_group().await;
    sink.flush();
    end
}

/// Splits a pipe into lines (on `\n`; `clean_line` handles `\r` redraws). A line without a
/// newline is cut at 64 KiB so one runaway line can't grow memory.
async fn pump(mut pipe: impl tokio::io::AsyncRead + Unpin, tx: mpsc::UnboundedSender<String>) {
    const MAX_LINE: usize = 64 * 1024;
    let (mut buf, mut chunk) = (Vec::new(), [0u8; 8192]);
    loop {
        let n = match pipe.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..n]);
        while let Some(i) = buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buf.drain(..=i).collect();
            let _ = tx.send(String::from_utf8_lossy(&line[..i]).into_owned());
        }
        if buf.len() > MAX_LINE {
            let _ = tx.send(String::from_utf8_lossy(&std::mem::take(&mut buf)).into_owned());
        }
    }
    if !buf.is_empty() {
        let _ = tx.send(String::from_utf8_lossy(&buf).into_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Collect(Mutex<Vec<String>>);
    impl LineSink for Collect {
        fn line(&self, raw: &str) {
            self.0.lock().unwrap().push(raw.to_owned());
        }
        fn flush(&self) {}
    }

    fn sh(script: &str) -> (Vec<String>, Vec<(String, String)>) {
        (
            vec!["-c".into(), script.into()],
            vec![("PATH".into(), "/usr/bin:/bin".into())],
        )
    }

    #[tokio::test]
    async fn streams_both_pipes_and_reports_the_exit_status() {
        let (args, env) = sh("echo out; echo err >&2; printf tail; exit 3");
        let sink = Collect::default();
        let (_tx, mut rx) = watch::channel(false);
        let spec = ProcSpec {
            program: Path::new("/bin/sh"),
            args: &args,
            env: &env,
            cwd: Path::new("/"),
        };
        let end = run_proc(spec, &sink, &mut rx, Duration::from_secs(1)).await;
        assert!(matches!(end, ProcEnd::Exited(st) if st.code() == Some(3)));
        let mut got = sink.0.lock().unwrap().clone();
        got.sort();
        assert_eq!(got, ["err", "out", "tail"]);
    }

    #[tokio::test]
    async fn cancel_stops_the_process_and_a_missing_program_fails_to_spawn() {
        let (args, env) = sh("sleep 30");
        let sink = Collect::default();
        let (tx, mut rx) = watch::channel(false);
        let spec = ProcSpec {
            program: Path::new("/bin/sh"),
            args: &args,
            env: &env,
            cwd: Path::new("/"),
        };
        tx.send(true).unwrap();
        let end = run_proc(spec, &sink, &mut rx, Duration::from_secs(1)).await;
        assert!(matches!(end, ProcEnd::Cancelled));

        let (_tx, mut rx) = watch::channel(false);
        let spec = ProcSpec {
            program: Path::new("/nonexistent/prog"),
            args: &[],
            env: &env,
            cwd: Path::new("/"),
        };
        let end = run_proc(spec, &sink, &mut rx, Duration::from_secs(1)).await;
        assert!(matches!(end, ProcEnd::SpawnFailed(_)));
    }
}
