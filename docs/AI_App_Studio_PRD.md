# AI App Studio
## Product Requirements & Implementation Specification for an AI Coding Agent

**Status:** Implementation brief (v1.0)  
**Primary platform:** Manjaro Linux, GNOME desktop, Wayland and X11  
**Future platforms:** Other Linux distributions/desktops, then Windows and macOS  
**Product type:** Local-first desktop application  
**Working product name:** AI App Studio (replaceable)

---

## 1. Product Vision

Build a user-friendly desktop application where a person can **create, develop, test, package, install, launch, update, and version-control their own applications using AI coding agents**.

The user should describe an app in natural language, select a configured coding agent (initially Claude Code), let the agent create or modify real local source code, test the results, and publish a working desktop application. On Linux, **AppImage is the preferred packaging format**, especially for Tauri/Rust GUI applications.

A published app must appear in the standard **GNOME Applications overview/search** and run **without AI App Studio or its coding agents being open**. AI App Studio should also contain its own library of apps with Launch, Edit, Build, and Roll Back actions.

This is **not** a no-code visual builder or an online app generator. It is a local project manager, AI coding workbench, build orchestrator, and personal application installer, with a graphical interface that hides unnecessary command-line complexity while keeping the real source files accessible.

### 1.1 Core workflow

1. User clicks **New Project**.
2. User enters a name and a natural-language description, chooses an app template and a configured AI agent.
3. Studio creates a workspace, initializes Git, and invokes the agent to generate the application.
4. User reviews progress, source changes, and any required approvals.
5. User clicks **Run / Preview** to test the application.
6. User requests changes using additional prompts; the agent modifies the existing project.
7. User clicks **Build AppImage**, then **Install to Desktop**.
8. The app appears in the GNOME launcher and works independently of Studio.
9. User can later edit, commit, release a new version, roll back to a previous release, and sync with GitHub.

### 1.2 Product principles

- **Local-first:** sources, builds, installed apps, version history, and local agent-session metadata live on the machine by default.
- **Real projects:** generate ordinary editable repositories, not opaque proprietary project files.
- **Agent-agnostic:** integrate AI coding tools through replaceable adapters; do not hardwire the architecture to Claude Code.
- **Safe by default:** no silent destructive operations, unrestricted filesystem access, credential exposure, or automatic installation of unreviewed builds.
- **Minimal friction:** common tasks use explicit GUI actions instead of copy-and-pasted terminal commands.
- **Native integration:** honor Linux/XDG paths, desktop entries, icon themes, and GNOME light/dark preferences.
- **Cross-platform architecture:** isolate platform-specific launchers, paths, packagers, and environment checks behind interfaces.

---

## 2. Target Users and Use Cases

### Primary user

A technical but not necessarily full-time programmer who frequently builds practical tools with AI, especially desktop applications in Rust, Tauri, and web UI stacks.

### Main use cases

- Build a small desktop utility from a written idea.
- Create a Rust + Tauri + Vue desktop application using Claude Code.
- Continue development of an existing local or GitHub repository.
- Switch among supported AI agents on a project-by-project or session-by-session basis.
- Ask for features or bug fixes using conversational prompts.
- Preview changes and inspect file diffs before accepting them.
- Build, install, and launch stable releases from GNOME without opening the development workbench.
- Maintain multiple versions of an app and roll back if a new release is broken.
- Push code, branches, tags, and eventually packaged releases to GitHub.

---

## 3. Scope and Priorities

Definitions: **P0 = mandatory MVP**, **P1 = next milestone**, **P2 = later expansion**.

| ID | Feature | Priority |
|---|---|---|
| F01 | Dashboard and project library | P0 |
| F02 | Create and import local projects | P0 |
| F03 | Project templates: Rust + Tauri + Vue; existing repo | P0 |
| F04 | Configurable AI agent profiles and provider adapter interface | P0 |
| F05 | Claude Code CLI integration | P0 |
| F06 | Conversational change requests with live progress, logs, and cancellation | P0 |
| F07 | Git initialization, status, diff, commit, history, local branches | P0 |
| F08 | Preview/run generated applications | P0 |
| F09 | Build Tauri Linux AppImage with diagnostics | P0 |
| F10 | Install/launch/uninstall generated apps via GNOME/XDG | P0 |
| F11 | Release history and non-destructive installed-version rollback | P0 |
| F12 | GitHub authentication via gh and repository push/pull | P0 |
| F13 | Dependency doctor and first-run onboarding | P0 |
| F14 | Multiple CLI agent adapters / custom commands | P1 |
| F15 | AI-generated project plan and task checklist | P1 |
| F16 | Embedded file browser, editor, and diff viewer | P1 |
| F17 | Automated tests and quality gates before release | P1 |
| F18 | Portable Linux build profile using a compatible container/build environment | P1 |
| F19 | GitHub branches, pull requests, tags, and Releases | P1 |
| F20 | Terminal, configurable build recipes, export/import/backup | P1 |
| F21 | Multi-agent workflows, provider routing, usage/cost reporting (if available) | P2 |
| F22 | Other Linux desktops and formats; Windows/macOS adapters | P2 |
| F23 | Update notifications, optional release automation and CI | P2 |
| F24 | Optional isolation/sandbox policy for untrusted builds/agents | P1 |

The MVP must deliver an end-to-end **vertical slice**, not just a pretty project dashboard: new project -> agent generates source -> app runs -> AppImage builds -> GNOME launch entry works -> modified version is installed -> rollback works.

---

## 4. Functional Requirements

### F01 — Dashboard and App Library (P0)

**Purpose:** Manage all projects and installed apps from one screen.

Requirements:

- Display project cards/list with icon, name, description, tech stack, last edited date, selected agent, source status, and installed version.
- Show states such as **Draft**, **Generating**, **Ready to Preview**, **Build Failed**, **Built**, **Installed**, **Update Available**.
- Search, sort, and filter by type, tag, agent, and installed status.
- Per-project quick actions: **Open**, **Edit with AI**, **Run Preview**, **Launch Installed**, **Build**, **Install**, **Open Folder**, **Open GitHub**.
- Distinguish clearly between a **project** (editable source code) and an **installed app** (a runnable release artifact).
- Show recent jobs and actionable failures; recover after app restart.
- Support project archive/remove from library separately from deleting local source and separately from uninstalling published apps.

**Acceptance:** A project is discoverable after restarting Studio and an installed release can be launched from its card.

### F02 — Project Creation and Import (P0)

Creation wizard fields:

- Project name, slug, description / app idea, optional icon and category.
- Project type: **Desktop GUI**, **CLI tool**, or **Existing project** (MVP can publish only supported GUI project types).
- Template: **Rust + Tauri 2 + Vue 3 + TypeScript** as first-class default.
- Configurable project folder; sensible default in a user-writable Projects directory.
- Coding agent profile and optional model/profile options supported by that agent.
- Git initialization toggle (enabled by default), optional GitHub remote (private by default).
- Optional first prompt and optional detailed specification input.

Functional behavior:

- Validate names, paths, collisions, permissions, existing Git repositories, and platform prerequisites.
- Generate a stable UUID for the project; do not use mutable names as primary identifiers.
- Store project metadata in Studio's database; keep application source in a normal external folder.
- Create a human-readable project metadata file such as `.ai-app-studio/project.json` without storing credentials.
- Provide **Import from local folder** and **Clone from GitHub** actions.
- Imported projects must not be overwritten or reinitialized without explicit confirmation.

**Acceptance:** Creating or importing a project produces a persistent library entry and a usable source workspace.

### F03 — Project Templates (P0/P1)

MVP template:

- Tauri 2 desktop application with a Rust backend and Vue 3 / TypeScript frontend.
- Standard scripts for developer mode, tests, formatting, and AppImage build.
- App metadata: display name, bundle identifier, icon, initial version, and application category.
- Minimal starter UI and a README describing how to develop/build manually outside Studio.
- Template-generated `.gitignore` excludes build outputs, local secrets, and Studio-specific caches.

Future template catalog:

- Pure Rust GUI applications (only if supported packaging adapter exists).
- Rust CLI utilities (run as command-line tools, not misleadingly as GNOME desktop GUI apps).
- Tauri + React, Tauri + Svelte, Electron, Python Qt/PySide, and web-only apps.
- User-defined templates.

**Important:** Every project template must declare supported operations (preview, test, build, package, desktop install). A web-only project does not magically become an AppImage.

### F04 — AI Coding Agent Configuration (P0)

Settings > AI Agents:

- Agent name, type, executable/CLI path, enabled state, default status.
- CLI version detection, connection/authentication verification when supported, and diagnostic messages.
- Per-agent optional model, timeout, environment, execution profile, and extra allowed options.
- Per-project default agent and optional per-session override.
- Support one or multiple installed profiles for the same provider.
- Never require the user to paste an API key if an installed CLI already provides authenticated access.
- Store secrets only in secure OS credential storage or delegate authentication to the provider CLI; never in project metadata, logs, shell strings, or Git.
- Detect tools launched from GNOME whose PATH differs from the interactive shell; resolve approved executable paths explicitly.

Define a backend **AgentAdapter** interface that supports:

- `detect()`, `get_version()`, `check_auth()` (best effort)
- `start_session(project, prompt, options)`
- `stream_events(session_id)`
- `cancel(session_id)`
- `resume_session(session_id, prompt)` (only when supported)
- `supported_capabilities()`

Normalize events into `status`, `text`, `tool_call`, `file_change`, `approval_needed`, `usage`, `warning`, `error`, and `completed`, without pretending all providers expose identical telemetry.

### F05 — Claude Code Integration (P0)

- Support a user-installed, authenticated **Claude Code CLI** as first adapter.
- Start commands with the project directory as working directory, using process argument arrays and stdin where appropriate; **do not build executable shell command strings** from user prompts.
- Use the CLI's supported non-interactive/structured output mode for events when practical (e.g., `claude -p` and `--output-format stream-json`), and parse output defensively.
- Preserve prompts, agent output, timestamps, exit status, and provider session identifiers when exposed.
- Surface permission requests and blocked operations. Do not invoke `--dangerously-skip-permissions` as a shortcut.
- If the CLI cannot complete a permission-sensitive action non-interactively, provide a safe workflow to approve or use a user-visible interactive session; do not silently bypass permissions.
- Include a configurable default instruction file, such as the project `CLAUDE.md`, but do not overwrite existing user instructions without consent.
- Show clear errors for not installed, unauthenticated, incompatible CLI version, expired login, missing dependencies, timeout, and cancelled process.

**Acceptance:** A prompt can create/modify real files in the chosen project; output streams to the UI; cancellation stops the spawned process tree where practical; the session history remains visible after restart.

### F06 — AI Development Workspace (P0/P1)

Primary project view includes tabs or panels:

1. **Overview:** description, latest status, shortcuts, last installed release.
2. **AI Chat:** prompt history, input composer, agent selector, structured live output.
3. **Changes:** Git working-tree status and before/after diffs.
4. **Run & Build:** preview lifecycle, tests, packaging job queue, logs.
5. **Versions:** source commits and separately installed releases.
6. **GitHub:** repository, branch, sync state, links, errors.
7. **Settings:** project template, launch behavior, build recipe, permissions.

Actions:

- Prompt examples: "Add a settings page", "Fix this error", "Replace the sidebar", "Write tests for this feature".
- Offer a **Plan first** mode and a **Make changes** mode; do not imply that every provider supports native planning semantics.
- Display actual file modifications, not merely a textual AI claim that work is done.
- Before an agent run, record a recoverable source checkpoint (Git commit, separate worktree, or safe snapshot); do not automatically commit dirty user changes without explicit approval.
- Allow **Review diff**, **Keep changes**, **Revert this run** where safe, and **Retry**.
- Use a bounded concurrent job queue (MVP: one modifying agent job per project).
- Show progress, stderr/stdout, last activity, exit result, and cancellation.
- Never claim success until the process exits successfully and resulting project state is checked.
- If the agent crashes mid-edit, retain recoverable changes and present a recovery workflow.

### F07 — Source Version Control / Git (P0)

- Initialize Git for new projects; detect Git in imported projects.
- Show tracked/untracked/modified/staged files, branch, last commit, and history.
- Provide GUI **Review diff**, **Stage selected**, **Commit**, **Create branch**, and **Restore safely**.
- Use Git commits for source history, not AppImage release numbers.
- Support project-level checkpoints before agent changes and before release builds.
- Clearly distinguish (a) local source state, (b) Git commit/tag, and (c) installed binary release.
- Warn if source files contain likely secrets before staging/committing, without claiming exhaustive detection.
- Handle merge conflicts visibly; never overwrite conflicting local edits automatically.

### F08 — Preview / Development Runner (P0)

- Start the template's defined development command (e.g., Tauri dev), capture logs, and report its process state.
- Support Stop and Restart; avoid detached/orphan processes during cancellation or Studio shutdown.
- Show error diagnostics for missing toolchain, compiler issues, ports, or runtime dependencies.
- The real application should open as its own GUI window when the template supports this.
- Do not confuse developer preview with an installed stable release.
- Optional P1: separate app log viewer, process inspection, and screenshot capture for bug-report prompts.

### F09 — Build and AppImage Packaging (P0)

- Build operations must use a registered **BuildAdapter** associated with the project template.
- For Tauri 2 on Linux, generate an AppImage via the supported Tauri bundling workflow.
- First-run diagnostics check for `git`, Rust/Cargo, relevant Node/package tooling, Tauri CLI, WebKitGTK build dependencies, and AppImage/runtime prerequisites; report missing components with Manjaro-compatible guidance.
- Preserve readable build logs, full exit status, output location, duration, and diagnostics.
- Treat a build as successful only when the packaging command exits successfully **and an expected, nonempty artifact actually exists**.
- Store artifact SHA-256, app ID, architecture, version, Git commit, build timestamp, build profile, and source-dirty flag.
- Separate **Local Build** (optimized for current Manjaro machine) from a later **Portable Build** profile (container or CI based on an older supported Linux baseline).
- Do not claim that a Manjaro-built AppImage will run on all Linux distributions. AppImage portability can be constrained by glibc and system dependencies.
- Run a smoke test where practical before allowing publication. If skipped, clearly label as unverified.
- MVP only promises AppImage for the supported Tauri GUI template; unsupported project formats get a clear "No AppImage builder configured" message.

### F10 — GNOME Application Launcher Integration (P0)

**Critical requirement:** A generated app must launch independently from the GNOME application grid/search, after closing Studio and without starting Claude Code.

Linux installer behavior:

- Copy a successfully built, approved AppImage to a **Studio-managed, user-owned release directory**, preserving prior versions.
- Set executable permission on the installed artifact.
- Maintain a **stable launch target** for the currently active release (e.g., a `current.AppImage` symlink updated atomically).
- Create/update an XDG-compatible `.desktop` entry under `${XDG_DATA_HOME:-~/.local/share}/applications/` with stable filename, display name, appropriate `Exec`, icon, categories, and `Terminal=false` for GUI apps.
- Store/install a matching application icon in a suitable user-owned XDG icon location.
- Follow the freedesktop Desktop Entry Specification for path escaping and quoted `Exec` values; never insert raw unescaped user inputs into launcher commands.
- Check for launcher/application-ID conflicts rather than overwriting unrelated files.
- Show **Install to Desktop**, **Launch Installed**, **Show in Files**, and **Uninstall from Desktop**.
- Uninstall removes only Studio-owned desktop entries/icons and installed release files for the selected app. It must not delete the source project without separate confirmation.
- Ensure an installed app still works after Studio is terminated. The `.desktop` launcher must not depend on the Studio process.
- Support new release activation without changing the stable application launcher identity.
- Show a helpful message if GNOME has not yet refreshed its application listing.

Illustrative layout (paths are conceptual, resolved via XDG APIs):

```text
$XDG_DATA_HOME/ai-app-studio/
  database.sqlite
  installed/
    <project-uuid>/
      current.AppImage -> releases/1.2.0/MyTool.AppImage
      releases/
        1.0.0/MyTool.AppImage
        1.1.0/MyTool.AppImage
        1.2.0/MyTool.AppImage
$XDG_DATA_HOME/applications/
  ai-app-studio-generated-<project-uuid>.desktop
$XDG_DATA_HOME/icons/hicolor/256x256/apps/
  ai-app-studio-generated-<project-uuid>.png
```

Requirements for stable symlinks: update the current pointer atomically where supported; verify the target before switching; retain the previous pointer and restore it if activation fails. Protect against conflicting versions, malformed user paths, and directory traversal.

**Acceptance:** Close Studio, open GNOME Activities, search for the app name, start the app, and confirm it functions. Repeat after installing version 1.1.0 and after rolling back to 1.0.0.

### F11 — Application Release History, Upgrade, Rollback (P0)

- Every successfully packaged app can be recorded as a release with version string, SHA-256, Git commit, build profile, install date, and notes.
- Support monotonically suggested semantic versions, but allow explicit version labels with validation.
- Show a table of built, installed, active, and retired releases.
- **Activate release** switches the stable launcher target only after validating the artifact.
- **Rollback installed app** switches the active executable to a prior retained release; it must not silently reset or delete source code.
- **Restore source** is a separate, explicitly confirmed Git operation.
- Retain at least the current and previous installed releases by default; configurable cleanup later.
- Warn if replacing a release while its process is running; use a safe activation strategy rather than overwriting the running file.
- A failed update must leave the previous release launchable.

### F12 — GitHub Integration (P0/P1)

MVP:

- Use an existing authenticated **GitHub CLI (`gh`)** where available; support `gh auth status` and user-initiated `gh auth login` flow.
- Support importing/cloning a remote GitHub repository.
- Offer **Create GitHub Repository** for a local project; default to **private** and require explicit confirmation for public visibility.
- Show remote, branch, ahead/behind status, and last sync result.
- Provide user-initiated **Push**, **Fetch**, and **Pull**, with clear descriptions of their effects.
- If divergence/merge conflict occurs, do not force-push or reset automatically.
- Source changes must never be pushed without a user action/approved workflow.

P1:

- Create/publish Git tags and GitHub Releases.
- Attach built AppImages and checksums to a GitHub Release with explicit user approval.
- Create branches and pull requests; show CI/Actions results if authenticated.
- Link issues and generate changelog/release notes from commits.

**Acceptance:** A local project can be created as a private GitHub repo, committed, pushed, reopened in the application, fetched, and pulled, with useful error messages for missing auth or conflicts.

### F13 — First-Run Onboarding and Environment Doctor (P0)

- Detect platform and desktop environment; explicitly support Manjaro GNOME in the first release.
- Inspect required tools and provider CLIs, showing **Installed / Missing / Misconfigured** status.
- Explain that AI provider authentication and network access are managed by configured provider tools, while local editing/building/installing can remain offline after prerequisites are available.
- Provide install/help instructions for missing dependencies, but do not run system-wide `sudo` commands silently.
- Offer a sample project and an end-to-end verification flow.
- Support manual executable-path overrides when tools are outside the GNOME process PATH.

---

## 5. User Interface and UX

Use a clean desktop layout with GNOME-friendly keyboard and window behavior, automatic light/dark theme, straightforward navigation, accessible labels, keyboard shortcuts, and clear job indicators.

### Main navigation

```text
AI APP STUDIO
├── Dashboard / Projects
├── Installed Apps
├── Recent Jobs
├── AI Agents
├── Settings & Environment Doctor
└── Help / Logs

PROJECT DETAIL
├── Overview
├── AI Workspace
├── Changes
├── Run & Build
├── Releases
├── Git / GitHub
└── Project Settings
```

### Essential UI states

- Empty state with a primary **Create Your First App** action.
- Progress view with live agent output, current file/operation if available, Cancel button, and relevant permission alerts.
- Build view showing progress, warnings, failures, resulting AppImage, checksum, and publish action.
- Installed release status with **Launch**, **Edit**, **Rebuild**, and **Roll Back**.
- Detailed, actionable errors: e.g. "Claude Code CLI not found", "GitHub authentication required", or "AppImage created but smoke test failed".
- Every potentially destructive operation must distinguish whether it affects source, Git history, installed binaries, or all three.

### Optional P1 UX improvements

- Rich text / markdown rendering in AI chat, searchable history.
- Built-in code editor and side-by-side diffs.
- Drag/drop local folders, import existing GitHub URLs.
- Templates gallery and per-project prompts/instructions.
- User-defined theme and project icons.

---

## 6. Recommended Technical Architecture

### 6.1 Stack

- **Desktop shell:** Tauri 2.
- **Backend:** Rust.
- **Frontend:** Vue 3 + TypeScript + Vite + Tailwind CSS 4 (or equivalent accessible component system).
- **Local database:** SQLite with versioned migrations.
- **Source control:** native Git executable through a typed Rust service; GitHub operations via `gh` CLI initially.
- **Agent runtime:** Rust process supervisor with per-agent adapters and streamed events.
- **Packaging:** template-specific build adapters; first implementation targets Tauri 2 AppImage.
- **State management:** typed frontend store (e.g. Pinia) with backend as source of truth for jobs, releases, and paths.
- **Logs:** structured, bounded local logs with secret redaction.

### 6.2 Backend services and interfaces

```text
Tauri UI (Vue)
   |
   +-- typed Tauri commands / event streams
         |
         +-- ProjectService
         +-- AgentService ---- AgentAdapter (ClaudeCodeCliAdapter, future adapters)
         +-- JobSupervisor
         +-- GitService
         +-- GitHubService
         +-- BuildService ---- BuildAdapter (TauriAppImageAdapter, future adapters)
         +-- InstallService -- PlatformLauncherAdapter (LinuxXdgLauncher, future adapters)
         +-- ReleaseService
         +-- DiagnosticsService
         +-- SettingsService
         +-- Persistence (SQLite + filesystem)
```

Important boundaries:

- The frontend must **not** gain unrestricted arbitrary-shell execution permissions.
- Rust services execute only defined operations with validated arguments, paths, environment, and project scopes.
- Agent adapters, build adapters, platform installers, and source-control adapters must be independently testable.
- Long-running processes run asynchronously; no UI blocking. Expose structured progress/events through Tauri.
- Use cancellation and subprocess/process-group cleanup as appropriate to Linux.
- Capture and redact logs; apply retention limits; distinguish provider output from verified filesystem or process state.
- Project scanning or code indexing must be bounded and avoid irrelevant `.git`, `node_modules`, `target`, build, and secrets directories.

### 6.3 Suggested repository structure

```text
ai-app-studio/
  README.md
  docs/
    architecture.md
    decisions/
  src/                         # Vue frontend
    components/
    views/
    stores/
    services/
  src-tauri/
    src/
      commands/
      domain/
      services/
        agents/
        builds/
        launchers/
        git/
        github/
        jobs/
        projects/
        releases/
      persistence/
      platform/
      security/
    migrations/
    capabilities/
  templates/
    tauri-vue/
  tests/
    integration/
  scripts/
  AGENTS.md
```

Use the actual conventions of the chosen Tauri/Vue scaffolding if they differ; keep the service boundaries.

### 6.4 Core data model

Suggested persistent entities:

| Entity | Minimum fields |
|---|---|
| `Project` | `id`, `name`, `slug`, `description`, `workspace_path`, `template_id`, `created_at`, `updated_at`, `default_agent_id`, `app_id`, `archived` |
| `AgentProfile` | `id`, `provider`, `name`, `executable_path`, `options_json`, `is_default`, `enabled` |
| `AgentSession` | `id`, `job_id`, `project_id`, `agent_id`, `mode`, `provider_session_id`, `prompt`, `log_path`, `before_ref`, `after_ref` (no status: the job owns it, see Milestone 2) |
| `Job` | `id`, `project_id`, `type`, `state`, `started_at`, `completed_at`, `exit_code`, `log_path`, `error_message` |
| `BuildArtifact` | `id`, `project_id`, `version`, `source_commit`, `dirty_source`, `build_profile`, `architecture`, `path`, `sha256`, `created_at`, `verified` |
| `InstalledRelease` | `id`, `project_id`, `artifact_id`, `install_path`, `installed_at`, `is_active` |
| `GitRemote` | `project_id`, `name`, `url`, `branch` |
| `AppSettings` | XDG paths, theme, retention, preferred app/build configuration |

Job states: `queued`, `running`, `waiting_for_approval`, `succeeded`, `failed`, `cancelled`, `interrupted`.

Use database migrations and foreign keys, and handle missing or moved workspaces gracefully. Project metadata must be recoverable by rescan/import where possible. Database is a metadata index, not a replacement for Git.

### 6.5 File locations

- Source projects: user-selected folder, with a convenient default such as `~/Projects/AIAppStudio/<slug>`.
- Studio metadata/installed releases: `${XDG_DATA_HOME:-~/.local/share}/ai-app-studio/`.
- Cache/build staging: `${XDG_CACHE_HOME:-~/.cache}/ai-app-studio/` when appropriate, while respecting template build directories.
- Configuration: `${XDG_CONFIG_HOME:-~/.config}/ai-app-studio/`.
- Launcher entries: `${XDG_DATA_HOME:-~/.local/share}/applications/`.
- Icons: relevant XDG icon theme directories.

Do not hardcode `/home/<username>` or assume the shell's `HOME` is always a reliable desktop application installation location.

---

## 7. Security and Reliability Requirements

AI coding agents can edit files, invoke tools, install packages, and execute code. The app must be designed around that fact.

1. **User approval:** Obtain explicit permission before destructive filesystem operations, installing software, changing system configurations, deleting source, replacing releases, pushing to GitHub, or publishing a public repository.
2. **Scoped workspaces:** Default agent operations to the selected project directory. Never give an agent unrestricted home-directory scope by default. Clearly indicate limitations of provider-enforced scope.
3. **No implicit sandbox claims:** A working-directory setting is not an OS sandbox. If stricter isolation is offered, implement and test it using suitable platform mechanisms.
4. **No blanket permission bypass:** Avoid unrestricted auto-approval modes and unsafe CLI flags.
5. **Secrets:** Delegate authentication to agent/GitHub CLIs or credential storage; redact tokens from logs and prevent accidental Git commits of common secret files.
6. **Safe process execution:** Use argument arrays, executable allowlists, controlled environments, working-directory validation, robust process termination, and correct quoting for launcher entries.
7. **Git safety:** Check dirty trees before agent runs, restore operations, pulls, and branch switches. Avoid destructive resets and force pushes without specific user consent.
8. **Build safety:** Keep build outputs separate from the source, validate artifact type/size/hash, use atomic publish/activation, and preserve last known working version.
9. **UI trust boundary:** Treat agent and project-generated output as untrusted text; escape/render safely rather than injecting raw HTML.
10. **Recovery:** Track interrupted operations and leave source plus previous installed releases intact after crashes or power loss.
11. **Local privacy:** No mandatory account, telemetry, cloud sync, or uploading project files except via the agent/GitHub services intentionally used by the user.
12. **Updates:** Do not auto-run AI-generated code as a privileged user. No root access should be required for normal per-user app installation.

---

## 8. Cross-Platform Strategy

Implement Linux/XDG first, but define interfaces now:

- `PlatformPaths`: config, cache, projects, application metadata, installation locations.
- `LauncherAdapter`: install/update/uninstall/launch desktop app entry.
- `PackageAdapter`: create and validate platform-specific artifact.
- `ToolchainDoctor`: OS-specific dependency requirements.

First target: **Manjaro GNOME**. Validate on the actual machine, including GNOME search/launcher behavior and installed app startup.

Future Linux: KDE Plasma and other XDG-compatible desktops, distributions with different package managers, and potentially Flatpak/deb/rpm packaging.

Future Windows: `.exe` / `.msi` installer and Start Menu integration.

Future macOS: `.app` / `.dmg`, Launchpad/Finder integration, signing/notarization considerations.

**AppImage is Linux-only.** Do not describe it as a cross-platform binary format. Also, building portable AppImages on a very recent rolling-release distribution can lead to runtime library/glibc compatibility problems on older Linux systems; P1 should offer builds from a supported older Linux baseline in a container or CI.

---

## 9. Main Workflows and Acceptance Scenarios

### Scenario A — New app in natural language (critical MVP)

**Given** an installed and authenticated Claude Code CLI and a functional Rust/Tauri toolchain:

1. Open Studio and select New Project.
2. Name it `QuickNotes` and enter: "Build a minimal notes app with a list, an editor, and local storage."
3. Select Tauri/Vue template and Claude Code.
4. Start generation; see streaming steps and resulting source changes.
5. Run preview; a separate working QuickNotes window appears.
6. Build AppImage; output is verified and listed as a build artifact.
7. Install to desktop; a QuickNotes entry appears in GNOME search.
8. Fully exit Studio; launch QuickNotes from GNOME, with no agent running.

**Pass condition:** All eight steps work on Manjaro GNOME without manually writing or editing launcher files.

### Scenario B — Prompt-driven change and new release (critical MVP)

1. Open QuickNotes project.
2. Prompt: "Add a dark theme option and remember the user's selection."
3. Observe a diff and test the change.
4. Commit the update, build version `1.1.0`, and install it.
5. Launch from the same GNOME app entry and confirm the new feature.
6. Roll back to version `1.0.0` and confirm that the old binary starts.

**Pass condition:** Launcher identity stays stable; release switching does not destroy latest source commits.

### Scenario C — GitHub sync (critical MVP)

1. Create a local Git repository (or use the initialized repo).
2. Authenticate with GitHub CLI using the supported flow.
3. Create a private remote repository and push an initial commit.
4. Make another local change and push it with explicit approval.
5. Open GitHub URL from Studio; clone/import to a separate workspace to verify content.

**Pass condition:** Errors and conflicts are visible; no repository becomes public by default.

### Scenario D — Recovery (critical MVP)

- Agent process fails during modification: source remains inspectable, logs are retained, and user can retry or recover.
- AppImage build fails: previous installed release continues to launch.
- Studio process is killed during install: old executable remains intact; next launch reports interrupted job.
- A project folder is moved/deleted: Studio shows a repair/relink path rather than crashing.
- An external CLI is missing: Studio shows remediation without hanging.

---

## 10. Nonfunctional Requirements

- **Supported system:** Recent Manjaro GNOME desktop; validate Wayland and X11 to the extent relevant.
- **Startup:** Dashboard should become usable promptly and must not synchronously probe slow AI/network services on the UI thread.
- **Responsiveness:** Agent/build output and project lists remain responsive during long jobs.
- **Reliability:** Jobs, sessions, build artifacts, and installed releases persist across restarts.
- **Observability:** Structured logs and copyable diagnostics for failures; log retention and privacy controls.
- **Accessibility:** Keyboard-first navigation, focus order, readable contrast, clear labels and meaningful status text.
- **Portability:** No hardcoded absolute home paths, no assumption of bash as interactive shell, no root requirement for normal app installation.
- **Testability:** Unit tests for metadata, path validation, version mapping, adapters and error cases; integration tests for Git, process supervision, AppImage build and `.desktop` lifecycle.
- **Maintainability:** Provider-specific options and templates are data/configuration-driven where practical.

---

## 11. MVP Implementation Sequence

Implement in this order, keeping the application runnable after each step.

### Milestone 1 — Foundation

Settled in the engineering review of 2026-11-02. Where this section is more specific than Sections 4–6, this section wins for Milestone 1.

**Order of work:** do the packaged spike (step 1) first. It settles the platform risks before the dashboard and wizard are built.

1. **Scaffold and packaged spike.** Scaffold Tauri 2 + Vue 3 + TypeScript with create-tauri-app (vue-ts), laid out in the Section 6.3 tree. Build an AppImage with `cargo tauri build --bundles appimage`, add a test `.desktop` entry, close the terminal and start Studio from GNOME Activities. While doing so:
   - record how Studio's environment inside the AppImage differs from a plain GNOME launch (input for step 6);
   - check the window renders on this NVIDIA + Wayland machine;
   - build and launch a stock app from the template.
2. **Capabilities.** Grant only Tauri core defaults, dialog open and a scoped opener. There is no shell plugin. A test fails if any `shell:` permission appears in `capabilities/*.json`.
3. **Persistence.** Use rusqlite with the `bundled` feature, and keep it in Rust only: the frontend never runs SQL. One connection sits behind a Mutex, with `foreign_keys=ON` and WAL. Migrations are embedded and tracked with `PRAGMA user_version`. Migration 0001 creates only `projects` and `app_settings`. `projects.default_agent_id` is nullable with no FK until `agent_profiles` exists. Every other table in 6.4 ships with the milestone that first uses it.
4. **Template and Create Project.**
   - **Template.** Vendor `templates/tauri-vue` and ship it as a Tauri bundle resource. Create copies it and sets the display name, bundle identifier and Cargo package name. It edits tauri.conf.json, package.json and Cargo.toml as parsed JSON/TOML, never with text replace. Identifier rule: reverse-DNS, each segment letters/digits/hyphens starting with a letter, at least two segments. Cargo package name rule: lowercase `[a-z0-9_-]`, no leading digit. Names with quotes, emoji or non-Latin text must still produce valid files. The template's `main.rs` carries the same guarded graphics logic as Studio (step 6).
   - **Validation.** Refuse:
     - a target that exists and is not empty;
     - a target inside another library project or inside Studio's data dir;
     - unresolved `~`, `..` or relative paths;
     - an unwritable parent;
     - a slug collision, or a name whose slug is empty.
   - **Crash-safe order.** Build the project in a Studio-named temp folder beside the target, rename it into place, then insert the DB row. Run `git init` only when the toggle is on. On failure, delete only what Create wrote, and never a folder the user picked. At startup, remove leftover Studio-named temp folders.
5. **Project identity, Import, Dashboard.**
   - **Identity.** `.ai-app-studio/project.json` owns identity: `schema` (1), `id`, `name`, `slug`, `template_id`, `app_id`. The database caches it plus local state (`workspace_path`, `archived`, timestamps). A `schema` newer than this Studio understands is refused with a "made by a newer Studio" message.
   - **Import admission.** Import works from a local folder only; Clone from GitHub moves to Milestone 5. Compare canonical paths (symlinks resolved): re-importing a registered workspace is a no-op. A folder with `src-tauri/tauri.conf.json` gets template `tauri-vue`, with name and identifier read from that file. Any other folder gets template `existing`, with no build/package/install operations, and its card says so. Never run `git init` in a folder that already has `.git`. When project.json and tauri.conf.json later disagree, project.json wins and the card shows a mismatch note.
   - **Known ID on import.**
     - Old folder gone: offer Relink.
     - Old folder still present: treat it as a copy. After the user confirms, it gets a new project ID **and** a new `app_id`/bundle identifier. The new row is inserted as pending, project.json is replaced atomically (temp file + rename), then pending is cleared. Startup reconciles pending rows from project.json.
   - **Dashboard.** Empty state ("Create Your First App"), project cards, and a Relink state for missing workspaces.
6. **Environment doctor.**
   - **Shell capture.** At startup, in the background, run `$SHELL -lic` once (10 s limit; output read between markers) to capture PATH and the aliases of tracked tools. Keep that PATH inside the diagnostics service and pass it explicitly to every spawned process. Never `set_var` the process environment.
   - **Resolution and spawning.** A tool is resolved from, in order: a per-tool path override in Settings, then the shell PATH, then the GUI PATH. The doctor shows which source was used. When Studio runs as an AppImage, remove the variables its launcher added (the list measured in step 1) before spawning host tools.
   - **Probes.** `--version` probes run in parallel via `tokio::process`, each in its own process group, with a 5 s limit. On timeout the group is killed and reaped, and the tool shows Misconfigured. Output is capped at 64 KiB per stream. Results are cached for the session, with a Re-check button.
   - **Warnings.** Warn when an alias adds environment variables (e.g. `claude` setting `CLAUDE_CONFIG_DIR`). Per-agent environment configuration stays with the Milestone 2 AgentProfile.
   - **Graphics.** A Graphics row reports NVIDIA + Wayland. Ship no default workaround. Only if step 1 or acceptance shows a blank window or Wayland Error 71: set `__NV_DISABLE_EXPLICIT_SYNC=1` first, and `WEBKIT_DISABLE_DMABUF_RENDERER=1` only if still broken. Set it in `main()` before Tauri starts, only when `/proc/driver/nvidia/version` exists and only if the user has not set the variable.
7. **Commands and types.** Every command that touches disk, DB or processes is `async`; disk and DB work goes through `spawn_blocking`. tauri-specta generates `src/bindings.ts` (pin exact tauri-specta/specta versions). A test fails when the committed bindings are stale.
8. **Frontend.** Main navigation, Dashboard, New Project wizard, Import Project (with the copy confirmation), Settings (default projects folder, tool overrides) and Doctor. State lives in Pinia. Vitest + @vue/test-utils specs cover wizard steps and store refresh, with invoke mocked through the generated bindings.
9. **Tests.**
   - **Rust.** Unit and integration tests use temp dirs and cover every Create/Import/List branch, including each half-finished crash state built directly. They also cover template substitution (quotes, emoji, CJK, leading digit), migrations, and the doctor with a fake `$SHELL` script (marker parsing, timeout fallback, alias warning, override precedence).
   - **Contract tests.** Capabilities and bindings.
10. **Acceptance.** On Manjaro GNOME (Wayland, NVIDIA), with the terminal closed, start Studio's AppImage from GNOME Activities. The window renders and the doctor matches reality (pnpm found via the shell PATH; claude alias warning shown). Create a project from the packaged Studio. That project builds with `pnpm install && pnpm tauri build --debug`, and its debug binary opens a window. Restart Studio and confirm the project card persists. Update the README with Manjaro setup and known limitations.

**Deferred out of Milestone 1:**
- AgentAdapter/BuildAdapter/LauncherAdapter traits move to Milestones 2 and 4, so each is designed against its first implementation.
- Clone from GitHub moves to Milestone 5.
- The other tables move to their milestones.
- Installing generated apps to GNOME moves to Milestone 4.

### Milestone 2 — First Working Agent

Settled in the engineering review of 2026-11-02 (D23–D40, recorded in `docs/decisions/0002-milestone-2-eng-review.md`). Where this section is more specific than Sections 4–6, this section wins for Milestone 2.

**Order of work:** the CLI spike (step 1) comes first. Then three parallel lanes: Claude adapter (steps 6–7), snapshots (step 4), process/persistence/profiles (steps 2, 3, 5). Then the job supervisor and commands, then the UI.

1. **CLI spike (done 2026-11-02; fixtures in `src-tauri/tests/fixtures/claude/`).** Run the installed Claude Code CLI with `-p --input-format stream-json --output-format stream-json --verbose` and host permission prompts. Record one file edit, one approved command, one denied command and one Plan-first run. Save the raw transcripts as test fixtures and note the verified CLI version range. Costs a few cents of quota; the owner picks the profile.
2. **Process helper.** `platform/process.rs`: spawn in a new process group with an exact env and cwd, stream lines, set `PR_SET_PDEATHSIG`, kill and reap the whole group. The doctor's `run_capped` moves onto it with its tests unchanged.
3. **Persistence.** Migration 0002 adds `agent_profiles`, `jobs` and `agent_sessions`, plus the `projects.default_agent_id` foreign key. `jobs.state` is the only run status. `agent_sessions` holds job_id, profile, mode, prompt, provider session id, log path and snapshot refs. An M1 database must upgrade with its projects intact.
4. **Snapshots, diff, revert.** `services/git` keeps a private shadow git repo per project in Studio's data dir (`GIT_DIR` there, work tree = project, temp index). The user's repo, branch, index and files are never touched; non-git projects work too.
   - **Snapshot timing.** BEFORE is written, and its ref stored on the job, before the agent starts. If BEFORE fails, the run does not start. AFTER is taken only on normal exit.
   - **What snapshots skip.** The project's `.gitignore`, plus always: `node_modules/`, `target/`, `dist/`, `.git/`, `.venv/`, files over 50 MB, and secret-looking files: `.env`/`.env.*` (except `.env.example`), `*.pem`, `*.key`, `*.p12`, SSH keys, `.npmrc`, `.netrc`, `*credentials*`. Skipped paths are recorded per snapshot and shown, and are never restored automatically.
   - **Diff.** BEFORE..AFTER, with a per-file diff viewer.
   - **Revert this run.** It reverts "all changes while the run was active". A confirmation lists every file with a checkbox (all on), so the user can untick their own edits. A file is restored only if it still equals AFTER; otherwise it is skipped and listed.
   - **Interrupted runs.** No AFTER; Revert shows BEFORE vs current and restores only the files the user picks.
   - **Retention.** Keep the last 20 runs per project; older ones show "snapshot pruned". Offer "Delete all snapshots".
5. **Agent profiles (Settings > AI Agents).** A profile holds executable, env, model, inactivity limit and instructions.
   - **First profile.** Proposed from the doctor: executable plus the alias's env (e.g. `CLAUDE_CONFIG_DIR`), with leading `~`/`$HOME` expanded. Saved only when the user confirms.
   - **Secrets.** Secret-looking env names or values (`*_KEY`, `*_TOKEN`, `*_SECRET`, `*PASSWORD*`, known token shapes) are refused: authentication stays with the CLI's own login.
   - **Instructions.** Studio's default instructions go in via `--append-system-prompt`, editable per profile; no CLAUDE.md is written.
   - **Probe.** A profile is probed with its exact env before first use.
6. **Claude adapter.**
   - **Interface.** The `AgentAdapter` trait is stateless: `capabilities()`, `probe(profile)`, `start(profile, RunRequest, EventSink) -> RunHandle { cancel, answer_approval, wait }`. The job supervisor owns which runs exist.
   - **Input and parsing.** The prompt goes in as a stream-json user message on stdin, never argv or a shell. stream-json is parsed into the normalized events of F04 inside the adapter; raw CLI types never reach the UI.
   - **Errors.** A run-outcome classifier maps every F05 error case to its own message.
   - **Logs.** Session logs are JSONL in Studio's data dir, with token-shaped secrets redacted. Each tool result is capped at 64 KiB and each log at 20 MB, with visible "truncated" markers. Approval, completion and error records are never truncated.
7. **Approvals (verified by the M2 spike on CLI 2.1.316).**
   - **Every run:** `--setting-sources project,local --strict-mcp-config` (D43), so the user's global allow rules, hooks, plugins and MCP servers never bypass Studio's approvals.
   - **Live approvals:** add `--permission-prompt-tool stdio`. Each `control_request` with subtype `can_use_tool` shows in the chat with the exact tool and full input (complete command for Bash). Studio replies with a `control_response`: allow (`updatedInput`) or deny (with a message). The run waits.
   - **Plan first** (`--permission-mode plan`): the `ExitPlanMode` request carries the finished plan in `input.plan`. Studio captures it as the run's result and denies the request, so Claude never starts coding. Note: the CLI writes its plan file into its own config dir, not the project.
   - **Fallback:** used if the CLI version is outside the spike-verified range or the handshake fails, and only if the flags below appear in that version's `--help`. It keeps the requested mode (D42):
     - Plan first: `--permission-mode plan --permission-prompts none`.
     - Make changes: the CLI's default mode with `--permission-prompts none`. Reads and read-only commands run; every edit and file-changing command is denied and listed with "Allow and retry".
     - `acceptEdits` is never used: it also auto-runs file-changing shell commands.
   - **Blocked:** if neither path works, the run is blocked with the incompatible-CLI error.
   - **Allow and retry** adds exactly that tool rule to `--allowedTools` for the next run of that session only.
   - **Never** `--dangerously-skip-permissions`.
8. **Job supervisor.**
   - One modifying job per project. State changes are single transactions.
   - A run is cancelled after 10 minutes without any CLI event, not counting time waiting for approval (per-profile override).
   - Cancel and app exit kill the process group. At startup, `running` and `waiting_for_approval` jobs become `interrupted`.
9. **Commands and events.** Typed tauri-specta events, one per agent message, tool step or approval (no token streaming), each with a per-run sequence number. A view subscribes, then replays the session log and drops duplicates.
10. **Frontend.**
    - **UI spec.** The bullets below come from the design review of 2026-11-02 (D44–D62, recorded in `docs/decisions/0003-milestone-2-design-review.md`). Where the approved mockups differ from this text (boxed timeline rows, the approval card's left stripe, invented tabs), this text wins.
    - **Navigation (D46).** Clicking a project card (or its primary "Open" button) opens the project view; the card's other buttons don't. The header shows a breadcrumb "Projects › QuickNotes". The sidebar keeps Projects active. The breadcrumb and Esc return to the dashboard.
    - **Run selection (D47).** One run picker sits in the project header ("Today 14:32 · Make changes · Succeeded"), shared by the Chat and Changes tabs, so both always show the same run. Opening a project selects the newest run, and starting a run selects it.
    - Project view with AI Chat: Plan first / Make changes, profile override, and a note that edits made during a run are included in its changes.
    - Event list and approval dialog.
    - Changes tab with file list, +/- counts, diff viewer and Revert confirmation.
    - Settings > AI Agents.
    - **States.** Each row says what you see.

      | Surface | Empty / first time | In progress | Problem | Done |
      |---|---|---|---|---|
      | Chat (D48) | Two lines: what to type, and what each mode does ("Plan first: Claude proposes, no files change"). Composer focused. Plan first preselected on a project's first run; afterwards the project remembers the last mode. | Timeline rows appear as events arrive. Status "Running · 1m 12s". Cancel replaces Run. | No saved profile: Run becomes "Set up Claude first", which opens Settings > AI Agents (D52). | See run endings. |
      | Run endings (D49) | — | — | The final row gives the plain reason, whether files changed, and one next action. Examples: "Stopped: no activity for 10 minutes. 2 files changed · Review changes"; "Interrupted when Studio closed · Review changes"; "Cancelled · Review changes"; "Blocked: this Claude Code version isn't supported · Open AI Agents settings". The raw CLI error sits under a collapsed "Details". | "Done. 3 files changed · Review changes". A Plan first run ends with its plan (see the journey section). |
      | Fallback denials (D50) | — | A one-line banner at run start: "Approvals are off for this Claude version; edits will be listed for you to allow". | — | A "Blocked actions" block in the final row. Each item shows the exact tool and input and has its own Allow and retry, which allows that one rule for one new run. |
      | Changes (D51) | "This run didn't change any files." | "Changes appear when the run finishes." | Interrupted run: a banner "Studio closed during this run. Showing the difference from before the run to now"; revert checkboxes start unticked. Pruned snapshots: "Snapshots for this run were cleaned up", and Revert is disabled. | Binary file: "Binary file changed (size before → after)". A diff over 5,000 lines is cut, with "Show full diff". After a revert, a report lists restored files and files skipped because they were edited since. |
      | AI Agents (D52) | The alias suggestion banner (D33). No CLI: "Claude Code isn't installed or isn't on your PATH", with the Doctor's install hint. | Test profile shows a spinner, then "Works · v2.1.316" or the error. | A version outside the verified range: amber "Not verified with this version; approvals may fall back to strict mode". A secret-looking env var: an inline field error. | "Saved". |
    - **Plan to changes (D53).** A Plan first run ends with its plan as a card. "Make these changes" starts a Make changes run that resumes the same Claude session, so the plan carries over. "Revise plan" focuses the composer for feedback, sent as another Plan first run.
    - **Approval while you're elsewhere (D54).** A waiting approval puts an amber "Needs approval" badge on the Chat tab, the project card and the Projects sidebar item; clicking any of them jumps to the approval card. If the Studio window isn't focused, Studio sends one GNOME notification ("QuickNotes: Claude wants to run a command"), and clicking it raises Studio. This uses the Tauri notification plugin.
    - **Timeline style (D55).** Rows sit on a vertical rail with icon nodes and no boxes. A tool step is one compact line (icon, verb, path, +/-), expandable. Consecutive reads collapse into "Read 6 files". Your prompt and Claude's text are plain paragraphs. Only the approval card and the plan card get a container.
    - **Approval card (D56).** A `--warn-bg` surface with a full 1px `--warn` border, an amber node on the rail, and a warning icon in the title. No colored left stripe. Its content: the full command or edit in `--mono`, the description, the working directory, then Deny, Allow once, and "Deny with a note…".
    - **Design system (D57).** Write a DESIGN.md with `/design-consultation` before building the M2 UI, seeded from the tokens in `src/style.css`. The new components (tabs, segmented control, timeline, approval card, badge, diff view, env key/value editor) must use its tokens only; no hex values inside components.
    - **Narrow windows (D58).** The minimum window is 760×560. Below 1100px, the sidebar collapses to a 56px icon rail with tooltips. Below 900px, the Changes file list becomes a dropdown above the diff, and the composer controls wrap onto a second row.
    - **Accessibility (D59).**
      - The timeline is `role="log"`. The status line is `aria-live="polite"`. A new approval is announced assertively ("Claude wants to run a command: pnpm add @vueuse/core").
      - Focus never moves on its own. Alt+A focuses the waiting approval card.
      - Allow has no Enter default and no single-key shortcut.
      - Ctrl+Enter runs from the composer.
      - Tabs, the segmented control and the checkboxes are native or follow the ARIA tabs/radiogroup patterns, with visible `--accent` focus rings.
      - The Revert dialog reuses Modal's focus trap.
    - **Agent text (D60).** Claude's text and the plan card render a safe Markdown subset: paragraphs, emphasis, lists, headings (as bold text), inline code, and code blocks in `--mono`. Use markdown-it with `html: false`. Raw HTML is escaped and images are not shown. A link shows as its text plus its URL and opens in the system browser only on click. A test feeds `<script>`, `<img onerror>` and `javascript:` links and checks that nothing executes.
    - **Cancel (D61).** Cancel stops the run at once, with no dialog. Files already changed stay changed. The final row reads "Cancelled · 2 files changed · Review changes".
    - **Journey (first run, acceptance prompt).**

      | Step | You do | You feel | Supported by |
      |---|---|---|---|
      | 1 | Open QuickNotes from the dashboard | oriented | D46 breadcrumb, D48 empty state |
      | 2 | Type a prompt, keep Plan first, press Run | safe | D48 Plan first default |
      | 3 | Read the plan, click "Make these changes" | in control | D53 plan card |
      | 4 | Approve `pnpm add` inline (or from a badge or notification) | trusted, informed | approval card with the full command, D54 |
      | 5 | See "Done. 3 files changed · Review changes" | finished | D49 final row |
      | 6 | Read the diff and revert one file if it's wrong | reassured | D51 states, revert report |
    - Vitest specs for each.
11. **Tests.**
    - **Fixtures.** Spike fixtures drive parser and classifier tables.
    - **Fake CLI.** A fake `claude` script on the real spawn path covers: a full run, allow/deny over stdin, fallback, blocked, cancel, the one-run lock, the inactivity limit and interrupted at startup.
    - **Snapshots.** Temp-repo tests cover .gitignore, non-git projects, dirty tree, skipped paths, edited-since files and pruning.
    - **Migration.** Upgrade from a v1 database.
12. **Acceptance.**
    - From the packaged AppImage with the real CLI on a fresh project: a Plan-first run changes nothing.
    - A Make changes run ("Add a dark theme option and remember the user's selection") with one approval; then diff, revert and cancel.
    - Kill Studio mid-run: claude exits, and the job shows interrupted after restart.
    - The project still builds.

**Deferred out of Milestone 2:** commit, staging and branch UI (Milestone 3); token-by-token streaming; storing agent secrets; additional agent adapters (F14); usage and cost reporting (F21).

### Milestone 3 — Source and Preview

- Add Git status/diff/commit/checkpoints.
- Add preview process management and build logs.
- Test change requests against existing project code.

### Milestone 4 — Build, Install, and Rollback

- Implement Tauri AppImage build adapter, artifact verification, and release metadata.
- Implement Linux XDG installer, stable launch target, icon and `.desktop` generation.
- Build Installed Apps view, install/uninstall, release activation and rollback.
- Run complete GNOME launcher acceptance scenario.

### Milestone 5 — GitHub

- Integrate `gh` authentication/status, private repository creation, clone, push, fetch, pull.
- Handle conflicts and missing authentication cleanly.
- Complete automated and manual end-to-end acceptance scenarios.

Then expand through P1 requirements. Avoid spending the first phase on a marketplace, multi-agent orchestration, visual page designer, or generalized packaging for every language.

---

## 12. Definition of Done for MVP

The MVP is complete only if all of the following are true:

- [ ] Studio runs on Manjaro GNOME as a desktop application.
- [ ] Local projects can be created, imported, listed, opened and persisted.
- [ ] Claude Code CLI is detected/configurable and can generate and modify real project files.
- [ ] AI output streams into UI; failures and cancellations are handled reliably.
- [ ] User can see Git status and code differences, then commit changes.
- [ ] A supported Tauri project can be previewed and built into an AppImage.
- [ ] Studio can install a generated app to the GNOME launcher without root privileges.
- [ ] Generated app starts from GNOME when Studio is completely closed.
- [ ] New releases can be installed without creating duplicate launcher entries.
- [ ] A previous AppImage release can be reactivated without resetting source code.
- [ ] GitHub repo create/import/push/fetch/pull work with authenticated `gh`.
- [ ] Project files, tokens, installed apps, and existing Git history survive error/restart tests.
- [ ] No unconditional dangerous permission bypass or unrestricted frontend shell execution exists.
- [ ] Installation/uninstallation affects only Studio-owned artifacts, unless the user explicitly requests otherwise.
- [ ] README includes setup and run instructions for Manjaro and documents known limitations.

---

## 13. Explicit Non-Goals for MVP

Do **not** implement these in the first release:

- Browser-hosted/cloud IDE, remote multi-user collaboration, or mandatory online accounts.
- A universal build-to-AppImage pipeline for arbitrary source repositories.
- Automatic deployment to servers, app stores, or production environments.
- Silent autonomous approval of arbitrary agent/system actions.
- Background AI work or external job scheduling after Studio exits.
- Full multi-provider parity or automatic provider failover.
- A full-featured replacement for VS Code/Cursor.
- An app marketplace or public distribution hosting service.
- Windows/macOS packaging; define extension points only.

---

## 14. Instructions to the Implementing AI Coding Agent

You are implementing **AI App Studio** according to this specification.

1. **First inspect the available Linux toolchain and repository state.** Do not assume all compilers, SDKs, CLIs, or dependencies are already installed.
2. **Create a short architecture plan and implementation checklist** aligned with the milestones above; identify concrete external dependencies and points that need user approval.
3. **Use Rust + Tauri 2 + Vue 3/TypeScript** for the Studio itself. Prefer standard, maintained libraries and consistent typed interfaces. Do not add a backend server merely to bridge local commands.
4. **Implement a working vertical slice before optional features.** Prioritize real project creation, real Claude Code integration, actual AppImage build, and GNOME launcher integration.
5. **Use adapters/interfaces** for agent providers, project builders, platform installers, and path resolution. Keep Manjaro-specific logic isolated.
6. **Treat generated commands and agent output as untrusted.** Do not expose unrestricted shell execution to the UI. Do not silently skip agent permissions.
7. **Write automated tests** for domain/services and small integration tests for executable detection, Git operations, process cancellation, file paths, release switches, and desktop entry generation.
8. **Keep the user informed through the actual UI:** actionable errors, genuine progress, file changes, build results, and installation status.
9. **Do not fabricate working integrations.** If an external CLI or build dependency is absent, report the missing capability rather than implementing mock success behavior.
10. **Maintain real source repositories** and clear README/architecture documentation. Keep experimental features behind explicit flags or outside MVP.
11. **Test on Manjaro GNOME** and demonstrate the complete Scenario A + B workflow before marking MVP done.
12. **Stop after each major milestone to run tests and resolve failures** before proceeding to the next.

### First coding task

Implement Milestone 1 exactly as specified in Section 11, starting with its step 1:
- Scaffold Tauri 2 + Vue 3 + TypeScript.
- Start the AppImage from GNOME.
- Confirm the window renders on this machine.

Then build persistence, the template and Create/Import flow, the environment doctor, typed commands and the frontend, and pass the Milestone 1 acceptance. Do **not** define AgentAdapter, BuildAdapter or LauncherAdapter interfaces yet: each one is designed in the milestone that ships its first implementation (Milestones 2 and 4). Stop after Milestone 1 to run all tests and resolve failures before starting Milestone 2.

---

## 15. Technical References

These are documentation references, not hardcoded version guarantees; verify the CLI and package versions installed on the target machine before implementation.

- Tauri v2 — AppImage packaging and Linux compatibility: https://v2.tauri.app/distribute/appimage/
- Tauri v2 — Shell and process permissions: https://v2.tauri.app/plugin/shell/
- Claude Code — CLI command and output formats: https://docs.anthropic.com/en/docs/claude-code/cli-usage
- GitHub CLI — manual and authentication: https://cli.github.com/manual/
- GitHub CLI — create repository: https://cli.github.com/manual/gh_repo_create
- AppImage — run and desktop integration: https://docs.appimage.org/user-guide/run-appimages.html
- AppImage — desktop integration details: https://docs.appimage.org/reference/desktop-integration.html
- freedesktop — Desktop Entry Specification: https://specifications.freedesktop.org/desktop-entry/latest-single/

**Implementation reminder:** A packaged Linux app and its launcher are distinct things. Generating an AppImage alone does **not** guarantee that it appears in the GNOME Applications overview. The XDG integration step is a mandatory product feature.
