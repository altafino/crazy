# 0001: Milestone 1 engineering decisions

- **Status:** Accepted
- **Date:** 2026-11-02
- **Scope:** Milestone 1 (Foundation) of [AI_App_Studio_PRD.md](../AI_App_Studio_PRD.md)

This file records why Milestone 1 looks the way it does. It is a record, not a spec: the implementation spec is PRD Section 11, "Milestone 1 — Foundation". Where the two disagree, the PRD wins.

## Context

The original Milestone 1 (scaffold, settings, SQLite, create/import, dashboard, environment doctor) was reviewed before any code existed. The review ran live checks on the target machine:

| Check | Result |
|---|---|
| Toolchain | git 2.56, cargo 1.99.1, node 26.8.1, pnpm 10.12, tauri-cli 2.11.4, webkit2gtk-4.1 2.52.6, Claude Code 2.1.316, gh 2.102 |
| Desktop | Manjaro, GNOME on Wayland, glibc 2.44 |
| GPU | NVIDIA RTX 4080 SUPER. Tauri's docs list blank windows and Wayland Error 71 for WebKitGTK on NVIDIA |
| GNOME session PATH | Lacks `~/.local/share/pnpm` |
| `claude` in the shell | An alias that sets `CLAUDE_CONFIG_DIR=~/.claude-a`. A process started from GNOME runs the binary with the default config, so a different account, with no error |

Codex (an independent model) also reviewed the plan. Its findings are folded into the decisions below.

## Decisions

### Scope

| # | Decision | Why |
|---|---|---|
| D2 | Do not define the AgentAdapter, BuildAdapter or LauncherAdapter interfaces in Milestone 1. They arrive in Milestones 2 and 4. | An interface written before its first implementation has the wrong shape. |
| D3 | Import is local-folder only. Clone from GitHub moves to Milestone 5. | Cloning needs gh auth and network error handling, which Milestone 5 builds. Until then, clone manually and import the folder. |
| D4 | Migration 0001 creates only `projects` and `app_settings`. | Other tables are designed when a milestone first uses them. |
| D5 | Use the full folder tree from PRD 6.3. | Owner's choice over flat modules. |
| D7 | Create Project copies a bundled `templates/tauri-vue` scaffold. | A new project builds right away (owner's choice over an empty workspace). |

### Architecture

| # | Decision | Why |
|---|---|---|
| D6 | Acceptance starts Studio's AppImage from GNOME Activities with the terminal closed. | Only a GUI launch shows the PATH, alias and NVIDIA problems. |
| D8 | `.ai-app-studio/project.json` owns project identity. The database is a cache of it. | Identity travels with the repo and survives database loss. |
| D9 | Tools are found through a background login-shell capture of PATH and aliases. That PATH is passed explicitly to every spawned process, and per-tool overrides win. | It is the only option that detects the `claude` alias. It also avoids `set_var` once threads are running. |
| D10 | The NVIDIA graphics workaround is applied only if the problem is observed, and only on NVIDIA. | Tauri advises against shipping the override unconditionally. |
| D11 | Use rusqlite with the `bundled` feature, in Rust only. | Keeps SQL behind typed commands (PRD: backend is the source of truth). |
| D12 | Use tauri-specta for generated TypeScript bindings, with a stale-bindings test. | One source of truth for command types (owner's choice over ts-rs). |
| D13 | Add Vitest for wizard and store tests. | Owner's choice over Rust-only tests. |
| D14 | Doctor probes time out after 5 s per tool and 10 s for the shell capture, and run in parallel. | Headroom for cold starts and heavy zsh setups. |
| D15 | project.json carries `"schema": 1`. | Lets later Studio versions handle old files safely. |

### From the outside review (Codex)

| # | Decision | Why |
|---|---|---|
| D16 | Create builds in a temp folder, renames it into place, then inserts the DB row. Copy-import inserts a pending row, replaces project.json atomically, then clears pending. Startup reconciles both. | If Studio is killed mid-way, the next start can recognize and repair the half-finished state. |
| D17 | When Studio runs as an AppImage, remove the launcher-added variables (measured, not guessed) before spawning host tools. | Inherited library variables can break git, node and pnpm only in the packaged app. |
| D18 | The template's `main.rs` gets the same graphics guard as Studio, and acceptance launches a generated app. | Generated apps are separate programs and would not get Studio's fix. |
| D19 | An imported copy gets a new project ID and a new app ID / bundle identifier. | Avoids two projects claiming one launcher and release history in Milestone 4. |
| D20 | Import detects template `tauri-vue`, or records `existing` with no build, package or install actions. | Cards offer only actions that can work. |
| D21 | Per-tool environment variables stay out of Milestone 1. The Milestone 2 AgentProfile owns agent environment. | One owner for agent env (PRD:167). The doctor only warns until then. |

Implementation mechanisms that follow from the PRD and the decisions above, without separate decisions:
- **Re-imports:** canonical-path comparison, so re-importing the same folder is a no-op.
- **Doctor processes:** each probe runs in its own process group. On timeout the group is killed and reaped. Output is capped at 64 KiB.
- **Commands:** async, with `spawn_blocking` for disk and DB work.
- **Frontend permissions:** no shell plugin. A test fails if one appears.

## Consequences

- Milestone 1 grows by a template, crash-safe ordering and a doctor that inspects the user's shell. In return, the main platform risks are tested in the first milestone instead of the fourth.
- The work order starts with a packaged spike: AppImage from GNOME, window rendering, the environment inside the AppImage, and a launched template app.
- Revisit:
  - D10 and D17 after the first spike produces evidence;
  - D21 when Milestone 2 designs AgentProfile;
  - D19 when Milestone 4 designs installation.

## Sources

- [Tauri: Linux graphics issues](https://v2.tauri.app/develop/debug/linux-graphics/)
- [fix-path-env-rs issue #14](https://github.com/tauri-apps/fix-path-env-rs/issues/14)
- [tokio `spawn_blocking`](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html)
- [AppImageKit: Creating AppImages](https://github.com/AppImage/AppImageKit/wiki/Creating-AppImages/cc2441518975caca23e9ce2dba6f08a22c678d1e)
