//! Agent services: the normalized events (F04), the stateless `AgentAdapter` trait and
//! the types a run exchanges with the job supervisor. Provider specifics live in
//! `claude/`; nothing from a provider's wire format leaves it.

use std::{future::Future, io, path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use tokio::sync::{mpsc, watch};

use crate::persistence::SessionMode;
use profiles::{AgentProfile, ProbeResult};

pub mod claude;
pub mod profiles;
pub(crate) mod secrets;

// ----------------------------------------------------------------- outcome

/// How a run ended: success, or the F05 error kind. Also the job's `error_kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Success,
    NotInstalled,
    Unauthenticated,
    ExpiredLogin,
    IncompatibleCli,
    MissingDependency,
    Timeout,
    Cancelled,
    BudgetExceeded,
    Failed,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::NotInstalled => "notInstalled",
            Self::Unauthenticated => "unauthenticated",
            Self::ExpiredLogin => "expiredLogin",
            Self::IncompatibleCli => "incompatibleCli",
            Self::MissingDependency => "missingDependency",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::BudgetExceeded => "budgetExceeded",
            Self::Failed => "failed",
        }
    }
}

/// A tool use the CLI refused because nobody could approve it (fallback mode, or a deny).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Denial {
    pub tool: String,
    #[specta(type = specta_typescript::Unknown)]
    pub input: Value,
    /// The exact rule "Allow and retry" adds to `allowed_tools` for one run.
    pub rule: Option<String>,
}

impl Denial {
    pub fn new(tool: String, input: Value) -> Self {
        let rule = allow_rule(&tool, &input);
        Self { tool, input, rule }
    }
}

/// The exact tool rule that would have permitted this denied call: `Bash(<command>)`,
/// `Write(<path>)` or `Edit(<path>)`. `\`, `(` and `)` are escaped so the rule matches
/// that one call literally. Other tools get no rule (they are not offered "Allow and retry").
pub fn allow_rule(tool: &str, input: &Value) -> Option<String> {
    let key = match tool {
        "Bash" => "command",
        "Write" | "Edit" => "file_path",
        _ => return None,
    };
    let arg = input.get(key)?.as_str()?;
    let escaped = arg
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)");
    Some(format!("{tool}({escaped})"))
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    pub outcome: Outcome,
    pub message: Option<String>,
    pub exit_code: Option<i32>,
    /// The provider's own session id, for `--resume`.
    pub provider_session_id: Option<String>,
    pub denials: Vec<Denial>,
    /// False when Studio killed the process (cancel, timeout): no AFTER snapshot is taken.
    pub exited_normally: bool,
}

// ------------------------------------------------------------------ events

/// One thing that happened in a run, in the order it happened. `seq` counts from 0 per
/// run so a view can subscribe, replay the session log and drop duplicates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentEvent {
    Status {
        seq: u32,
        /// Unix ms when it happened; 0 in logs written before this existed.
        #[serde(default)]
        #[specta(type = f64)] // ms fit exactly; specta forbids i64
        at_ms: i64,
        text: String,
    },
    Text {
        seq: u32,
        /// Unix ms when it happened; 0 in logs written before this existed.
        #[serde(default)]
        #[specta(type = f64)] // ms fit exactly; specta forbids i64
        at_ms: i64,
        text: String,
    },
    ToolCall {
        seq: u32,
        /// Unix ms when it happened; 0 in logs written before this existed.
        #[serde(default)]
        #[specta(type = f64)] // ms fit exactly; specta forbids i64
        at_ms: i64,
        id: String,
        name: String,
        #[specta(type = specta_typescript::Unknown)]
        input: Value,
        summary: String,
    },
    ToolResult {
        seq: u32,
        /// Unix ms when it happened; 0 in logs written before this existed.
        #[serde(default)]
        #[specta(type = f64)] // ms fit exactly; specta forbids i64
        at_ms: i64,
        id: String,
        is_error: bool,
        output: String,
    },
    /// A display hint from Write/Edit inputs; the real changes come from snapshots.
    FileChange {
        seq: u32,
        /// Unix ms when it happened; 0 in logs written before this existed.
        #[serde(default)]
        #[specta(type = f64)] // ms fit exactly; specta forbids i64
        at_ms: i64,
        path: String,
    },
    ApprovalNeeded {
        seq: u32,
        /// Unix ms when it happened; 0 in logs written before this existed.
        #[serde(default)]
        #[specta(type = f64)] // ms fit exactly; specta forbids i64
        at_ms: i64,
        request_id: String,
        tool: String,
        #[specta(type = specta_typescript::Unknown)]
        input: Value,
        description: String,
    },
    ApprovalAnswered {
        seq: u32,
        /// Unix ms when it happened; 0 in logs written before this existed.
        #[serde(default)]
        #[specta(type = f64)] // ms fit exactly; specta forbids i64
        at_ms: i64,
        request_id: String,
        allowed: bool,
    },
    /// Auto-approve allowed this request without asking (Settings > Approvals); no card.
    AutoAllowed {
        seq: u32,
        #[specta(type = f64)] // ms fit exactly; specta forbids i64
        at_ms: i64,
        request_id: String,
        tool: String,
        #[specta(type = specta_typescript::Unknown)]
        input: Value,
        description: String,
    },
    Usage {
        seq: u32,
        /// Unix ms when it happened; 0 in logs written before this existed.
        #[serde(default)]
        #[specta(type = f64)] // ms fit exactly; specta forbids i64
        at_ms: i64,
        cost_usd: Option<f64>,
        duration_ms: u32,
    },
    Warning {
        seq: u32,
        /// Unix ms when it happened; 0 in logs written before this existed.
        #[serde(default)]
        #[specta(type = f64)] // ms fit exactly; specta forbids i64
        at_ms: i64,
        text: String,
        /// Extra detail for the row's expander (e.g. the Git error behind a missing checkpoint).
        #[serde(default)]
        details: Option<String>,
    },
    Error {
        seq: u32,
        /// Unix ms when it happened; 0 in logs written before this existed.
        #[serde(default)]
        #[specta(type = f64)] // ms fit exactly; specta forbids i64
        at_ms: i64,
        error_kind: Outcome,
        message: String,
    },
    PlanReady {
        seq: u32,
        /// Unix ms when it happened; 0 in logs written before this existed.
        #[serde(default)]
        #[specta(type = f64)] // ms fit exactly; specta forbids i64
        at_ms: i64,
        plan: String,
    },
    Completed {
        seq: u32,
        /// Unix ms when it happened; 0 in logs written before this existed.
        #[serde(default)]
        #[specta(type = f64)] // ms fit exactly; specta forbids i64
        at_ms: i64,
        outcome: Outcome,
        denials: Vec<Denial>,
    },
}

impl AgentEvent {
    pub fn seq(&self) -> u32 {
        match self {
            Self::Status { seq, .. }
            | Self::Text { seq, .. }
            | Self::ToolCall { seq, .. }
            | Self::ToolResult { seq, .. }
            | Self::FileChange { seq, .. }
            | Self::ApprovalNeeded { seq, .. }
            | Self::ApprovalAnswered { seq, .. }
            | Self::AutoAllowed { seq, .. }
            | Self::Usage { seq, .. }
            | Self::Warning { seq, .. }
            | Self::Error { seq, .. }
            | Self::PlanReady { seq, .. }
            | Self::Completed { seq, .. } => *seq,
        }
    }

    /// Stamps the run-order number and the wall-clock time; the one place both are assigned.
    pub fn set_meta(&mut self, n: u32, ms: i64) {
        match self {
            Self::Status { seq, at_ms, .. }
            | Self::Text { seq, at_ms, .. }
            | Self::ToolCall { seq, at_ms, .. }
            | Self::ToolResult { seq, at_ms, .. }
            | Self::FileChange { seq, at_ms, .. }
            | Self::ApprovalNeeded { seq, at_ms, .. }
            | Self::ApprovalAnswered { seq, at_ms, .. }
            | Self::AutoAllowed { seq, at_ms, .. }
            | Self::Usage { seq, at_ms, .. }
            | Self::Warning { seq, at_ms, .. }
            | Self::Error { seq, at_ms, .. }
            | Self::PlanReady { seq, at_ms, .. }
            | Self::Completed { seq, at_ms, .. } => {
                *seq = n;
                *at_ms = ms;
            }
        }
    }

    /// Approval, plan, usage, error and completion records: never dropped or truncated
    /// by the session log (D36).
    pub fn is_control(&self) -> bool {
        matches!(
            self,
            Self::ApprovalNeeded { .. }
                | Self::ApprovalAnswered { .. }
                | Self::AutoAllowed { .. }
                | Self::PlanReady { .. }
                | Self::Usage { .. }
                | Self::Error { .. }
                | Self::Completed { .. }
        )
    }
}

/// Where a run delivers its events; the job supervisor provides it. Called in order from
/// the run's task, so it must not block for long.
pub type EventSink = Arc<dyn Fn(AgentEvent) + Send + Sync>;

// ----------------------------------------------------------------- adapter

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // reported once a second adapter makes the UI branch on it (F14)
pub struct Capabilities {
    pub name: &'static str,
    pub plan_mode: bool,
    pub live_approvals: bool,
    pub resume: bool,
}

/// Auto-approve for one Make changes run: commands, file edits and reads are allowed without a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoApprove {
    /// Still ask for risky commands and edits outside the project.
    pub ask_for_risky: bool,
}

#[derive(Debug, Clone)]
pub struct RunRequest {
    pub project_dir: PathBuf,
    pub prompt: String,
    pub mode: SessionMode,
    pub resume_session_id: Option<String>,
    /// Tool rules permitted for this run only (Allow and retry); never stored on a profile.
    pub allowed_tools: Vec<String>,
    /// The conversation's attachments folder; kept readable (also on a resumed run).
    pub attachments_dir: Option<PathBuf>,
    /// Absolute paths of the attached files, in chip order; listed at the end of the prompt.
    pub attachments: Vec<PathBuf>,
    /// Set only for Make changes runs with Auto-approve on.
    pub auto_approve: Option<AutoApprove>,
    pub spawn_env: Vec<(String, String)>,
    /// Warnings the supervisor found before the start (text, details); they become the
    /// run's first `Warning` events so they get proper sequence numbers and log lines.
    pub pre_warnings: Vec<(String, Option<String>)>,
    /// Session log (JSONL of normalized events); the raw CLI lines go to the sibling
    /// `<name>.raw.jsonl`.
    pub log_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(Option<String>),
}

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("{0} was not found or is not executable")]
    NotInstalled(String),
    #[error("{0}")]
    IncompatibleCli(String),
    #[error("could not start the agent: {0}")]
    Io(#[from] io::Error),
}

impl StartError {
    pub fn outcome(&self) -> Outcome {
        match self {
            Self::NotInstalled(_) => Outcome::NotInstalled,
            Self::IncompatibleCli(_) => Outcome::IncompatibleCli,
            Self::Io(_) => Outcome::Failed,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("the run has already ended")]
pub struct RunEnded;

pub(crate) enum Command {
    Cancel,
    Answer {
        request_id: String,
        decision: Decision,
    },
}

/// Control surface of one live run. `&self` throughout, so the supervisor can share it
/// between the task that waits and the callers that cancel or answer.
pub struct RunHandle {
    commands: mpsc::UnboundedSender<Command>,
    done: watch::Receiver<Option<RunOutcome>>,
}

impl RunHandle {
    pub(crate) fn new(
        commands: mpsc::UnboundedSender<Command>,
        done: watch::Receiver<Option<RunOutcome>>,
    ) -> Self {
        Self { commands, done }
    }

    /// Terminates the run's process group (SIGTERM, then SIGKILL after a grace period).
    pub fn cancel(&self) {
        // A closed channel means the run already ended; nothing to cancel.
        let _ = self.commands.send(Command::Cancel);
    }

    pub fn answer_approval(&self, request_id: &str, decision: Decision) -> Result<(), RunEnded> {
        self.commands
            .send(Command::Answer {
                request_id: request_id.to_owned(),
                decision,
            })
            .map_err(|_| RunEnded)
    }

    pub async fn wait(&self) -> RunOutcome {
        let mut rx = self.done.clone();
        match rx.wait_for(Option::is_some).await {
            Ok(o) => o.clone().expect("checked by wait_for"),
            // The run task died without reporting (a panic).
            Err(_) => RunOutcome {
                outcome: Outcome::Failed,
                message: Some("the agent run ended unexpectedly".into()),
                exit_code: None,
                provider_session_id: None,
                denials: Vec::new(),
                exited_normally: false,
            },
        }
    }
}

/// Stateless: every call carries what it needs, and the supervisor alone knows which
/// runs exist (decision record, Shape 2).
pub trait AgentAdapter: Send + Sync + 'static {
    #[allow(dead_code)]
    fn capabilities(&self) -> Capabilities;

    fn probe(
        &self,
        profile: &AgentProfile,
        spawn_env: &[(String, String)],
    ) -> impl Future<Output = ProbeResult> + Send;

    fn start(
        &self,
        profile: &AgentProfile,
        request: RunRequest,
        sink: EventSink,
    ) -> impl Future<Output = Result<RunHandle, StartError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn allow_rules_are_exact_and_escaped() {
        let bash = json!({"command": "touch a.txt && echo (x)"});
        assert_eq!(
            allow_rule("Bash", &bash).unwrap(),
            r"Bash(touch a.txt && echo \(x\))"
        );
        let file = json!({"file_path": "/project/hello.txt", "content": "hi"});
        assert_eq!(
            allow_rule("Write", &file).unwrap(),
            "Write(/project/hello.txt)"
        );
        assert_eq!(
            allow_rule("Edit", &file).unwrap(),
            "Edit(/project/hello.txt)"
        );
        assert_eq!(allow_rule("WebFetch", &json!({"url": "x"})), None);
        assert_eq!(allow_rule("Bash", &json!({})), None);
    }

    #[test]
    fn old_logs_without_at_ms_replay_and_set_meta_stamps() {
        let old = r#"{"kind":"text","seq":4,"text":"hi"}"#;
        let mut e: AgentEvent = serde_json::from_str(old).unwrap();
        assert_eq!(
            e,
            AgentEvent::Text {
                seq: 4,
                at_ms: 0,
                text: "hi".into()
            }
        );
        e.set_meta(7, 1_700_000_000_123);
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({"kind": "text", "seq": 7, "atMs": 1_700_000_000_123_i64, "text": "hi"})
        );
    }

    #[test]
    fn events_serialize_camel_case_with_kind_and_seq() {
        let e = AgentEvent::ToolResult {
            seq: 3,
            at_ms: 0,
            id: "t".into(),
            is_error: true,
            output: "x".into(),
        };
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({"kind": "toolResult", "seq": 3, "atMs": 0, "id": "t", "isError": true, "output": "x"})
        );
        let back: AgentEvent = serde_json::from_value(serde_json::to_value(&e).unwrap()).unwrap();
        assert_eq!(back, e);
    }
}
