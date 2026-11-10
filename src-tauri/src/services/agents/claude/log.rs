//! Session logs (D36): normalized events as JSONL plus a sibling raw-line file for
//! debugging. Token shapes are redacted in both; past the size limit only control
//! records are kept, after one "log limit reached" warning.
use std::{
    fs::{self, File},
    io::{self, BufRead, BufReader, Write},
    path::Path,
};

use serde_json::Value;

use crate::persistence::UsageTotals;
use crate::services::agents::{AgentEvent, secrets::redact};

pub(super) const LOG_LIMIT_BYTES: u64 = 20 * 1024 * 1024;

pub(super) struct SessionLog {
    events: File,
    raw: File,
    events_len: u64,
    raw_len: u64,
    limit: u64,
    limit_noted: bool,
}

/// Appends one redacted line. Logging is best-effort: a full disk must not kill the run,
/// so write errors are dropped.
fn append(file: &mut File, len: &mut u64, line: &str) {
    let line = redact(line);
    if writeln!(file, "{line}").is_ok() {
        *len += line.len() as u64 + 1;
    }
}

impl SessionLog {
    /// `path` is the events file; the raw lines go to `<name>.raw.jsonl` beside it.
    pub(super) fn create(path: &Path, limit: u64) -> io::Result<Self> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        Ok(Self {
            events: File::create(path)?,
            raw: File::create(path.with_extension("raw.jsonl"))?,
            events_len: 0,
            raw_len: 0,
            limit,
            limit_noted: false,
        })
    }

    pub(super) fn event(&mut self, e: &AgentEvent) {
        if !e.is_control() && self.events_len >= self.limit {
            if !self.limit_noted {
                self.limit_noted = true;
                let note = AgentEvent::Warning {
                    seq: e.seq(),
                    at_ms: 0,
                    text: "log limit reached".into(),
                    details: None,
                };
                self.write(&note);
            }
            return;
        }
        self.write(e);
    }

    fn write(&mut self, e: &AgentEvent) {
        if let Ok(line) = serde_json::to_string(e) {
            append(&mut self.events, &mut self.events_len, &line);
        }
    }

    /// Past the limit only `result` lines are kept: the run's usage is read from them.
    pub(super) fn raw(&mut self, line: &str) {
        if self.raw_len < self.limit || is_result(line) {
            append(&mut self.raw, &mut self.raw_len, line);
        }
    }
}

fn is_result(line: &str) -> bool {
    line.contains(r#""type":"result""#)
}

/// One run's token usage and cost, summed over the `result` lines of its raw log (a resume
/// respawned fresh has two). A missing or unreadable log counts as zero.
pub(crate) fn raw_log_usage(log_path: &Path) -> UsageTotals {
    let mut u = UsageTotals::default();
    let Ok(file) = File::open(log_path.with_extension("raw.jsonl")) else {
        return u;
    };
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Some(v) = is_result(&line)
            .then(|| serde_json::from_str::<Value>(&line).ok())
            .flatten()
        else {
            continue;
        };
        let n = |k: &str| v["usage"][k].as_i64().unwrap_or(0);
        u.input_tokens += n("input_tokens");
        u.output_tokens += n("output_tokens");
        u.cache_read_tokens += n("cache_read_input_tokens");
        u.cache_write_tokens += n("cache_creation_input_tokens");
        u.cost_usd += v["total_cost_usd"].as_f64().unwrap_or(0.0);
    }
    u
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agents::{Denial, Outcome};

    fn lines(path: &Path) -> Vec<serde_json::Value> {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn text(seq: u32, t: &str) -> AgentEvent {
        AgentEvent::Text {
            seq,
            at_ms: 0,
            text: t.into(),
        }
    }

    #[test]
    fn redacts_tokens_in_both_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions/s1.jsonl");
        let mut log = SessionLog::create(&path, LOG_LIMIT_BYTES).unwrap();
        log.event(&text(0, "my key is sk-ant-secret123"));
        log.raw(r#"{"x":"ghp_abcdef123456"}"#);
        let events = fs::read_to_string(&path).unwrap();
        let raw = fs::read_to_string(dir.path().join("sessions/s1.raw.jsonl")).unwrap();
        assert!(events.contains("[redacted]") && !events.contains("secret123"));
        assert!(raw.contains("[redacted]") && !raw.contains("abcdef"));
    }

    #[test]
    fn past_the_limit_only_control_records_survive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        let mut log = SessionLog::create(&path, 200).unwrap();
        for i in 0..10 {
            log.event(&text(i, &"x".repeat(60)));
        }
        let big_plan = "p".repeat(5000);
        log.event(&AgentEvent::PlanReady {
            seq: 10,
            at_ms: 0,
            plan: big_plan.clone(),
        });
        log.event(&text(11, "dropped"));
        log.event(&AgentEvent::Completed {
            seq: 12,
            at_ms: 0,
            outcome: Outcome::Success,
            denials: vec![Denial::new(
                "Bash".into(),
                serde_json::json!({"command": "ls"}),
            )],
        });
        let got = lines(&path);
        let kinds: Vec<_> = got.iter().map(|v| v["kind"].as_str().unwrap()).collect();
        let notes = kinds.iter().filter(|k| **k == "warning").count();
        assert_eq!(notes, 1, "{kinds:?}");
        assert!(got.iter().any(|v| v["text"] == "log limit reached"));
        assert!(got.iter().all(|v| v["text"] != "dropped"));
        let plan = got.iter().find(|v| v["kind"] == "planReady").unwrap();
        assert_eq!(
            plan["plan"].as_str().unwrap(),
            big_plan,
            "control records are whole"
        );
        assert_eq!(*kinds.last().unwrap(), "completed");
    }

    #[test]
    fn raw_usage_sums_result_lines_kept_past_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        let mut log = SessionLog::create(&path, 50).unwrap();
        log.raw(&"x".repeat(60)); // reaches the limit
        log.raw(r#"{"type":"assistant","usage":{"input_tokens":999}}"#); // dropped
        log.raw(r#"{"type":"result","total_cost_usd":0.5,"usage":{"input_tokens":10,"output_tokens":20,"cache_read_input_tokens":300,"cache_creation_input_tokens":40}}"#);
        log.raw(r#"{"type":"result","total_cost_usd":0.25,"usage":{"input_tokens":1}}"#);
        let u = raw_log_usage(&path);
        let tokens = (
            u.input_tokens,
            u.output_tokens,
            u.cache_read_tokens,
            u.cache_write_tokens,
        );
        assert_eq!(tokens, (11, 20, 300, 40));
        assert!((u.cost_usd - 0.75).abs() < 1e-9);
        let missing = raw_log_usage(&dir.path().join("missing.jsonl"));
        assert_eq!(missing, UsageTotals::default());
    }
}
