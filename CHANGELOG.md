# Changelog

What changed in AI App Studio, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

Nothing yet.

## [0.4.0] - 2026-11-10

Usage per AI agent profile, and scrolling that stays put at every window size.

### Added

- **Usage on Settings › AI Agents:** each saved profile shows its runs, input, output and cache tokens and Claude Code's cost estimate for today, the last 7 and 30 days and all time. Runs from before this version are counted too.

### Fixed

- **Scrolling:** Chat, Changes, Source and Environment Doctor no longer scroll away into empty space. The project header, the tabs and the prompt stay in place; in Chat only the conversation scrolls.
- The **⋮** menu on a project card opens upward when there is no room below, so **Remove from Studio…** is always reachable.
- **Small windows:** the prompt box in Chat starts at one line and grows as you type, so the conversation keeps more room. The file list on Source and the diff on Changes keep a usable height, and the page scrolls instead.
- In a narrow window the commit box on Source no longer covers the diff.
- The tools table in Environment Doctor fits the narrowest window without scrolling sideways.

## [0.3.0] - 2026-11-09

Clearer Settings that save as you go, release notes in the app, and a Windows export fix.

### Added

- **Release notes on About:** the notes for your version, with older versions folded away. After an update, Studio offers **What's new** once.
- **Settings › Tools** shows where each tool was found ("Found: /usr/bin/git · 2.47.1"), or that it's missing, and lets you set or clear your own path.

### Changed

- **Settings saves as you go:** each change is saved when you leave the field, with "Saved" next to the group; a value that isn't valid stays marked and isn't saved. Only AI Agents profiles keep a Save button, and leaving one with unsaved edits asks first.
- Settings has five tabs: General, AI Agents, Approvals, Limits and Tools. Preview, attachment and Database limits moved to **Limits**.
- **Reset to defaults** offers Undo.
- The finish-notification switch is stored with your other settings, so it no longer resets when the app's web storage is cleared. Your choice from 0.2.0 is kept.
- Better contrast for the selected sidebar item.

### Fixed

- **Windows export** no longer fails at "Installing dependencies" when the project uses a package published in the last day. The export installs exactly what your test build used.

## [0.2.0] - 2026-11-09

A smoother path from the first prompt to an installed app.

### Added

- **Conversations continue.** A new prompt in Chat continues the conversation with Claude, so follow-ups, plan corrections and **Make these changes** keep their context. **New conversation** above the composer starts fresh for the next run (with **Undo**). Each run says whether it continued the conversation or started a new one, and why. If a conversation can't be continued (for example after Claude Code's config folder changed), the run starts fresh by itself and says so.
- **Chat is one thread** of all the project's runs, oldest at the top, with day dividers. The last 20 runs show; **Show earlier runs** loads more. Older runs load as you scroll to them.
- **Jump to latest:** scrolled up while a run works, a pill tells you about new activity, in amber when an approval is waiting.
- **Start preview** on a finished run's last row, or "Preview updates by itself" when a preview already runs.
- **Build and install in one step.** The build dialog's "Install in GNOME when the test launch passes" (on by default, remembered per project) installs the release right after its test launch passes. It never installs a release whose test failed or that is older than the app's data.
- **A friendlier build dialog:** the next version is filled in, with minor and major bump links; release notes are prefilled from the runs since the last release; with uncommitted changes, **Commit first…** opens Source with the files ticked and brings you back to the dialog after the commit.
- **Finish notifications:** while Studio is in the background, a desktop notification says when a run finishes, a preview starts or a build ends. Clicking it opens that project. Turn them off in **Settings > General > Notifications**; approval notifications stay on.

### Changed

- Run labels lead with your prompt: "Add a dark theme · 14:32 · 3 files".
- Project tabs follow the loop: Chat · Plan · Changes · Preview · Source · Releases · GitHub · Database. The run picker is gone from Chat, which shows every run.

## [0.1.0] - 2026-11-08

The first public version. Everything below was built between 2 and 8 November 2026. Licensed under Apache-2.0; forks and redistributions must keep the `NOTICE` file that credits the original project.

### Added

**Help, About and ArtCraft (8 Nov)**
- **Help** and **About** pages in the sidebar. About shows the version, commit, build date and Tauri version, and copies them for a bug report.
- All fifteen ArtCraft apps can be used in agent runs: PhotoCraft, VectorCraft, SoundCraft, DeckCraft, CADCraft, GridCraft, TextureCraft, MaterialCraft, FilmCraft, EffectCraft, FigCraft, DesignCraft, LightCraft, PdfCraft and WordCraft.

**ArtCraft apps and project menu (7 Nov)**
- **ArtCraft apps:** with ArtCraft apps installed, Claude can make images, sprites, sounds and more for your app. Pick them with the ArtCraft button in the chat composer; the results are saved in the project's `assets/` folder. With Auto-approve on, Craft tools that can only write to `assets/` run without asking.
- The Environment Doctor lists which ArtCraft apps are installed.
- "Preparing <App> (first use)…" while an ArtCraft AppImage unpacks the first time.
- A **⋮** menu on project cards and in the project view, with **Remove from Studio** (your files stay).

**Project plans and exports (6 Nov)**
- **Plan tab:** describe the app, let Claude write a checklist plan, and keep it as `.ai-app-studio/plan.md` in the project. Start tasks one by one; Chat offers **Mark done** and **Start next**.
- **Exports for other systems:** build a release as an AppImage, `.deb` and `.rpm` for other Linux distributions, or as a Windows `setup.exe`, inside a local Docker container.

**App databases and icons (5 Nov)**
- New projects can store their data in **SQLite**, with numbered migrations; Claude is told the rules.
- **Database tab:** browse the Preview or installed app's data read-only, with backups, restore and reset.
- Releases protect the app's data: Install, Activate and Roll back back it up first.
- **App icons:** pick a picture for a project and Studio writes every icon size.

**Attachments, auto-approve and the Makefile (4 Nov)**
- **Chat attachments:** images, PDFs, text and code files and Word documents, by button, drag and drop or paste.
- **Auto-approve** for Make changes runs: edits, commands and reads run without asking, while risky actions (`sudo`, `git push`, deleting outside the project, reading `~/.ssh` and similar) still ask.
- A `Makefile` to build, install and run Studio's own AppImage (`make install`, `make run`, `make status`).

**GitHub (3–4 Nov)**
- Sign in with the GitHub CLI (the one-time code is shown in Studio), create a repository from a project, push, fetch, pull and merge, and clone your repositories into Studio.
- Merge conflicts open in the Source tab, with **Ask Claude to resolve**.

**Releases and installs (3 Nov)**
- **Releases tab:** build an AppImage of your app, test-launch it, and install it so it appears in the GNOME app grid. Activate, roll back and uninstall releases.
- **Installed Apps** view for every app Studio installed.
- Only one Studio window: a second launch focuses the open one.

**Source control and preview (2–3 Nov)**
- **Source tab:** commit selected files, create and switch branches, see history and restore files with your own Git.
- **Git checkpoints** before every agent run (`refs/studio/checkpoints/<run-id>`), without touching your branch.
- **Preview tab:** run the app with `tauri dev` on a free port, with build phases, the log and **Ask Claude to fix** on errors.

**Agent runs (2 Nov)**
- **Chat** with Claude Code in **Plan first** or **Make changes** mode, with live approval cards for every edit and command.
- **Changes tab:** see the diff of each run and revert the files you pick.
- Agent profiles in **Settings > AI Agents**, which can copy the environment of a `claude` shell alias.

**Foundations (2 Nov)**
- Create projects from a Tauri 2 + Vue 3 template, import existing folders, and relink moved ones.
- **Environment Doctor** that checks every tool Studio needs and says how to fix what's missing.

### Fixed

- Builds of Studio and of your apps no longer download the AppImage runtime from GitHub each time, so they keep working while GitHub is down (8 Nov).
- The sidebar stays in place and Help and About are always reachable at every window size; each view opens at its top (8 Nov).
- Exports: the real export passes; fixed bun, SELinux, orphan containers, old images and the cache timeout (6 Nov).
- GitHub: a rejected SSH key is no longer reported as a sign-in problem, and a failed fetch no longer says "Up to date" (4 Nov).
- Preview: no more white window on the first load, and AppImage paths no longer leak into child processes (3 Nov).
- Chat no longer gets stuck on "Working" after a run (3 Nov).

[Unreleased]: https://github.com/altafino/crazy/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/altafino/crazy/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/altafino/crazy/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/altafino/crazy/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/altafino/crazy/releases/tag/v0.1.0
