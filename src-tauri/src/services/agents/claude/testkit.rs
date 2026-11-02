//! A fake `claude` for tests: a bash script written at test time that speaks just enough
//! stream-json. Behaviour is chosen by env vars carried on the profile.
//!
//! - `FAKE_SCENARIO`: write | approve | plan | hang | noinit | fail | auth | echo_argv
//! - `FAKE_HELP`: live (default) | fallback | none
//! - `FAKE_VERSION`: text printed for `--version`
//! - `FAKE_ARGV`, `FAKE_STDIN`, `FAKE_PIDFILE`: files the script records into
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::services::agents::{
    AgentEvent, EventSink,
    profiles::{AgentProfile, DEFAULT_INSTRUCTIONS, EnvVar},
};

const SCRIPT: &str = r##"#!/bin/bash
for a in "$@"; do
  case "$a" in
    --version) echo "${FAKE_VERSION:-2.1.316 (Claude Code)}"; exit 0;;
    --help)
      echo "Usage: claude [options]"
      case "${FAKE_HELP:-live}" in
        live) echo "  --permission-prompt-tool <tool>"; echo "  --permission-prompts <mode>";;
        fallback) echo "  --permission-prompts <mode>";;
      esac
      exit 0;;
  esac
done
[ -n "$FAKE_ARGV" ] && printf '%s\n' "$@" > "$FAKE_ARGV"
read -r prompt
[ -n "$FAKE_STDIN" ] && echo "$prompt" >> "$FAKE_STDIN"
init() { echo '{"type":"system","subtype":"init","session_id":"sess-1","model":"fake","permissionMode":"default"}'; }
tool_use() { echo "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"id\":\"$1\",\"name\":\"$2\",\"input\":$3}]}}"; }
tool_result() { echo "{\"type\":\"user\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"$1\",\"content\":\"$2\",\"is_error\":$3}]}}"; }
result() { echo "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"done\",\"session_id\":\"sess-1\",\"total_cost_usd\":0.001,\"duration_ms\":5,\"permission_denials\":$1}"; }
case "$FAKE_SCENARIO" in
  write)
    init
    tool_use t1 Write "{\"file_path\":\"$PWD/out.txt\",\"content\":\"hi\"}"
    echo hello > out.txt
    tool_result t1 ok false
    result '[]';;
  approve)
    init
    tool_use t1 Bash '{"command":"touch approved.txt"}'
    echo '{"type":"control_request","request_id":"req-1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"touch approved.txt"},"description":"Create a file"}}'
    read -r resp
    [ -n "$FAKE_STDIN" ] && echo "$resp" >> "$FAKE_STDIN"
    case "$resp" in
      *'"behavior":"allow"'*) touch approved.txt; tool_result t1 ok false; result '[]';;
      *) tool_result t1 denied true
         result '[{"tool_name":"Bash","tool_use_id":"t1","tool_input":{"command":"touch approved.txt"}}]';;
    esac;;
  plan)
    init
    echo '{"type":"control_request","request_id":"req-p","request":{"subtype":"can_use_tool","tool_name":"ExitPlanMode","input":{"plan":"# Plan\n1. do it"}}}'
    read -r resp
    [ -n "$FAKE_STDIN" ] && echo "$resp" >> "$FAKE_STDIN"
    result '[]';;
  hang)
    sleep 300 &
    [ -n "$FAKE_PIDFILE" ] && echo $! > "$FAKE_PIDFILE"
    init
    wait;;
  noinit)
    sleep 300 &
    wait;;
  fail) echo "boom" >&2; exit 3;;
  auth) echo "Invalid API key · Please run /login" >&2; exit 1;;
  *) init; result '[]';;
esac
"##;

pub(crate) fn install_fake(dir: &Path) -> PathBuf {
    let path = dir.join("claude");
    fs::write(&path, SCRIPT).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

pub(crate) fn spawn_env() -> Vec<(String, String)> {
    vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())]
}

/// A profile pointing at the fake; `env` entries become the fake's switches.
pub(crate) fn fake_profile(exe: &Path, scenario: &str, env: &[(&str, &str)]) -> AgentProfile {
    let var = |name: &str, value: &str| EnvVar {
        name: name.into(),
        value: value.into(),
    };
    AgentProfile {
        id: "p1".into(),
        name: "Fake".into(),
        provider: "claude".into(),
        executable: exe.display().to_string(),
        env: std::iter::once(var("FAKE_SCENARIO", scenario))
            .chain(env.iter().map(|(k, v)| var(k, v)))
            .collect(),
        model: None,
        inactivity_secs: 600,
        instructions: DEFAULT_INSTRUCTIONS.into(),
        is_default: true,
        enabled: true,
    }
}

pub(crate) type Events = Arc<Mutex<Vec<AgentEvent>>>;

pub(crate) fn collecting_sink() -> (EventSink, Events) {
    let events: Events = Arc::default();
    let sink_events = events.clone();
    let sink: EventSink = Arc::new(move |e| sink_events.lock().unwrap().push(e));
    (sink, events)
}

/// Polls until `pred` holds for the collected events (5 s limit).
pub(crate) async fn wait_for_event(events: &Events, pred: impl Fn(&AgentEvent) -> bool) {
    for _ in 0..250 {
        if events.lock().unwrap().iter().any(&pred) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("event never arrived: {:?}", events.lock().unwrap());
}

pub(crate) fn kinds(events: &[AgentEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| {
            serde_json::to_value(e).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}
