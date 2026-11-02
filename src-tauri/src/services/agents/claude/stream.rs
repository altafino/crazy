//! Claude Code stream-json lines -> normalized events. This is the only place that knows
//! the CLI's wire format (red flag #2). Parsing is lenient: unknown shapes become a
//! `warning`, never an abort.
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::services::agents::{AgentEvent, Denial};

/// Longest tool output kept in an event or the log (D36).
pub(super) const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const TRUNCATED: &str = "…[truncated]";

/// What one CLI line meant. Events carry `seq: 0`; the run state stamps the real number.
pub(super) enum Item {
    Event(AgentEvent),
    Init {
        session_id: String,
    },
    Approval {
        request_id: String,
        tool: String,
        input: Value,
        description: String,
    },
    Result(ResultInfo),
}

pub(super) struct ResultInfo {
    pub is_error: bool,
    pub subtype: String,
    pub text: String,
    pub session_id: Option<String>,
    pub denials: Vec<Denial>,
}

pub(super) struct Parser {
    project_dir: PathBuf,
}

fn warn(text: impl Into<String>) -> Item {
    Item::Event(AgentEvent::Warning {
        seq: 0,
        text: text.into(),
    })
}

fn str_at<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str()
}

/// Cuts `s` to `max` bytes on a char boundary and marks the cut.
pub(super) fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{TRUNCATED}", &s[..end])
}

impl Parser {
    pub(super) fn new(project_dir: PathBuf) -> Self {
        Self { project_dir }
    }

    pub(super) fn parse(&self, line: &str) -> Vec<Item> {
        if line.trim().is_empty() {
            return Vec::new();
        }
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return vec![warn(format!("unreadable line from the agent: {e}"))],
        };
        match str_at(&v, "type") {
            Some("system") => self.system(&v),
            Some("assistant") => self.assistant(&v),
            Some("user") => self.user(&v),
            Some("control_request") => self.control_request(&v),
            Some("result") => result(&v),
            // Informational lines with nothing to show.
            Some("rate_limit_event") => Vec::new(),
            Some(other) => vec![warn(format!(
                "unknown message type from the agent: {other}"
            ))],
            None => vec![warn("message from the agent has no type")],
        }
    }

    fn system(&self, v: &Value) -> Vec<Item> {
        if str_at(v, "subtype") != Some("init") {
            return Vec::new(); // status, permission_denied, hooks: not shown
        }
        let session_id = str_at(v, "session_id").unwrap_or_default().to_owned();
        let model = str_at(v, "model").unwrap_or("unknown model");
        let mode = str_at(v, "permissionMode").unwrap_or("default");
        vec![
            Item::Event(AgentEvent::Status {
                seq: 0,
                text: format!("Session started ({model}, {mode} permissions)"),
            }),
            Item::Init { session_id },
        ]
    }

    fn assistant(&self, v: &Value) -> Vec<Item> {
        let Some(blocks) = v.pointer("/message/content").and_then(Value::as_array) else {
            return vec![warn("assistant message without content")];
        };
        let mut items = Vec::new();
        for b in blocks {
            match str_at(b, "type") {
                Some("text") => {
                    let text = str_at(b, "text").unwrap_or_default();
                    if !text.trim().is_empty() {
                        items.push(Item::Event(AgentEvent::Text {
                            seq: 0,
                            text: text.to_owned(),
                        }));
                    }
                }
                Some("tool_use") => items.extend(self.tool_use(b)),
                Some("thinking" | "redacted_thinking") => {}
                other => items.push(warn(format!(
                    "unknown assistant block from the agent: {}",
                    other.unwrap_or("?")
                ))),
            }
        }
        items
    }

    fn tool_use(&self, b: &Value) -> Vec<Item> {
        let name = str_at(b, "name").unwrap_or("tool").to_owned();
        let input = b.get("input").cloned().unwrap_or(Value::Null);
        let mut items = vec![Item::Event(AgentEvent::ToolCall {
            seq: 0,
            id: str_at(b, "id").unwrap_or_default().to_owned(),
            summary: self.summary(&name, &input),
            name: name.clone(),
            input: input.clone(),
        })];
        if matches!(name.as_str(), "Write" | "Edit" | "MultiEdit")
            && let Some(path) = str_at(&input, "file_path")
        {
            items.push(Item::Event(AgentEvent::FileChange {
                seq: 0,
                path: self.relative(path),
            }));
        }
        items
    }

    fn relative(&self, path: &str) -> String {
        std::path::Path::new(path)
            .strip_prefix(&self.project_dir)
            .map_or_else(|_| path.to_owned(), |p| p.display().to_string())
    }

    fn summary(&self, name: &str, input: &Value) -> String {
        let detail = match name {
            "Bash" => str_at(input, "command").map(|c| c.lines().next().unwrap_or("").to_owned()),
            "Read" | "Write" | "Edit" | "MultiEdit" => {
                str_at(input, "file_path").map(|p| self.relative(p))
            }
            "Grep" | "Glob" => str_at(input, "pattern").map(str::to_owned),
            _ => None,
        };
        match detail {
            Some(d) => truncate(&format!("{name}: {d}"), 200),
            None => name.to_owned(),
        }
    }

    fn user(&self, v: &Value) -> Vec<Item> {
        // A plain string is an echo of the prompt; only tool results matter.
        let Some(blocks) = v.pointer("/message/content").and_then(Value::as_array) else {
            return Vec::new();
        };
        blocks
            .iter()
            .filter(|b| str_at(b, "type") == Some("tool_result"))
            .map(|b| {
                Item::Event(AgentEvent::ToolResult {
                    seq: 0,
                    id: str_at(b, "tool_use_id").unwrap_or_default().to_owned(),
                    is_error: b.get("is_error").and_then(Value::as_bool).unwrap_or(false),
                    output: truncate(&tool_output(b.get("content")), MAX_OUTPUT_BYTES),
                })
            })
            .collect()
    }

    fn control_request(&self, v: &Value) -> Vec<Item> {
        let req = v.get("request").unwrap_or(&Value::Null);
        match (str_at(req, "subtype"), str_at(v, "request_id")) {
            (Some("can_use_tool"), Some(id)) => vec![Item::Approval {
                request_id: id.to_owned(),
                tool: str_at(req, "tool_name").unwrap_or("tool").to_owned(),
                input: req.get("input").cloned().unwrap_or(Value::Null),
                description: str_at(req, "description")
                    .or_else(|| str_at(req, "display_name"))
                    .unwrap_or_default()
                    .to_owned(),
            }],
            (sub, _) => vec![warn(format!(
                "unsupported request from the agent: {}",
                sub.unwrap_or("?")
            ))],
        }
    }
}

/// Tool output is a string or a list of `{type: text, text}` blocks.
fn tool_output(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|b| str_at(b, "text"))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn result(v: &Value) -> Vec<Item> {
    let denials = v
        .get("permission_denials")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|d| {
                    Some(Denial::new(
                        str_at(d, "tool_name")?.to_owned(),
                        d.get("tool_input").cloned().unwrap_or_else(|| json!({})),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let usage = AgentEvent::Usage {
        seq: 0,
        cost_usd: v.get("total_cost_usd").and_then(Value::as_f64),
        duration_ms: v
            .get("duration_ms")
            .and_then(Value::as_u64)
            .map_or(0, |ms| u32::try_from(ms).unwrap_or(u32::MAX)),
    };
    vec![
        Item::Event(usage),
        Item::Result(ResultInfo {
            is_error: v.get("is_error").and_then(Value::as_bool).unwrap_or(false),
            subtype: str_at(v, "subtype").unwrap_or("success").to_owned(),
            text: str_at(v, "result").unwrap_or_default().to_owned(),
            session_id: str_at(v, "session_id").map(str::to_owned),
            denials,
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_respects_char_boundaries() {
        assert_eq!(truncate("abc", 10), "abc");
        assert_eq!(truncate("aéb", 2), "a…[truncated]");
        assert!(truncate(&"x".repeat(100), 10).ends_with("…[truncated]"));
    }

    #[test]
    fn malformed_and_unknown_lines_warn() {
        let p = Parser::new("/p".into());
        for line in ["{not json", r#"{"type":"mystery"}"#, r#"{"a":1}"#] {
            let items = p.parse(line);
            assert!(
                matches!(&items[..], [Item::Event(AgentEvent::Warning { .. })]),
                "{line}"
            );
        }
        assert!(p.parse("   ").is_empty());
        assert!(p.parse(r#"{"type":"rate_limit_event"}"#).is_empty());
    }

    #[test]
    fn write_tool_gets_a_relative_file_change_hint() {
        let p = Parser::new("/project".into());
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t","name":"Write","input":{"file_path":"/project/src/a.rs","content":"x"}}]}}"#;
        let events: Vec<_> = p
            .parse(line)
            .into_iter()
            .filter_map(|i| match i {
                Item::Event(e) => Some(e),
                _ => None,
            })
            .collect();
        assert!(
            matches!(&events[0], AgentEvent::ToolCall { summary, .. } if summary == "Write: src/a.rs")
        );
        assert!(matches!(&events[1], AgentEvent::FileChange { path, .. } if path == "src/a.rs"));
    }
}
