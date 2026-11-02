# 0004: Milestone 3 engineering decisions

- **Status:** Accepted
- **Date:** 2026-11-02
- **Scope:** Milestone 3 (Source and Preview) of [AI_App_Studio_PRD.md](../AI_App_Studio_PRD.md)

This file records why Milestone 3 looks the way it does. It is a record, not a spec: the implementation spec is PRD Section 11, "Milestone 3 — Source and Preview". Where the two disagree, the PRD wins.

Decision IDs are prefixed `M3-` because M1 and M2 already used D2–D62. The engineering review used M3-D1 to M3-D32; the design review continues at M3-D33 ([0005](0005-milestone-3-design-review.md)).

## Context

Milestone 2 shipped the Claude agent with live approvals, a private shadow-git snapshot store per project (before/after each run, diff, revert), the job supervisor (one agent run per project), and the process-group helper. M3 adds the user's own Git (status, diff, commit, branches, restore, history), Git checkpoints, and a preview runner for `tauri dev`, then tests change requests on an imported project.

Facts that shaped the decisions:
- Every generated project's Vite dev server uses port 1420 with `strictPort`, and so does Studio itself in dev mode.
- New projects have no `node_modules`; imported projects may use npm, yarn or bun.
- Migrations run inside a transaction with foreign keys on, so a table rebuild would cascade-delete agent sessions.
- `SnapshotStore::ensure()` writes `info/exclude` and `info/attributes` into whatever Git dir it gets.

Codex was blocked by the host permission classifier, so a native Claude subagent gave the independent review. Its findings are folded in below (M3-D22 onward).

## Decisions

### Scope

| # | Decision | Why |
|---|---|---|
| M3-D1 | Studio records a Git checkpoint in the project repo before each agent run, without touching your branch or committing dirty edits into history. The release-build checkpoint stays with M4. | Owner's choice over relying on the shadow snapshots alone. |
| M3-D2 | List, create and switch local branches. Switching is refused while the tree has uncommitted changes; no stash in M3. | Owner's choice over create-only. |
| M3-D3 | Module layout: `services/git/repo.rs`, `services/git/checkpoint.rs`, `services/preview/`, `commands/source.rs`, `commands/preview.rs`, a Source tab and a Preview tab with their own components and stores. Both Git runners stay. | Owner's choice over the smaller arrangement. |

### Architecture

| # | Decision | Why |
|---|---|---|
| M3-D4, M3-D22 | The checkpoint is a commit object at `refs/studio/checkpoints/<run_id>` in the project's (common) Git dir, created by fetching the shadow BEFORE snapshot: `git --git-dir=<project> fetch --no-tags <shadow> refs/studio/runs/<id>/before:refs/studio/checkpoints/<id>`. Keep 20; none for non-Git projects. | Same content as the snapshot, no second scan, and your `.git/info`, attributes, filters and hooks are never touched (D22 replaced D4's "shared capture"). |
| M3-D5 | If the checkpoint fails, the run still starts with a warning row; the shadow snapshot stays required. | Revert still works; a broken `.git` must not block agent work. |
| M3-D6, M3-D24 | Previews are jobs: migration 0003 lets `jobs.kind` be `'preview'`; JobSupervisor becomes generic over a job trait; history rows; startup marks running previews interrupted. | Owner kept one job system for every long-running process (reviewer suggested a standalone service). |
| M3-D7, M3-D32 | Each preview gets a free port via `tauri dev --config` (devUrl and `beforeDevCommand: pnpm dev --port N --strictPort`), only when the project's dev command is the template's `pnpm dev`. If the port is taken at start, retry once. | Port 1420 collides with other previews and with Studio in dev mode. |
| M3-D8, M3-D23 | Install dependencies before every preview, with the project's own package manager (lockfile, then `packageManager`, then pnpm). | New projects have no `node_modules`; pnpm would dirty or break npm/yarn/bun projects. |
| M3-D9, M3-D30 | While a Make-changes run is live, Restore, Switch branch and Initialize Git are refused (backend and UI). Commit and Create branch stay allowed; Plan-first runs lock nothing. | Only file-changing actions can fight the agent; one rule shared with the job system. |
| M3-D10 | Per project, at most one agent run and one preview, independently. | Watching the app reload while the agent works is the core loop. |
| M3-D11, M3-D31 | Restore safely = shadow snapshot first, then restore from HEAD; "Undo restore" until the next restore; secret/huge files marked "can't be undone"; keep 1 restore snapshot. | The only destructive Source action becomes undoable. |
| M3-D12 | If `user.name`/`user.email` are missing, an inline form saves them with `git config --local`. Global config is never written. | First commit works without leaving Studio. |
| M3-D13 | Commits run hooks and signing like a terminal, with a 2-minute limit and the hook output on failure. | Respects the repo's checks without hanging Studio. |
| M3-D14, M3-D29 | Preview lifecycle lives in `jobs.state`; a live phase (Installing, Compiling, Waiting for another Rust build, App running, Build failed) comes from known output lines, tested against recorded logs. | "Process running" doesn't mean "app running". |
| M3-D25 | Studio doesn't restore from checkpoints; the run's details show the ref and the README documents the Git command. | Revert is the one in-app undo for a run. |
| M3-D26 | Each project's real repo is resolved with `git rev-parse`; Source is scoped to the project folder; checkpoints go to the common dir; Initialize Git is refused inside another repo. | Monorepo folders, worktrees and submodules. |
| M3-D27 | Before a preview, check the doctor report for node, the package manager, cargo, rustc and webkit2gtk; stop early with the doctor's hint. | F08 toolchain diagnostics. |
| M3-D28 | A preview started during a Make-changes run skips its install and says so. | Two installs on one `node_modules` can corrupt it. |

### Code quality

| # | Decision | Why |
|---|---|---|
| M3-D15 | A held `index.lock` shows "Another Git program is using this repository" with Retry; Studio never deletes the lock. | Honest, and never risks another program's work. |
| M3-D16 | Commit commits exactly the ticked files (`git add -- <new>` then `git commit --only -- <paths>`); other staged entries stay staged, with a note. | What you see is what gets committed. |
| M3-D17 | The secret warning checks file names and a short list of key patterns in the added lines; it only warns, never blocks. | Catches keys pasted into ordinary source files. |
| M3-D18 | The template `.gitignore` ignores `.env` and `.env.*`, keeps `.env.example`. | PRD F03 requirement, exposed by the commit UI. |

### Tests and performance

| # | Decision | Why |
|---|---|---|
| M3-D19 | The migration runner turns foreign keys off around rebuild migrations (outside the transaction), uses create-copy-drop-rename, runs `foreign_key_check`, and fails the upgrade on violations. Proof: a populated v2 database migrates with every run and chat intact; the 13 supervisor tests keep their assertions. | A rebuild with foreign keys on deletes every agent session. |
| M3-D20 | Preview output: defaults 5,000 lines in memory, 10 MB log file per preview, 5 logs kept per project, configurable in Settings; UI updates batched every 100 ms. | Owner: "Default A, but configurable setting". |
| M3-D21 | Source status refreshes on open/focus, after a run, after each Source action and on Refresh; capped at 5,000 entries; 30 s timeout. | No background Git work while builds or agents write files. |

Corrections that keep approved behavior and needed no separate approval:
- **Commit failure** resets files Studio added (`git reset -q -- <added>`); a ticked rename passes both paths.
- **Identity check** reads `git config user.name/user.email`, not `git var` (which invents an identity).
- **Runners:** Source and checkpoints use a GitCmd-style runner on the project repo, never the shadow `GitRunner`.
- **Preview env:** previews spawn with the doctor's `spawn_env()`.
- **Dev command match:** the template's `pnpm dev` is matched in string or `{script}` form, after `tauri.linux.conf.json`.
- **Staged-only content:** the restore dialog says when staged content can't be undone.

## Consequences

- Tasks T1–T13 in five lanes: persistence → jobs → checkpoints → preview; Source backend; template; then commands and bindings; then the two UI tabs; then acceptance.
- `services/jobs` is refactored (M3-D24); M3-D19's regression contract guards it.
- Revisit:
  - the phase and lock-wait patterns (M3-D14, M3-D29) after each Tauri or cargo update;
  - the secret patterns (M3-D17) when new token formats appear;
  - the reviewer's remaining disagreements: a standalone PreviewService (M3-D24) and constant log limits (M3-D20).
