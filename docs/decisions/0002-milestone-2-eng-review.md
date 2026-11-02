# 0002: Milestone 2 engineering decisions

- **Status:** Accepted
- **Date:** 2026-11-02
- **Scope:** Milestone 2 (First Working Agent) of [AI_App_Studio_PRD.md](../AI_App_Studio_PRD.md)

This file records why Milestone 2 looks the way it does. It is a record, not a spec: the implementation spec is PRD Section 11, "Milestone 2 — First Working Agent". Where the two disagree, the PRD wins.

## Context

Milestone 1 shipped:
- project create and import;
- the environment doctor, which detects your shell's PATH and aliases;
- a process-group spawn helper inside the doctor;
- SQLite with `user_version` migrations;
- tauri-specta bindings.

Milestone 2 adds the first AI agent, the Claude Code CLI 2.1.316 on the target machine. Two facts shaped the decisions:
- **Undocumented protocol.** The CLI's stream-json event format and its host permission protocol have no official reference. Community sources disagree, and the protocol changed between recent versions.
- **Shell alias.** On the target machine, `claude` is a shell alias that sets `CLAUDE_CONFIG_DIR`, so Studio must pass that variable itself.

Codex reviewed the plan independently. Its findings are folded in below.

## Decisions

### Scope

| # | Decision | Why |
|---|---|---|
| D23 | Build a full per-file diff viewer in M2. | Owner's choice over a changed-file list (viewer deferred to M3). |
| D24 | Include "Revert this run" in M2. | Owner's choice over deferring to M3. |
| D25 | Keep `services/agents`, `services/jobs` and `services/git` as separate services. | M3 previews and commits, and M4 builds, reuse the job supervisor and git service. |
| D26 | Start with a CLI spike that records real transcripts as fixtures. | The formats are undocumented and change between versions; the recordings double as tests. |

### Architecture

| # | Decision | Why |
|---|---|---|
| D27 | Approve commands live in the chat via the CLI's host protocol. If it fails, fall back to non-interactive mode, keeping the requested mode. Never bypass permissions. | You approve in context, and a protocol change degrades safely instead of breaking or bypassing. |
| D28 | Before/after snapshots per run live in a private shadow git repo in Studio's data dir. | Diffs show only the run's changes even with your own work in progress; your repo is never touched; non-git projects work too. |
| D29 | Stream message-level typed events and replay the session log when a run is opened. | Few, meaningful updates that are testable with fixtures and survive navigation and restart. |
| D30 | `jobs.state` is the only run status. | One owner per fact; M3/M4 reuse the job states. |
| — | The AgentAdapter is stateless: `capabilities`, `probe`, and `start -> RunHandle`. | It hides spawn, parsing, approvals, cancel and resume behind three calls; the job supervisor alone owns which runs exist. |

### Code quality and limits

| # | Decision | Why |
|---|---|---|
| D31 | One shared process-group helper (`platform/process.rs`) for the doctor and the agent runner. | One tested place for orphan-process safety. |
| D32 | Pass Studio's default instructions with `--append-system-prompt`, editable per profile. | Never writes to your CLAUDE.md; same rules for imported projects. |
| D33 | Pre-fill the first Claude profile from the alias's env, saved only on confirmation. | Studio uses the same Claude account as your terminal from the first run. |
| D34 | Stop a run after 10 minutes without CLI events, not counting approval waits. | A hung CLI can't hold the project's single job slot forever. |
| D35 | Snapshots always skip build folders and files over 50 MB, and record what they skipped. | Bounded snapshot time and disk use on any imported project. |
| D36 | Store tool results up to 64 KiB and session logs up to 20 MB; control records are never truncated. | Bounded logs with visible cuts. |

### From the outside review (Codex)

| # | Decision | Why |
|---|---|---|
| D37 | Revert covers "all changes while the run was active", via a confirmation with per-file checkboxes. | Studio can't tell your saves during a run from the agent's; nothing is reverted without you seeing the list. |
| D38 | Snapshots never store secret-looking files (`.env`, keys, credentials). | Your credentials are never copied into Studio's data folder. |
| D39 | Keep snapshots for the last 20 runs per project; offer "Delete all snapshots". | Bounded disk use and no indefinitely kept old file versions. |
| D40 | Agent profiles refuse secret-looking env vars. | PRD:171: authentication stays with the CLI; Studio never stores secrets. |

These corrections follow from the decisions above and needed no separate approval:
- **Mode preserved:** the fallback keeps Plan mode read-only, and a run is blocked if neither transport works.
- **Allow and retry** permits exactly one tool rule, for one run.
- **Snapshot ordering:** BEFORE is written before spawn; interrupted runs get a manual revert pick and no AFTER.
- **Skipped paths** are never restored automatically.
- **Event delivery:** per-run sequence numbers with subscribe-then-replay deduplication.
- **Alias values:** `~` and `$HOME` are expanded, and the profile is probed with its exact environment.

## Consequences

- M2 is large. It has 13 tasks in three parallel backend lanes and three parallel UI lanes, with the spike first.
- Studio's data dir gains a snapshot store per project, bounded by D35, D38 and D39.
- Revisit:
  - D27's verified version range after each CLI update;
  - D35's and D38's skip lists if users report missing coverage;
  - D39's retention once M3 adds commits.
