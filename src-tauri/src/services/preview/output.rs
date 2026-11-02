//! Bounded preview output (M3-D20): the last N lines in memory, a size-capped log file, and
//! a pending batch the service flushes to the UI every 100 ms.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::services::settings::PreviewLimits;

pub(crate) const TRUNCATED_MARKER: &str =
    "[log truncated: this preview's log file reached its size limit; the live view continues]";

pub(crate) fn log_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("previews")
}

pub(crate) fn log_path(data_dir: &Path, job_id: &str) -> PathBuf {
    log_dir(data_dir).join(format!("{job_id}.log"))
}

pub(crate) struct LogSink {
    lines: VecDeque<String>,
    /// Sequence number of `lines[0]`; the next pushed line gets `base + lines.len()`.
    base: u32,
    max_lines: usize,
    file: Option<BufWriter<File>>,
    written: u64,
    cap: u64,
    truncated: bool,
    pending: Vec<String>,
    pending_from: u32,
}

/// A slice of a preview's output for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tail {
    pub lines: Vec<String>,
    /// Sequence number of the line after the last one returned; live events carry the
    /// `from_seq` of their first line, so a view drops anything below this.
    pub next_seq: u32,
    pub log_truncated: bool,
}

impl LogSink {
    /// `path` is where the log file goes; a file that can't be created only costs the file.
    pub(crate) fn new(path: &Path, limits: &PreviewLimits) -> Self {
        let file = path
            .parent()
            .map(fs::create_dir_all)
            .transpose()
            .ok()
            .and_then(|_| File::create(path).ok())
            .map(BufWriter::new);
        Self {
            lines: VecDeque::new(),
            base: 0,
            max_lines: limits.max_lines as usize,
            file,
            written: 0,
            cap: u64::from(limits.max_log_mb) * 1024 * 1024,
            truncated: false,
            pending: Vec::new(),
            pending_from: 0,
        }
    }

    fn next_seq(&self) -> u32 {
        self.base + u32::try_from(self.lines.len()).unwrap_or(u32::MAX)
    }

    fn keep(&mut self, line: String) {
        if self.pending.is_empty() {
            self.pending_from = self.next_seq();
        }
        self.pending.push(line.clone());
        self.lines.push_back(line);
        if self.lines.len() > self.max_lines {
            self.lines.pop_front();
            self.base += 1;
        }
    }

    pub(crate) fn push(&mut self, line: String) {
        if !self.truncated {
            let need = line.len() as u64 + 1;
            if self.written + need > self.cap {
                self.truncated = true;
                if let Some(f) = &mut self.file {
                    let _ = writeln!(f, "{TRUNCATED_MARKER}");
                }
                self.keep(TRUNCATED_MARKER.to_owned());
            } else if let Some(f) = &mut self.file {
                if writeln!(f, "{line}").is_err() {
                    self.file = None;
                }
                self.written += need;
            }
        }
        self.keep(line);
    }

    /// The lines gathered since the last call, with the sequence number of the first.
    pub(crate) fn take_batch(&mut self) -> Option<(u32, Vec<String>)> {
        if let Some(f) = &mut self.file {
            let _ = f.flush();
        }
        (!self.pending.is_empty()).then(|| (self.pending_from, std::mem::take(&mut self.pending)))
    }

    pub(crate) fn tail(&self) -> Tail {
        Tail {
            lines: self.lines.iter().cloned().collect(),
            next_seq: self.next_seq(),
            log_truncated: self.truncated,
        }
    }
}

/// The last `max_lines` lines of a finished preview's log file, reading at most the final
/// 8 MB. `None` when the file is gone (pruned) or unreadable.
pub(crate) fn read_tail(path: &Path, max_lines: usize) -> Option<Tail> {
    const WINDOW: u64 = 8 * 1024 * 1024;
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(WINDOW);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<&str> = text.lines().collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0); // a partial first line
    }
    let skip = lines.len().saturating_sub(max_lines);
    let lines: Vec<String> = lines[skip..].iter().map(|s| (*s).to_owned()).collect();
    let log_truncated = lines.iter().any(|l| l == TRUNCATED_MARKER);
    Some(Tail {
        lines,
        next_seq: 0,
        log_truncated,
    })
}

/// Deletes the log files of all but the newest `keep` previews. `job_ids` is newest first;
/// the job rows stay.
pub(crate) fn prune_logs(data_dir: &Path, job_ids: &[String], keep: usize) {
    for id in job_ids.iter().skip(keep) {
        let _ = fs::remove_file(log_path(data_dir, id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(lines: u32, mb: u32) -> PreviewLimits {
        PreviewLimits {
            max_lines: lines,
            max_log_mb: mb,
            keep_logs: 5,
        }
    }

    #[test]
    fn memory_keeps_only_the_last_lines_and_sequence_numbers_stay_absolute() {
        let d = tempfile::tempdir().unwrap();
        // 1,000 is the smallest the settings allow; the sink itself doesn't care.
        let mut s = LogSink::new(&d.path().join("a.log"), &limits(1_000, 1));
        for i in 0..1_500 {
            s.push(format!("l{i}"));
        }
        let t = s.tail();
        assert_eq!(t.lines.len(), 1_000);
        assert_eq!(t.lines[0], "l500");
        assert_eq!(t.lines.last().unwrap(), "l1499");
        assert_eq!(t.next_seq, 1_500);
        // The file still has everything (it is under the cap).
        s.take_batch();
        let file = fs::read_to_string(d.path().join("a.log")).unwrap();
        assert_eq!(file.lines().count(), 1_500);
    }

    #[test]
    fn batches_carry_the_first_sequence_number_and_empty_means_none() {
        let d = tempfile::tempdir().unwrap();
        let mut s = LogSink::new(&d.path().join("a.log"), &limits(1_000, 1));
        assert!(s.take_batch().is_none());
        s.push("a".into());
        s.push("b".into());
        assert_eq!(s.take_batch(), Some((0, vec!["a".into(), "b".into()])));
        assert!(s.take_batch().is_none());
        s.push("c".into());
        assert_eq!(s.take_batch(), Some((2, vec!["c".into()])));
    }

    #[test]
    fn file_is_capped_with_a_marker_and_the_live_tail_continues() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("a.log");
        let mut s = LogSink::new(&path, &limits(1_000, 1));
        let line = "x".repeat(1023); // 1 KiB with the newline
        for _ in 0..1_100 {
            s.push(line.clone());
        }
        s.push("after the cap".into());
        s.take_batch();
        let size = fs::metadata(&path).unwrap().len();
        assert!(
            size <= 1024 * 1024 + TRUNCATED_MARKER.len() as u64 + 1,
            "{size}"
        );
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().last().unwrap(), TRUNCATED_MARKER);
        assert!(!text.contains("after the cap"));
        let t = s.tail();
        assert!(t.log_truncated);
        assert_eq!(t.lines.last().unwrap(), "after the cap");
        // A finished preview's tail comes back from the file with the marker flagged.
        assert!(read_tail(&path, 10).unwrap().log_truncated);
    }

    #[test]
    fn read_tail_returns_the_last_lines_or_none() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("a.log");
        fs::write(&path, "1\n2\n3\n4\n").unwrap();
        assert_eq!(read_tail(&path, 2).unwrap().lines, ["3", "4"]);
        assert!(read_tail(&d.path().join("missing.log"), 2).is_none());
    }

    #[test]
    fn prune_keeps_the_newest_files() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir_all(log_dir(d.path())).unwrap();
        let ids: Vec<String> = (0..7).map(|i| format!("j{i}")).collect(); // newest first
        for id in &ids {
            fs::write(log_path(d.path(), id), "x").unwrap();
        }
        prune_logs(d.path(), &ids, 5);
        let exists = |i: usize| log_path(d.path(), &ids[i]).exists();
        assert!((0..5).all(exists));
        assert!(!exists(5) && !exists(6));
    }
}
