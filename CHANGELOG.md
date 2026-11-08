# Changelog

What changed in AI App Studio, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

Nothing yet.

## [0.1.0] - 2026-11-08

The first public version. Everything below was built between 2 and 8 November 2026.

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
