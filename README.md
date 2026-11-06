# AI App Studio

A local-first desktop app for creating, building and installing your own apps with AI coding agents. Built with Tauri 2, Rust and Vue 3. The product spec is in [docs/AI_App_Studio_PRD.md](docs/AI_App_Studio_PRD.md).

**Status:** Milestones 1–3 are done; Milestone 4 (AppImage build and install of generated apps) is built and waiting for acceptance; Milestone 5 (GitHub) is built and waiting for acceptance; chat attachments are built and waiting for manual checks. You can create, import and clone projects, check your tools with the environment doctor, and ask Claude Code to plan or make changes. Edits and commands are approved live in the chat. Each run's changes can be reviewed as a diff and reverted. The Source tab commits, branches and restores with your own Git; the Preview tab runs the app with `tauri dev`; the Releases tab builds and installs an AppImage; the GitHub tab creates a repository and pushes, fetches, pulls and merges.

## Requirements (Manjaro / Arch)

```sh
sudo pacman -S --needed base-devel git rustup nodejs pnpm webkit2gtk-4.1 \
  libappindicator-gtk3 librsvg openssl file xdotool fuse2 github-cli \
  xdg-utils desktop-file-utils
rustup default stable
cargo install tauri-cli --version "^2"   # or: pacman -S tauri-cli (AUR)
```

Agent runs need the [Claude Code CLI](https://docs.claude.com/en/docs/claude-code), signed in from a terminal (Studio never stores credentials). Live approvals are verified with Claude Code 2.1.316 and later 2.1.x releases. Set it up in **Settings > AI Agents**: Studio offers to copy the environment of a `claude` shell alias (for example `CLAUDE_CONFIG_DIR`), and saves nothing until you confirm.

## Develop

```sh
make dev        # install dependencies if needed, run Studio with hot reload
make check      # Rust format, lint and tests, then frontend types and tests
```

The frontend's TypeScript bindings (`src/bindings.ts`) are generated from the Rust commands. After changing a command or a type it exposes, regenerate them:

```sh
cd src-tauri && UPDATE_BINDINGS=1 cargo test bindings_are_fresh
```

`cargo test` fails if the committed bindings are stale.

**`cargo test` does a real AppImage build.** `services/builds/real_build_test.rs` creates a project from the bundled template, builds it with `tauri build`, smoke-tests it, installs it into a temporary data folder, checks the launcher with `desktop-file-validate` and uninstalls it. Nothing outside the temp folder is touched (not your real `~/.local/share/applications`). It needs node, pnpm, Rust, the Tauri Linux packaging tools from the Requirements above (the environment doctor lists them) and `desktop-file-utils`. The first run takes several minutes; the generated app's build cache lives in `src-tauri/target/real-build-cache`, so reruns are much faster. Without a display the launch test is recorded as Unverified.

## Build and install the AppImage

```sh
make            # list every command
make install    # build the AppImage and install it in ~/Applications (replaces older versions)
make run        # open the installed Studio
make status     # what is built, installed and running
```

`make install` uses AppImageLauncher when it is installed (it adds the menu entry and icons); otherwise it copies the AppImage and writes its own menu entry. The build output stays in `src-tauri/target/release/bundle/appimage/`. `make uninstall` removes the app but keeps your projects and Studio data.

Studio's data lives in `${XDG_DATA_HOME:-~/.local/share}/ai-app-studio/`: the database, session logs (`sessions/`), and a private snapshot repository per project used for diffs and revert. New projects default to `~/Projects/AIAppStudio/`.

## Using an agent

1. Open a project from the dashboard.
2. In **Chat**, describe a change. **Plan first** (the default for a new project) has Claude propose a plan without changing files. **Make these changes** on the plan then continues the same conversation. **Make changes** lets Claude edit right away.
3. File edits and commands appear as approval cards with the exact command or edit. Nothing runs until you click **Allow once**. When Studio isn't focused, a desktop notification tells you a run is waiting.
   **Auto-approve** (the box next to the mode switch) lets a **Make changes** run edit files, run commands and read files outside the project without asking; the timeline marks each one "Allowed automatically". Risky actions still ask: `sudo`, `git push`/`commit`/`remote`/`reset --hard`, `rm -r` outside the project, downloads piped into a shell, edits outside the project or in `.git`, and reading hidden files in your home folder (like `~/.ssh`) or `.env` files. **Settings > Approvals** sets whether the box starts checked, and can turn off the risky-action check. **Plan first** always asks.
4. **Changes** shows what the selected run changed. **Revert this run…** restores the files you tick.
5. **Source** shows your repository's uncommitted changes. Tick files and **Commit** commits exactly those; you can also create and switch branches, see the history, and **Restore…** files to the last commit (undoable until the next restore). **Commit…** at the end of a run pre-ticks that run's files.
6. **Preview** runs the app as a separate window (`tauri dev`, on a free port). It installs dependencies with the project's package manager first, shows the build phase and log, and on a build error offers **Ask Claude to fix**, which fills the chat without running anything.

### Git checkpoints

Before each agent run in a Git project, Studio records the project's state as `refs/studio/checkpoints/<run-id>` in your repository. It never moves your branch or commits your edits; the last 20 are kept. The run's details show the ref. Studio doesn't restore from checkpoints (use **Revert this run…**), but you can with Git:

```sh
git for-each-ref refs/studio/checkpoints            # list them
git diff refs/studio/checkpoints/<run-id>           # compare with now
git restore --source=refs/studio/checkpoints/<run-id> -- <file>...   # bring back named files
```

Checkpoints skip the same files as snapshots (build folders, files over 50 MB, secret-looking files). Name files rather than restoring `.`: Git deletes tracked files that the checkpoint doesn't contain, such as a committed `.env`. Tools that show every ref (`git log --all`, gitk) list checkpoints as commits named `before`; they are on no branch.

## GitHub

Studio talks to GitHub through the GitHub CLI (`gh`, in the Requirements above) and your own Git. It never stores a token: `gh` keeps its own login.

- **Sign in** from the project's **GitHub** tab (or the Clone dialog). Studio runs `gh auth login --web` and shows the one-time code inline; enter it on the GitHub page that **Open GitHub** opens. Signing in once in a terminal (`gh auth login`) works too.
- **Create GitHub repository…** on the GitHub tab creates the repository from the project (private by default; public needs an extra confirmation) and pushes the current branch. The new-project wizard can do this right after creating the project.
- **Push N commits** pushes the commits listed under "Not on GitHub yet". Studio never force-pushes; if GitHub has newer commits, pull first.
- **Fetch** checks GitHub; **Pull** only fast-forwards. When both sides changed, **Merge GitHub's changes** starts a merge: conflicts appear in the Source tab, where you fix them (or **Ask Claude to resolve**), **Mark resolved** and **Commit merge**. **Abort merge** goes back to where you were.
- **Clone from GitHub** on the dashboard picks one of your repositories (or takes any `owner/name` or URL) and opens it as a project.
- Pull, merge and push are refused while the project has uncommitted changes (where it matters), a Make-changes run or a release build is running, or another sync for the project is running.
- Network Git commands run without prompts: SSH uses `BatchMode`, and HTTPS to github.com gets its credentials from `gh auth git-credential`. Your own `GIT_SSH_COMMAND` or `core.sshCommand` is kept. A transfer that stalls is stopped after about a minute, and every sync has **Stop**.

## Attachments

Chat can send files along with a prompt. Claude reads them with its own Read tool.

- **Types:** images (PNG, JPEG, GIF, WebP), PDF, text and code files (txt, md, csv, json, yaml, source files and similar) and Word (.docx). Other formats, such as spreadsheets and slide decks, are refused for now. Secret-looking files (`.env`, keys) are refused too.
- **How to attach:** the **Attach files** button, drag and drop onto the Chat tab, or Ctrl+V in the message box to paste an image or files you copied in Files. Each file shows as a chip; a red chip says why a file can't be sent, and **Run** waits until it is removed.
- **Limits:** by default 10 files per prompt, images up to 20 MB, PDFs up to 32 MB, text files up to 2 MB, Word files up to 20 MB, and 64 MB per prompt in total. Change them in **Settings > Attachments**; the new values apply to the next file you attach.
- **Word files need [pandoc](https://pandoc.org/)** (`sudo pacman -S pandoc`). It is optional: the environment doctor lists it, and without it a Word file shows "Install pandoc to attach Word files".
- **Where files go:** Studio copies each file to `${XDG_DATA_HOME:-~/.local/share}/ai-app-studio/attachments/<conversation>/`, outside your project, so Source and snapshots never see them. Your original is not changed. A conversation's files stay readable for later runs of the same conversation, including **Make these changes** after a plan. They are deleted when none of its runs is among the project's last 20; its chips then read "File removed". Files you attached but never sent are dropped when Studio restarts, and left-over folders are cleaned after 24 hours.
- **Attached content is untrusted input.** A document or image can contain text that tries to instruct Claude. Studio's approval cards still apply to everything beyond reading the attachments folder: edits and commands need your **Allow once**. Read what Claude plans to do before you approve it.

## App databases

An app can keep its data (scores, notes, settings) in SQLite.

- **New projects:** tick **Store data in a database (SQLite)** in the wizard. The app's Rust code opens the database with rusqlite and applies the numbered migrations in `src-tauri/migrations/` at startup; the Vue code calls Tauri commands and never runs SQL. For an existing project, the **Database** tab offers **Ask Claude to add a database**, which fills the chat with a request that follows the same convention.
- **Where the data lives:** Preview uses `~/.local/share/<identifier>/dev/app.db`, the installed app `~/.local/share/<identifier>/app.db`, so testing in Preview never touches the installed app's data.
- **Claude knows the rules:** in a project with a database, Studio adds the convention to Claude's instructions (a new numbered migration for every schema change; never edit an applied one).
- **Database tab:** shows the Preview or the installed app's data, read-only: tables, row counts, 50 rows per page, a filter. **Back up…** keeps copies you can restore; the ⋯ menu copies the installed data into Preview or resets Preview data. Restore, copy and reset are disabled while the app using that file runs. Slow reads stop after the limit in **Settings > General** (2 s by default, 0 = no limit).
- **Releases protect the data:** Install, Activate and Roll back back up the installed app's data first and change nothing if that backup fails. A release that changes the schema says so; rolling back to an older schema offers to restore the data from before the newer release. A build's test launch runs on a copy of the installed data in temporary folders, never on the real file.

## App icons

Click the icon next to the project's name to pick a picture (PNG, JPEG or WebP at least 256 × 256, or SVG). Studio writes every icon size into `src-tauri/icons/`; a picture that isn't square is centered on a transparent square. Preview and the next release use it, and the GNOME launcher shows it after you install that release. The changed files appear in **Source** like any edit, so you can commit or restore them. The new-project wizard can set the icon too (it is then part of the first commit). Changing the icon is refused while a Make-changes run or a release build is running.

## Project plan

Open a project's **Plan** tab (next to Chat), describe what the app should do and press **Make a plan**: Chat opens in Plan first with a prepared request, and nothing runs until you press Run. When Claude's plan has checklist lines (`- [ ] Task`), press **Use as project plan** on it. Studio saves it as `.ai-app-studio/plan.md`, a normal project file that goes into Git and GitHub. The Plan tab shows the tasks with progress (also as "3/9" on the tab); **Start** runs a task in Make changes, and when that run succeeds Chat offers **Mark done** and **Start next**. Only you tick tasks; Claude never ticks them. To change the plan, ask in Chat with Plan first and use the new plan. The new-project wizard has the same optional "What should this app do?" field. Ticking, saving and Start are off while a Make-changes run or a release build is running.

## Layout

```
src/                     Vue 3 frontend (Pinia stores, views, Vitest specs)
src/bindings.ts          generated by tauri-specta, do not edit
src-tauri/src/
  commands/              Tauri command boundary
  domain/                project metadata and UI types
  persistence/           SQLite (rusqlite), migrations via PRAGMA user_version
  platform/              XDG paths, graphics quirks, AppImage environment cleanup
  security/              name, slug, identifier and path validation
  services/projects/     create / import / relink / list, crash-safe ordering
  services/diagnostics/  environment doctor
  services/settings/     default projects folder, tool path overrides
  services/attachments/  chat attachments: staging, limits, pandoc conversion, pruning
  services/agents/       agent profiles; Claude Code adapter (stream-json, live approvals, fallback)
  services/jobs/         job supervisor: one run per project, cancel, inactivity limit, restart recovery
  services/git/          per-project snapshot store for diffs and revert; repo.rs (your Git: status, commit, branches, restore); checkpoint.rs
  services/preview/      preview runner: preflight, install, tauri dev on a free port, phases, bounded logs
  platform/process.rs    process groups, so cancel and exit never leave orphans
src/components/chat/     timeline, approval and plan cards, composer
src/components/changes/  file list, diff view, revert dialog
src/components/source/   Source file list, branch control, identity form
src/components/preview/  phase stepper, log view
templates/tauri-vue/     template copied into every new project
DESIGN.md                design system: tokens, components, rules
docs/decisions/          decision records
docs/design/             reference mockups
```

## Known limitations

- **NVIDIA on Wayland:** WebKitGTK crashed with "Error 71 (Protocol error)" on the development machine. Studio and the project template set `__NV_DISABLE_EXPLICIT_SYNC=1` at startup. They only do this on NVIDIA + Wayland, and only when you haven't set the variable yourself.
- **Apps started from GNOME** don't see the PATH set up in your shell. The doctor reads your login shell once in the background and shows where each tool was found. A shell alias that sets environment variables (e.g. `claude` setting `CLAUDE_CONFIG_DIR`) isn't applied automatically. Copy it into the agent profile (Settings > AI Agents offers this).
- **Agent runs ignore your global Claude settings.** Studio starts Claude Code with `--setting-sources project,local --strict-mcp-config`, so your user-level allow rules, hooks, plugins and MCP servers don't apply inside Studio, and Studio's approvals stay in charge. Project-level `.claude/` settings still apply.
- **Approval fallback:** if the installed Claude Code doesn't support live approvals, Studio runs in strict mode. Every edit and file-changing command is refused, and listed afterwards with **Allow and retry**. Studio never uses `acceptEdits` or skips permissions; Auto-approve answers the live approval requests itself, so it needs live approvals.
- **Changes include your own edits.** A run's diff covers every change in the project while the run was active, including edits you made in another editor. The revert dialog lets you untick those files.
- **Snapshots** skip build folders, files over 50 MB and secret-looking files (`.env`, keys), so those are never shown or reverted. Studio keeps snapshots for the last 20 runs per project.
- **X11 sessions with NVIDIA:** Studio's NVIDIA fix only covers Wayland. If the window stays blank under X11 (for example with `GDK_BACKEND=x11`), start it with `WEBKIT_DISABLE_DMABUF_RENDERER=1`.
- **AppImageLauncher**, if installed, intercepts AppImages. When you open one, it may move the file into `~/Applications` and create its own menu entry. Set `APPIMAGELAUNCHER_DISABLE=1` to start an AppImage directly.
- **AppImage portability:** an AppImage built on Manjaro needs a glibc at least as new as the build machine's. Portable builds from an older baseline are planned (PRD F18).
- **Commits run your hooks and signing** like a terminal does, with a 2-minute limit. A failed commit in a repo with hooks is reported as rejected by a hook, even if signing was the cause; the output is shown either way. Studio never deletes a Git `index.lock`.
- **Undo restore** brings back file contents, not what was staged, and overwrites any edits made after the restore. Secret-looking and very large files can't be undone; the dialog lists them first.
- **White preview window on first start (NVIDIA + Wayland, WebKitGTK):** WebKit can get a 404 from Vite for images imported from `public/` by absolute path (`<img src="/logo.svg">` in a `.vue` file) on the first load, and the app never mounts. Right-click → Reload in the app window fixes it for that session; importing images from `src/assets` (relative paths) avoids it. New projects do this.
- **Database row counts:** the query time limit can't stop a plain row count, so a very large table can still take a moment to count; filters and pages are stopped at the limit.
- **Preview** only handles Tauri projects. The free-port override applies when the dev command is the template's `pnpm dev`; other projects run their dev command unchanged on their own port.
- **Scope so far:** Claude Code is the only agent, and attachments work only with it. GitHub support covers one repository per project and its current branch: no tags, GitHub Releases, pull requests or CI results (PRD F12 P1).
- **Platforms:** Linux only for now. Final project creation renames a temp folder onto an empty target, which works on Linux but would need changes on Windows.
