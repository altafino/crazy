# 0006: Milestone 4 engineering decisions

- **Status:** Accepted
- **Date:** 2026-11-03
- **Scope:** Milestone 4 (Build, Install, and Rollback) of [AI_App_Studio_PRD.md](../AI_App_Studio_PRD.md)

This file records why Milestone 4 looks the way it does. It is a record, not a spec: the implementation spec is PRD Section 11, "Milestone 4 — Build, Install, and Rollback". Where the two disagree, the PRD wins.

Decision IDs are prefixed `M4-`. The engineering review used M4-D1 to M4-D17; the design review uses M4-DR3 to M4-DR25 ([0007](0007-milestone-4-design-review.md)).

## Context

Milestone 3 shipped Source, previews as jobs, Git checkpoints and the `$APPDIR`-free child environment. M4 builds AppImages of generated Tauri apps, installs them as GNOME launchers, and switches between releases.

Facts that shaped the decisions:
- Tauri sets the GTK application ID only when `app.enableGTKAppId` is true (which also forces a single instance). Otherwise a window's Wayland app_id is its process name, the `mainBinaryName` (`tauri/src/app.rs`, `tao` `event_loop.rs`).
- AppImageLauncher on this machine intercepts AppImage launches (M1 finding).
- `~/.local/share/icons/hicolor/icon-theme.cache` exists on this machine.
- Studio's own AppImage is 108 MB; linuxdeploy is cached in `~/.cache/tauri`.
- On Arch, linuxdeploy's bundled `strip` can fail on `.relr.dyn` sections; newer Tauri bundlers need `xdg-open`.
- Adding a job kind needs a `jobs` table rebuild with foreign keys off (prior learning).

No outside voice: Codex was blocked by the host permission classifier, and the owner stopped the native fallback.

## Decisions

### Scope

| # | Decision | Why |
|---|---|---|
| M4-D1 | Restore source (F11) is deferred; builds still record their exact source. Tracked in `TODOS.md`. | The Definition of Done needs release rollback only. |
| M4-D2 | Two services: `services/builds` (BuildService, BuildAdapter, Tauri AppImage adapter) and `services/install` (installer owning activation and rollback, LauncherAdapter, XDG launcher). | A separate ReleaseService would split ownership of the active release. |

### Architecture

| # | Decision | Why |
|---|---|---|
| M4-D3 | Each build records HEAD commit, a dirty flag and a shadow snapshot fetched into the project repo as `refs/studio/builds/<release-id>`; dirty builds are allowed. | Exact source per release, reusing the M3 checkpoint path. |
| M4-D4 | The desktop entry sets `StartupWMClass=<mainBinaryName>`; install refuses a binary-name clash with another installed app; the spike confirms the window ID. | GNOME matches windows to launchers by app_id; works for imports too. |
| M4-D5 | The `current.AppImage` symlink is the only record of the active release; activation verifies the SHA-256, then renames a temp symlink over it. | A file and a row can't be updated atomically together. |
| M4-D6 | One copy per build, stored directly in `installed/<project-id>/releases/<version>/app.AppImage`; uninstall removes all release files. | Half the disk, no copy step at install. |
| M4-D7 | Keep the active release, the one activated before it, and the 5 newest; manual Delete for others. | Bounded disk (~700 MB per app) with several rollback targets. |
| M4-D8 | The version is chosen per build (next patch suggested, duplicates refused) and passed with `--config`; source files are never edited. | Correct release numbers without dirtying the tree. |
| M4-D9 | Builds always set `NO_STRIP=true`. | Removes a known Arch failure for a slightly larger file. |
| M4-D10 | An automatic 5-second test launch follows each build: Passed, Failed (install blocked) or Unverified (install after confirmation). | A release that crashes on start never reaches the launcher. |
| M4-D11 | Builds and Make-changes runs block each other. | A release must match its recorded source. |
| M4-D12 | The launcher icon is an absolute path inside the app folder, not the hicolor theme. | No icon cache involved. |
| M4-D13 | Launch Installed runs `gio launch <desktop file>`. | Tests the real launcher entry; the app outlives Studio. |

### Code quality

| # | Decision | Why |
|---|---|---|
| M4-D14 | Preview's process runner and log sink move to `services/jobs` and are shared with builds. | Identical stop and log behavior; about 90 lines saved. |
| M4-D15 | `preview/project.rs` moves to `services/projects/tauri.rs` with typed getters for the effective Tauri config. | Preview and build must read settings identically. |

### Tests

| # | Decision | Why |
|---|---|---|
| M4-D16 | The real AppImage build and install integration test runs in every `cargo test` (owner's choice over on-demand). | Packaging regressions are caught on the next test run. |
| M4-D17 | `TODOS.md` tracks Restore source. | The deferral stays visible. |

Required behavior carried from F09–F11 without separate questions:
- **Artifact check:** exactly one new, nonempty, type-2 AppImage after a zero exit.
- **Stored names:** Studio's own file names; strict semver for directories.
- **Writes:** temp file plus rename, with the desktop entry written last.
- **Ownership:** an ownership marker in each entry; foreign files are never touched.
- **Exec:** freedesktop escaping, with `APPIMAGELAUNCHER_DISABLE=1`.
- **Restart:** an interrupted build is reconciled at startup.
- **Doctor:** a new `xdg-open` row.
- **Paths:** `StudioPaths.applications_dir`.
- **Blocking work:** hashing and copying run off the async runtime.

## Consequences

- **Tasks:** T1–T10. Spike and shared moves first, then the schema, then the builds and install lanes in parallel, then commands, UI, the real build test and acceptance.
- **Slower tests:** `cargo test` now needs node, pnpm, Rust and the Tauri packaging tools, and takes several minutes longer (M4-D16).
- **Revisit:**
  - the `StartupWMClass` assumption if Tauri changes its default app ID;
  - `NO_STRIP` once linuxdeploy's strip handles `.relr.dyn`;
  - the retention numbers after real use.
