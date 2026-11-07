use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use tempfile::TempDir;

use super::testkit::*;
use super::*;
use crate::services::agents::{Decision, Denial, Outcome};

fn fixture_lines(name: &str) -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/claude")
        .join(format!("{name}.jsonl"));
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|v| v["dir"] == "out")
        .map(|v| v["msg"].to_string())
        .collect()
}

/// Feeds a recorded transcript through the run state like the read loop would.
fn replay(name: &str) -> (Vec<AgentEvent>, Vec<String>, RunOutcome) {
    let mut state = RunState::new("/project".into());
    let (mut events, mut stdin) = (Vec::new(), Vec::new());
    for line in fixture_lines(name) {
        let step = state.on_line(&line);
        events.extend(step.events);
        stdin.extend(step.stdin);
    }
    let (step, outcome) = state.finish(Some(0), "", Cause::Exited);
    events.extend(step.events);
    (events, stdin, outcome)
}

#[test]
fn every_fixture_parses_to_the_expected_events() {
    let table: &[(&str, &[&str])] = &[
        (
            "fallback_accept_edits_bash",
            &[
                "status",
                "toolCall",
                "toolResult",
                "text",
                "usage",
                "completed",
            ],
        ),
        (
            "fallback_accept_edits_write",
            &[
                "status",
                "toolCall",
                "fileChange",
                "toolResult",
                "text",
                "usage",
                "completed",
            ],
        ),
        (
            "host_bash_allow",
            &[
                "status",
                "toolCall",
                "approvalNeeded",
                "toolResult",
                "text",
                "usage",
                "completed",
            ],
        ),
        (
            "host_bash_deny",
            &[
                "status",
                "toolCall",
                "approvalNeeded",
                "toolResult",
                "text",
                "usage",
                "completed",
            ],
        ),
        (
            "host_edit_allow",
            &[
                "status",
                "toolCall",
                "approvalNeeded",
                "toolResult",
                "text",
                "usage",
                "completed",
            ],
        ),
        (
            "host_plan",
            &[
                "status",
                "toolCall",
                "toolResult",
                "toolCall",
                "fileChange",
                "toolResult",
                "toolCall",
                "planReady",
                "toolResult",
                "text",
                "usage",
                "completed",
            ],
        ),
        (
            "no_host_write_denied",
            &[
                "status",
                "toolCall",
                "toolResult",
                "toolCall",
                "fileChange",
                "toolResult",
                "text",
                "usage",
                "completed",
            ],
        ),
    ];
    for (name, want) in table {
        let (events, _, outcome) = replay(name);
        assert_eq!(kinds(&events), *want, "{name}");
        let seqs: Vec<u32> = events.iter().map(AgentEvent::seq).collect();
        assert_eq!(
            seqs,
            (0..events.len() as u32).collect::<Vec<_>>(),
            "{name}: seq"
        );
        assert_eq!(outcome.outcome, Outcome::Success, "{name}");
        assert!(outcome.provider_session_id.is_some(), "{name}");
    }
}

#[test]
fn denied_bash_is_an_error_result_with_a_retry_rule() {
    let (events, _, outcome) = replay("host_bash_deny");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::ToolResult { is_error: true, .. }))
    );
    let [Denial { tool, rule, .. }] = &outcome.denials[..] else {
        panic!("{:?}", outcome.denials)
    };
    assert_eq!(tool, "Bash");
    assert_eq!(
        rule.as_deref(),
        Some("Bash(touch denied.txt && ls -l denied.txt)")
    );
    let Some(AgentEvent::Completed { denials, .. }) = events.last() else {
        panic!()
    };
    assert_eq!(denials, &outcome.denials);
}

#[test]
fn plan_request_is_captured_and_denied_without_asking() {
    let (events, stdin, _) = replay("host_plan");
    let plan = events.iter().find_map(|e| match e {
        AgentEvent::PlanReady { plan, .. } => Some(plan),
        _ => None,
    });
    assert!(plan.unwrap().starts_with("# Plan: add README.md"));
    assert!(!kinds(&events).contains(&"approvalNeeded".to_owned()));
    let [reply] = &stdin[..] else {
        panic!("{stdin:?}")
    };
    let v: Value = serde_json::from_str(reply).unwrap();
    assert_eq!(v["response"]["response"]["behavior"], "deny");
    assert_eq!(v["response"]["response"]["message"], PLAN_DENY_MESSAGE);
    assert_eq!(
        v["response"]["request_id"],
        "2a48e465-36e5-41ec-a993-1fcb6ee61acf"
    );
}

#[test]
fn answers_are_written_as_the_fixture_shapes() {
    let mut state = RunState::new("/project".into());
    for line in fixture_lines("host_bash_allow") {
        state.on_line(&line);
    }
    let step = state.answer("65fc02df-a89a-4352-8481-ce6b3a8fe09d", &Decision::Allow);
    let sent: Value = serde_json::from_str(&step.stdin[0]).unwrap();
    let recorded: Value = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude/host_bash_allow.jsonl"),
    )
    .unwrap()
    .lines()
    .map(|l| serde_json::from_str::<Value>(l).unwrap())
    .find(|v| v["msg"]["type"] == "control_response")
    .unwrap()["msg"]
        .clone();
    assert_eq!(sent, recorded);

    let mut state = RunState::new("/project".into());
    for line in fixture_lines("host_bash_deny") {
        state.on_line(&line);
    }
    let step = state.answer(
        "0fb62b21-76cb-45aa-847b-b4eb004f9016",
        &Decision::Deny(None),
    );
    let sent: Value = serde_json::from_str(&step.stdin[0]).unwrap();
    assert_eq!(sent["response"]["response"]["message"], USER_DENY_MESSAGE);
    assert!(matches!(
        step.events[0],
        AgentEvent::ApprovalAnswered { allowed: false, .. }
    ));
    // Answering twice, or an unknown id, only warns.
    let again = state.answer("0fb62b21-76cb-45aa-847b-b4eb004f9016", &Decision::Allow);
    assert!(again.stdin.is_empty() && matches!(again.events[0], AgentEvent::Warning { .. }));
}

#[test]
fn auto_approve_answers_safe_requests_itself_and_asks_for_risky_ones() {
    let ask = |id: &str, cmd: &str| {
        json!({"type": "control_request", "request_id": id, "request": {"subtype": "can_use_tool",
            "tool_name": "Bash", "input": {"command": cmd}, "description": "d"}})
        .to_string()
    };
    let auto = Some(super::AutoApprove {
        ask_for_risky: true,
    });
    let mut state = RunState::new("/project".into()).with_auto(auto);

    let step = state.on_line(&ask("r1", "pnpm install"));
    let sent: Value = serde_json::from_str(&step.stdin[0]).unwrap();
    assert_eq!(sent["response"]["request_id"], "r1");
    assert_eq!(sent["response"]["response"]["behavior"], "allow");
    assert_eq!(
        sent["response"]["response"]["updatedInput"]["command"],
        "pnpm install"
    );
    assert!(
        matches!(&step.events[..], [AgentEvent::AutoAllowed { request_id, .. }] if request_id == "r1")
    );
    assert!(!state.has_pending());

    // Risky: the usual card, nothing written until the user answers.
    let step = state.on_line(&ask("r2", "git push"));
    assert!(step.stdin.is_empty());
    assert!(matches!(
        &step.events[..],
        [AgentEvent::ApprovalNeeded { .. }]
    ));
    assert!(state.has_pending());

    // Without auto-approve every request asks.
    let mut state = RunState::new("/project".into());
    let step = state.on_line(&ask("r3", "pnpm install"));
    assert!(
        step.stdin.is_empty() && matches!(&step.events[..], [AgentEvent::ApprovalNeeded { .. }])
    );
}

// ----------------------------------------------------------------- argv

fn request(mode: SessionMode) -> RunRequest {
    RunRequest {
        project_dir: "/project".into(),
        prompt: "SECRET PROMPT TEXT".into(),
        mode,
        resume_session_id: None,
        allowed_tools: Vec::new(),
        attachments_dir: None,
        attachments: Vec::new(),
        auto_approve: None,
        spawn_env: Vec::new(),
        pre_warnings: Vec::new(),
        mcp_servers: Vec::new(),
        log_path: "/tmp/unused.jsonl".into(),
    }
}

fn profile() -> AgentProfile {
    fake_profile(Path::new("claude"), "x", &[])
}

fn has_pair(argv: &[String], flag: &str, value: &str) -> bool {
    argv.windows(2).any(|w| w[0] == flag && w[1] == value)
}

#[test]
fn argv_for_every_mode_and_transport() {
    let live_plan = build_argv(&profile(), &request(SessionMode::Plan), Transport::Live);
    assert!(has_pair(&live_plan, "--permission-prompt-tool", "stdio"));
    assert!(has_pair(&live_plan, "--permission-mode", "plan"));
    assert!(has_pair(&live_plan, "--setting-sources", "project,local"));
    assert!(live_plan.contains(&"--strict-mcp-config".to_owned()));
    assert!(has_pair(
        &live_plan,
        "--append-system-prompt",
        DEFAULT_INSTRUCTIONS_FOR_TEST
    ));

    let live_changes = build_argv(&profile(), &request(SessionMode::Changes), Transport::Live);
    assert!(has_pair(&live_changes, "--permission-mode", "manual"));

    let fb_plan = build_argv(&profile(), &request(SessionMode::Plan), Transport::Fallback);
    assert!(has_pair(&fb_plan, "--permission-prompts", "none"));
    assert!(has_pair(&fb_plan, "--permission-mode", "plan"));
    assert!(!fb_plan.contains(&"--permission-prompt-tool".to_owned()));

    let fb_changes = build_argv(
        &profile(),
        &request(SessionMode::Changes),
        Transport::Fallback,
    );
    assert!(has_pair(&fb_changes, "--permission-prompts", "none"));
    assert!(
        !fb_changes.contains(&"--permission-mode".to_owned()),
        "CLI default mode"
    );
    for argv in [&live_plan, &live_changes, &fb_plan, &fb_changes] {
        assert!(
            !argv.contains(&"--mcp-config".to_owned()),
            "no servers: today's argv"
        );
    }
}

#[test]
fn argv_with_craft_servers_adds_mcp_config_and_craft_rules() {
    for t in [Transport::Live, Transport::Fallback] {
        let mut r = request(SessionMode::Changes);
        r.allowed_tools = vec!["Bash(touch a.txt)".into()];
        r.attachments_dir = Some("/home/u/x".into());
        r.mcp_servers = vec![crate::services::agents::McpServerSpec {
            name: "craft_photocraft".into(),
            command: "flatpak".into(),
            args: vec!["run".into(), "--command=photocraft-cli".into()],
        }];
        let argv = build_argv(&profile(), &r, t);

        assert!(argv.contains(&"--strict-mcp-config".to_owned()));
        let at = argv.iter().position(|a| a == "--mcp-config").unwrap();
        let config: Value = serde_json::from_str(&argv[at + 1]).unwrap();
        assert_eq!(
            config,
            json!({"mcpServers": {"craft_photocraft": {
                "type": "stdio",
                "command": "flatpak",
                "args": ["run", "--command=photocraft-cli"],
            }}})
        );
        assert!(argv[at + 2].starts_with("--"), "variadic flag is closed");
        let add = argv.iter().position(|a| a == "--add-dir").unwrap();
        let allowed = argv.iter().position(|a| a == "--allowedTools").unwrap();
        assert!(at < add && add < allowed);

        let i = argv
            .iter()
            .position(|a| a == "--append-system-prompt")
            .unwrap();
        assert!(argv[i + 1].starts_with(DEFAULT_INSTRUCTIONS_FOR_TEST));
        assert!(argv[i + 1].contains("PhotoCraft"), "{}", argv[i + 1]);
        assert!(argv[i + 1].contains("`assets/`"));
    }
}

const DEFAULT_INSTRUCTIONS_FOR_TEST: &str = crate::services::agents::profiles::DEFAULT_INSTRUCTIONS;

#[test]
fn argv_adds_model_resume_and_allowed_tools_last() {
    let mut p = profile();
    p.model = Some("opus".into());
    let mut r = request(SessionMode::Changes);
    r.resume_session_id = Some("sess-9".into());
    r.allowed_tools = vec!["Bash(touch a.txt)".into(), "Write(/p/b.txt)".into()];
    let argv = build_argv(&p, &r, Transport::Live);
    assert!(has_pair(&argv, "--model", "opus"));
    assert!(has_pair(&argv, "--resume", "sess-9"));
    let at = argv.iter().position(|a| a == "--allowedTools").unwrap();
    assert_eq!(&argv[at + 1..], ["Bash(touch a.txt)", "Write(/p/b.txt)"]);
}

#[test]
fn argv_without_attachments_has_no_add_dir_or_read_rule() {
    for t in [Transport::Live, Transport::Fallback] {
        let mut r = request(SessionMode::Changes);
        r.allowed_tools = vec!["Bash(touch a.txt)".into()];
        let argv = build_argv(&profile(), &r, t);
        assert!(!argv.contains(&"--add-dir".to_owned()));
        assert!(!argv.iter().any(|a| a.starts_with("Read(")));
    }
    assert_eq!(
        user_content(&request(SessionMode::Plan)),
        "SECRET PROMPT TEXT"
    );
}

#[test]
fn argv_with_attachments_adds_dir_before_allowed_tools_and_read_rule_last() {
    for t in [Transport::Live, Transport::Fallback] {
        let mut r = request(SessionMode::Changes);
        r.allowed_tools = vec!["Bash(touch a.txt)".into()];
        r.attachments_dir = Some("/home/u/x".into());
        r.attachments = vec!["/home/u/x/a.png".into(), "/home/u/x/b.pdf".into()];
        let argv = build_argv(&profile(), &r, t);
        let add = argv.iter().position(|a| a == "--add-dir").unwrap();
        assert_eq!(argv[add + 1], "/home/u/x");
        assert!(argv[add + 2].starts_with("--"));
        let at = argv.iter().position(|a| a == "--allowedTools").unwrap();
        assert!(add < at);
        assert_eq!(
            &argv[at + 1..],
            ["Bash(touch a.txt)", "Read(//home/u/x/**)"]
        );
        assert_eq!(
            user_content(&r),
            "SECRET PROMPT TEXT\n\nAttached files:\n/home/u/x/a.png\n/home/u/x/b.pdf"
        );
    }
}

#[test]
fn continue_plan_argv_keeps_the_plan_runs_folder_readable() {
    let mut r = request(SessionMode::Changes);
    r.resume_session_id = Some("sess-1".into());
    r.attachments_dir = Some("/home/u/x".into());
    let argv = build_argv(&profile(), &r, Transport::Live);
    assert!(has_pair(&argv, "--resume", "sess-1"));
    assert!(has_pair(&argv, "--add-dir", "/home/u/x"));
    assert_eq!(argv.last().unwrap(), "Read(//home/u/x/**)");
    assert_eq!(user_content(&r), "SECRET PROMPT TEXT");
}

#[test]
fn argv_never_bypasses_permissions_and_never_carries_the_prompt() {
    for mode in [SessionMode::Plan, SessionMode::Changes] {
        for t in [Transport::Live, Transport::Fallback] {
            let argv = build_argv(&profile(), &request(mode), t);
            let joined = argv.join(" ");
            assert!(!joined.contains("acceptEdits"), "{joined}");
            assert!(!joined.contains("dangerously-skip-permissions"), "{joined}");
            assert!(!joined.contains("bypassPermissions"), "{joined}");
            assert!(!joined.contains("SECRET PROMPT TEXT"), "{joined}");
        }
    }
}

// ------------------------------------------------------------- transport

#[test]
fn transport_table() {
    let live_help = "--permission-prompt-tool <t>\n--permission-prompts <m>";
    let fb_help = "--permission-prompts <m>";
    let cases: &[(Option<&str>, &str, Option<Transport>)] = &[
        (
            Some("2.1.316 (Claude Code)"),
            live_help,
            Some(Transport::Live),
        ),
        (Some("2.1.400"), live_help, Some(Transport::Live)),
        (Some("2.1.315"), live_help, Some(Transport::Fallback)),
        (Some("2.2.0"), live_help, Some(Transport::Fallback)),
        (Some("3.0.0"), live_help, Some(Transport::Fallback)),
        (None, live_help, Some(Transport::Fallback)),
        (Some("2.1.316"), fb_help, Some(Transport::Fallback)),
        (Some("2.1.316"), "--verbose", None),
        (Some("1.0.0"), "--permission-prompt-tool", None),
    ];
    for (version, help, want) in cases {
        let got = choose_transport(*version, help);
        assert_eq!(got.as_ref().ok().map(|(t, _)| *t), *want, "{version:?}");
        if let Ok((t, warning)) = got {
            assert_eq!(warning.is_some(), t == Transport::Fallback);
        }
    }
}

// ------------------------------------------------------- through the fake

struct Fx {
    dir: TempDir,
    exe: PathBuf,
    project: PathBuf,
}

fn fx() -> Fx {
    let dir = TempDir::new().unwrap();
    let exe = install_fake(dir.path());
    let project = dir.path().join("project");
    fs::create_dir(&project).unwrap();
    Fx { dir, exe, project }
}

impl Fx {
    fn request(&self, mode: SessionMode) -> RunRequest {
        RunRequest {
            project_dir: self.project.clone(),
            prompt: "do it".into(),
            mode,
            resume_session_id: None,
            allowed_tools: Vec::new(),
            attachments_dir: None,
            attachments: Vec::new(),
            auto_approve: None,
            spawn_env: spawn_env(),
            pre_warnings: Vec::new(),
            mcp_servers: Vec::new(),
            log_path: self.dir.path().join("logs/s1.jsonl"),
        }
    }

    fn file(&self, name: &str) -> String {
        self.dir.path().join(name).display().to_string()
    }
}

async fn run_to_end(
    fx: &Fx,
    scenario: &str,
    env: &[(&str, &str)],
) -> (RunOutcome, Vec<AgentEvent>) {
    let (sink, events) = collecting_sink();
    let profile = fake_profile(&fx.exe, scenario, env);
    let handle = ClaudeAdapter::default()
        .start(&profile, fx.request(SessionMode::Changes), sink)
        .await
        .unwrap();
    let outcome = handle.wait().await;
    let events = events.lock().unwrap().clone();
    (outcome, events)
}

#[tokio::test]
async fn full_run_logs_normalized_events() {
    let fx = fx();
    let (outcome, events) = run_to_end(&fx, "write", &[]).await;
    assert_eq!(outcome.outcome, Outcome::Success);
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(outcome.provider_session_id.as_deref(), Some("sess-1"));
    assert!(outcome.exited_normally);
    assert_eq!(
        kinds(&events),
        [
            "status",
            "toolCall",
            "fileChange",
            "toolResult",
            "usage",
            "completed"
        ]
    );
    assert!(matches!(&events[2], AgentEvent::FileChange { path, .. } if path == "out.txt"));
    assert_eq!(
        fs::read_to_string(fx.project.join("out.txt")).unwrap(),
        "hello\n"
    );

    let log = fs::read_to_string(fx.dir.path().join("logs/s1.jsonl")).unwrap();
    assert_eq!(log.lines().count(), events.len());
    let raw = fs::read_to_string(fx.dir.path().join("logs/s1.raw.jsonl")).unwrap();
    assert!(raw.lines().next().unwrap().contains("\"init\""));
}

#[tokio::test]
async fn prompt_goes_to_stdin_and_argv_matches_live_changes() {
    let fx = fx();
    let (stdin_log, argv_log) = (fx.file("stdin.txt"), fx.file("argv.txt"));
    run_to_end(
        &fx,
        "write",
        &[("FAKE_STDIN", &stdin_log), ("FAKE_ARGV", &argv_log)],
    )
    .await;
    let first: Value = serde_json::from_str(
        fs::read_to_string(stdin_log)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(first["message"]["content"], "do it");
    let argv = fs::read_to_string(argv_log).unwrap();
    assert!(argv.contains("--permission-prompt-tool\nstdio\n--permission-mode\nmanual"));
    assert!(!argv.contains("do it"));
}

#[tokio::test]
async fn approval_allow_round_trip() {
    let fx = fx();
    let stdin_log = fx.file("stdin.txt");
    let (sink, events) = collecting_sink();
    let profile = fake_profile(&fx.exe, "approve", &[("FAKE_STDIN", &stdin_log)]);
    let handle = ClaudeAdapter::default()
        .start(&profile, fx.request(SessionMode::Changes), sink)
        .await
        .unwrap();
    wait_for_event(&events, |e| matches!(e, AgentEvent::ApprovalNeeded { .. })).await;
    handle.answer_approval("req-1", Decision::Allow).unwrap();
    let outcome = handle.wait().await;
    assert_eq!(outcome.outcome, Outcome::Success);
    assert!(fx.project.join("approved.txt").exists());
    let events = events.lock().unwrap().clone();
    assert_eq!(
        kinds(&events),
        [
            "status",
            "toolCall",
            "approvalNeeded",
            "approvalAnswered",
            "toolResult",
            "usage",
            "completed"
        ]
    );
    let reply: Value = serde_json::from_str(
        fs::read_to_string(stdin_log)
            .unwrap()
            .lines()
            .nth(1)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(reply["response"]["response"]["behavior"], "allow");
    assert_eq!(
        reply["response"]["response"]["updatedInput"]["command"],
        "touch approved.txt"
    );
}

#[tokio::test]
async fn approval_deny_round_trip_reports_a_retry_rule() {
    let fx = fx();
    let (sink, events) = collecting_sink();
    let profile = fake_profile(&fx.exe, "approve", &[]);
    let handle = ClaudeAdapter::default()
        .start(&profile, fx.request(SessionMode::Changes), sink)
        .await
        .unwrap();
    wait_for_event(&events, |e| matches!(e, AgentEvent::ApprovalNeeded { .. })).await;
    handle
        .answer_approval("req-1", Decision::Deny(None))
        .unwrap();
    let outcome = handle.wait().await;
    assert!(!fx.project.join("approved.txt").exists());
    assert_eq!(
        outcome.denials[0].rule.as_deref(),
        Some("Bash(touch approved.txt)")
    );
    let events = events.lock().unwrap().clone();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::ApprovalAnswered { allowed: false, .. }))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::ToolResult { is_error: true, .. }))
    );
}

#[tokio::test]
async fn plan_run_captures_the_plan_and_denies_exit_plan_mode() {
    let fx = fx();
    let stdin_log = fx.file("stdin.txt");
    let (sink, events) = collecting_sink();
    let profile = fake_profile(&fx.exe, "plan", &[("FAKE_STDIN", &stdin_log)]);
    let handle = ClaudeAdapter::default()
        .start(&profile, fx.request(SessionMode::Plan), sink)
        .await
        .unwrap();
    assert_eq!(handle.wait().await.outcome, Outcome::Success);
    let events = events.lock().unwrap().clone();
    assert_eq!(
        kinds(&events),
        ["status", "planReady", "usage", "completed"]
    );
    assert!(matches!(&events[1], AgentEvent::PlanReady { plan, .. } if plan == "# Plan\n1. do it"));
    assert!(
        fs::read_to_string(stdin_log)
            .unwrap()
            .contains(PLAN_DENY_MESSAGE)
    );
}

#[tokio::test]
async fn old_cli_without_the_prompt_tool_runs_in_fallback_with_a_warning() {
    let fx = fx();
    let argv_log = fx.file("argv.txt");
    let (outcome, events) = run_to_end(
        &fx,
        "write",
        &[("FAKE_HELP", "fallback"), ("FAKE_ARGV", &argv_log)],
    )
    .await;
    assert_eq!(outcome.outcome, Outcome::Success);
    assert!(
        matches!(&events[0], AgentEvent::Warning { text, .. } if text.contains("without live approvals"))
    );
    let argv = fs::read_to_string(argv_log).unwrap();
    assert!(argv.contains("--permission-prompts\nnone"));
    assert!(!argv.contains("--permission-prompt-tool"));
}

#[tokio::test]
async fn unverified_version_falls_back_too() {
    let fx = fx();
    let (_, events) = run_to_end(&fx, "write", &[("FAKE_VERSION", "2.0.9")]).await;
    assert!(matches!(&events[0], AgentEvent::Warning { text, .. } if text.contains("2.0.9")));
}

#[tokio::test]
async fn blocked_when_neither_flag_exists() {
    let fx = fx();
    let (sink, _) = collecting_sink();
    let profile = fake_profile(&fx.exe, "write", &[("FAKE_HELP", "none")]);
    let err = ClaudeAdapter::default()
        .start(&profile, fx.request(SessionMode::Changes), sink)
        .await
        .err()
        .unwrap();
    assert!(matches!(err, StartError::IncompatibleCli(_)), "{err}");
    assert_eq!(err.outcome(), Outcome::IncompatibleCli);
}

#[tokio::test]
async fn missing_executable_is_not_installed() {
    let fx = fx();
    let (sink, _) = collecting_sink();
    let profile = fake_profile(&fx.dir.path().join("nope"), "write", &[]);
    let err = ClaudeAdapter::default()
        .start(&profile, fx.request(SessionMode::Changes), sink)
        .await
        .err()
        .unwrap();
    assert_eq!(err.outcome(), Outcome::NotInstalled);
}

#[tokio::test]
async fn failures_are_classified() {
    let fx = fx();
    let (outcome, events) = run_to_end(&fx, "fail", &[]).await;
    assert_eq!(outcome.outcome, Outcome::Failed);
    assert_eq!(outcome.exit_code, Some(3));
    assert!(outcome.message.unwrap().contains("boom"));
    assert_eq!(kinds(&events), ["error", "completed"]);

    let (outcome, _) = run_to_end(&fx, "auth", &[]).await;
    assert_eq!(outcome.outcome, Outcome::Unauthenticated);
}

#[tokio::test]
async fn silent_live_cli_fails_the_handshake() {
    let fx = fx();
    let (sink, _) = collecting_sink();
    let adapter = ClaudeAdapter {
        handshake: Duration::from_millis(300),
        ..ClaudeAdapter::default()
    };
    let handle = adapter
        .start(
            &fake_profile(&fx.exe, "noinit", &[]),
            fx.request(SessionMode::Changes),
            sink,
        )
        .await
        .unwrap();
    let outcome = handle.wait().await;
    assert_eq!(outcome.outcome, Outcome::IncompatibleCli);
    assert!(!outcome.exited_normally);
}

// ------------------------------------------------------- real CLI (T8, R11)

/// A one-page PDF whose text is `text`, with a correct xref table (no PDF crate needed).
fn minimal_pdf(text: &str) -> Vec<u8> {
    let stream = format!("BT /F1 24 Tf 72 700 Td ({text}) Tj ET");
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_string(),
        format!(
            "<< /Length {} >>\nstream\n{stream}\nendstream",
            stream.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{body}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
    for o in offsets {
        out.extend(format!("{o:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .bytes(),
    );
    out
}

/// Opt-in: `STUDIO_CLAUDE_E2E=1 cargo test claude_attachments_e2e -- --nocapture`. Runs the real
/// `claude` (`CLAUDE_BIN` or PATH; `CLAUDE_CONFIG_DIR` and the rest of the environment pass
/// through) in Plan mode with a text file and a PDF in an attachments folder, and spends credits.
/// Passes when the run succeeds, no approval was requested, and both code words come back.
#[tokio::test]
async fn claude_attachments_e2e() {
    if std::env::var("STUDIO_CLAUDE_E2E").as_deref() != Ok("1") {
        eprintln!("skipped: set STUDIO_CLAUDE_E2E=1 to run against the real Claude Code CLI");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("project");
    let attachments = tmp.path().join("attachments");
    fs::create_dir(&project).unwrap();
    fs::create_dir(&attachments).unwrap();
    let (txt, pdf) = (attachments.join("note.txt"), attachments.join("doc.pdf"));
    fs::write(&txt, "The code word in this text file is KUMQUAT-4417.\n").unwrap();
    fs::write(
        &pdf,
        minimal_pdf("The code word in this PDF is MARMALADE-9921"),
    )
    .unwrap();

    let exe = std::env::var("CLAUDE_BIN").unwrap_or_else(|_| "claude".into());
    let profile = fake_profile(Path::new(&exe), "unused", &[]);
    let profile = AgentProfile {
        env: Vec::new(), // the fake's FAKE_SCENARIO switch has no place here
        ..profile
    };
    let req = RunRequest {
        project_dir: project,
        prompt: "Read the attached files and reply with the code word of each, nothing else."
            .into(),
        mode: SessionMode::Plan,
        resume_session_id: None,
        allowed_tools: Vec::new(),
        attachments_dir: Some(attachments),
        attachments: vec![txt, pdf],
        auto_approve: None,
        spawn_env: crate::platform::process_env(),
        pre_warnings: Vec::new(),
        mcp_servers: Vec::new(),
        log_path: tmp.path().join("session.jsonl"),
    };
    let (sink, events) = collecting_sink();
    let handle = ClaudeAdapter::default()
        .start(&profile, req, sink)
        .await
        .expect("claude starts");

    // Deny anything that asks, and remember it: the test fails afterwards.
    let mut asked = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(180);
    let outcome = loop {
        let step = timeout(Duration::from_millis(250), handle.wait()).await;
        if let Ok(outcome) = step {
            break outcome;
        }
        let pending: Vec<_> = events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                AgentEvent::ApprovalNeeded {
                    request_id, tool, ..
                } => Some((request_id.clone(), tool.clone())),
                _ => None,
            })
            .filter(|(id, _)| !asked.iter().any(|(a, _)| a == id))
            .collect();
        for (id, tool) in pending {
            let _ = handle.answer_approval(&id, Decision::Deny(Some("e2e denies".into())));
            asked.push((id, tool));
        }
        if Instant::now() > deadline {
            handle.cancel(); // kills the process group
            let _ = handle.wait().await;
            panic!("timed out after 180 s; approvals asked: {asked:?}");
        }
    };

    let text: String = events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(outcome.outcome, Outcome::Success, "{:?}", outcome.message);
    assert!(asked.is_empty(), "approval requests: {asked:?}");
    assert!(text.contains("KUMQUAT-4417"), "text file not read: {text}");
    assert!(text.contains("MARMALADE-9921"), "PDF not read: {text}");
}

/// Opt-in, free: `STUDIO_CLAUDE_E2E=1 cargo test claude_hung_craft_server_e2e -- --nocapture`.
/// The real `claude` with an ArtCraft server that never answers and `MCP_TIMEOUT=10000`
/// (eng D10): init must still arrive inside the 30 s handshake, with a warning naming the app.
/// A model name that doesn't exist ends the run before any API usage.
#[tokio::test]
async fn claude_hung_craft_server_e2e() {
    if std::env::var("STUDIO_CLAUDE_E2E").as_deref() != Ok("1") {
        eprintln!("skipped: set STUDIO_CLAUDE_E2E=1 to run against the real Claude Code CLI");
        return;
    }
    let tmp = TempDir::new().unwrap();
    let exe = std::env::var("CLAUDE_BIN").unwrap_or_else(|_| "claude".into());
    let profile = AgentProfile {
        env: Vec::new(),
        model: Some("studio-probe-no-such-model".into()),
        ..fake_profile(Path::new(&exe), "unused", &[])
    };
    let mut spawn_env = crate::platform::process_env();
    spawn_env.push(("MCP_TIMEOUT".into(), "10000".into()));
    let req = RunRequest {
        project_dir: tmp.path().to_path_buf(),
        prompt: "Say OK".into(),
        mode: SessionMode::Plan,
        resume_session_id: None,
        allowed_tools: Vec::new(),
        attachments_dir: None,
        attachments: Vec::new(),
        auto_approve: None,
        spawn_env,
        pre_warnings: Vec::new(),
        mcp_servers: vec![crate::services::agents::McpServerSpec {
            name: "craft_soundcraft".into(),
            command: "sleep".into(),
            args: vec!["300".into()],
        }],
        log_path: tmp.path().join("session.jsonl"),
    };
    let (sink, events) = collecting_sink();
    let started = Instant::now();
    let handle = ClaudeAdapter::default()
        .start(&profile, req, sink)
        .await
        .expect("claude starts");
    let _ = timeout(Duration::from_secs(60), handle.wait())
        .await
        .expect("run ends");

    let events = events.lock().unwrap();
    assert!(
        events.iter().any(
            |e| matches!(e, AgentEvent::Status { text, .. } if text.starts_with("Session started"))
        ),
        "init arrived, the handshake limit did not end the run: {:?}",
        kinds(&events)
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::Warning { text, .. }
            if text == "SoundCraft didn't start, so Claude can't use it in this run.")),
        "{events:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[test]
fn database_rules_are_appended_only_for_projects_with_the_entry() {
    use crate::domain::{DatabaseEntry, ProjectMeta, TemplateId};
    let dir = tempfile::tempdir().unwrap();
    let mut meta = ProjectMeta::new("A", "a", TemplateId::TauriVue, "com.x.a");
    meta.write(dir.path()).unwrap();
    let mut r = request(SessionMode::Changes);
    r.project_dir = dir.path().to_path_buf();

    let without = build_argv(&profile(), &r, Transport::Live);
    assert!(has_pair(
        &without,
        "--append-system-prompt",
        DEFAULT_INSTRUCTIONS_FOR_TEST
    ));

    meta.database = Some(DatabaseEntry {
        engine: "sqlite".into(),
    });
    meta.write(dir.path()).unwrap();
    let with = build_argv(&profile(), &r, Transport::Live);
    let flags = with.iter().filter(|a| *a == "--append-system-prompt");
    assert_eq!(flags.count(), 1);
    let i = with
        .iter()
        .position(|a| a == "--append-system-prompt")
        .unwrap();
    assert!(with[i + 1].starts_with(DEFAULT_INSTRUCTIONS_FOR_TEST));
    assert!(with[i + 1].contains("src-tauri/migrations/NNNN_name.sql"));
    assert!(with[i + 1].contains("dev/app.db"));
}
