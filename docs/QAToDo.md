# QA to-do

Open manual checks for the MVP (Milestones 4 and 5 plus the Definition of Done in PRD Section 12). Automated tests (`cargo test`, `pnpm test`) pass on `dev`; everything here needs a person, a real GNOME session, a browser or a GitHub account.

Mark each row **Pass** or **Fail** in the Result column and add a note (what you saw, a screenshot path) for every Fail. When a section passes, update its milestone status in `docs/AI_App_Studio_PRD.md`.

## Setup

- Build the packaged Studio from `dev`:
  ```sh
  pnpm tauri build --bundles appimage
  APPIMAGELAUNCHER_DISABLE=1 "./src-tauri/target/release/bundle/appimage/AI App Studio_0.1.0_amd64.AppImage"
  ```
- Test on Manjaro GNOME (Wayland). Note the GPU (NVIDIA or not) in the run log below.
- To test signing in from Studio, run `gh auth logout` first. This also signs out `gh` in the terminal; signing in through Studio restores it.
- Use a throwaway project. Delete the test repositories on github.com afterwards (`gh repo delete` needs the `delete_repo` scope).

## 1. Milestone 4: build, install, rollback (PRD Milestone 4, step 9)

| # | Step | Expected | Result |
|---|---|---|---|
| 4.1 | Create a new project; in Releases, build 1.0.0 | Build ends Passed; the release row shows 1.0.0 | Pass (isolated Studio on Xvnc, 2026-11-04): built in ~2 min, test Passed |
| 4.2 | Install 1.0.0 | Installed badge on 1.0.0 | Pass (isolated): Active; one .desktop, desktop-file-validate OK |
| 4.3 | Quit Studio completely; search the app in Activities and start it | Its own icon; one dock entry; it starts without Studio | Partial (isolated): Exec line starts the app with Studio closed; GNOME Activities/dock not testable headless |
| 4.4 | Change the app with Claude; build 1.1.0 and install it | 1.1.0 runs from Activities; still only one launcher | Pass (isolated): 1.1.0 built from uncommitted edits and installed; still one .desktop |
| 4.5 | Roll back to 1.0.0 | GNOME starts 1.0.0; project source is unchanged (Source tab shows no changes) | Pass (isolated): current.AppImage → 1.0.0, toast with Undo; source unchanged |
| 4.6 | Break the build on purpose (for example a syntax error) and build | Build fails with diagnostics; the installed release still launches | Pass (isolated): "unclosed delimiter" diagnostics, "GNOME still starts 1.0.0", Ask Claude to fix; 1.0.0 still launches |
| 4.7 | Uninstall the app | Launcher is gone from Activities; the project remains on the Dashboard and on disk | Pass (isolated): .desktop and installed/ removed; project kept. Copy bug: dialog says "QA Notes1.0.0" (missing space) |

## 2. Milestone 5: GitHub, Scenario C (PRD Milestone 5, step 9)

### 2A. Create, push, clone

| # | Step | Expected | Result |
|---|---|---|---|
| 5.1 | New project with Initialize Git on; commit once in Source | One commit in history | |
| 5.2 | GitHub tab → Sign in | Code shown inline (mono, selectable); Open GitHub, Copy code, Stop; tab label shows a waiting dot | |
| 5.3 | Enter the code on GitHub and approve | Tab flips to signed in by itself; footer "Signed in as <login>" | |
| 5.4 | Create GitHub repository… | Name pre-filled with the slug; Private selected; choosing Public shows the warning and a required checkbox, and the button reads "Create public repository" | |
| 5.5 | Create it as Private | "Up to date with GitHub"; caption shows owner/repo, private, branch | |
| 5.6 | Change the app with Claude; commit it as two commits | Tab label "GitHub ↑2"; Source "Nothing to commit" state says "2 commits not on GitHub yet · Open GitHub tab" | |
| 5.7 | GitHub tab | "2 commits to push to GitHub" with both commits under "Not on GitHub yet" | |
| 5.8 | Push 2 commits | No confirm dialog; "Pushed 2 commits" announced; state "Up to date" | |
| 5.9 | Open on GitHub | Browser opens the repository; both commits are there | |
| 5.10 | Dashboard → Clone from GitHub | Your repositories listed newest first; filter works; typing owner/name or a URL also works | |
| 5.11 | Save in a different folder (for example `~/clone-test`) and clone | "Cloning <repo> · N s" with Stop; project opens when done | |
| 5.12 | Compare: `diff -r --exclude=.git ~/Projects/AIAppStudio/<slug> ~/clone-test/<slug>` | No differences | |
| 5.13 | Clone the same repo into the same folder again | "<path> already exists." with Change folder | |
| 5.14 | Start a clone and press Stop | No half-cloned folder is left | |
| 5.15 | New project wizard: "Also create a private GitHub repository" | Off by default; disabled with "Needs Git" when Git is off; "as <login>" when signed in; creating makes the project and the private repo | |

### 2B. Conflict round-trip

| # | Step | Expected | Result |
|---|---|---|---|
| 5.20 | Edit one line of a file on github.com and commit there; change the same line in Studio and commit | | |
| 5.21 | GitHub tab → Fetch | "Your project and GitHub both have new commits" with Merge GitHub's changes | |
| 5.22 | Push now | Refused: "GitHub has commits you don't have yet." Nothing is force-pushed | |
| 5.23 | Merge GitHub's changes | Source shows the amber merge callout with the conflicted file, Abort merge and Ask Claude to resolve | |
| 5.24 | Abort merge | Project back where it was; no conflict; history unchanged | |
| 5.25 | Merge again; Ask Claude to resolve | Chat is pre-filled with the conflicted files; nothing runs until Run | |
| 5.26 | Resolve one file by hand; press Mark <file> resolved while markers remain | "Mark resolved anyway?" asks first | |
| 5.27 | Resolve all; Commit merge | Button disabled with "Resolve N conflicts first" until 0 conflicts; message pre-filled "Merge GitHub's changes into <branch>" | |
| 5.28 | Push | Up to date with GitHub; GitHub shows the merge commit | |

### 2C. Guards, offline, restart

| # | Step | Expected | Result |
|---|---|---|---|
| 5.30 | Behind GitHub with uncommitted changes → Pull | Disabled: "Commit or restore your changes first." | |
| 5.31 | Pull or Merge during a Make-changes run | Disabled: "Wait for the Make-changes run to finish." | |
| 5.32 | Pull or Merge during a release build | Disabled: "Wait for the release build to finish." | |
| 5.33 | Network off (`nmcli networking off`); open the GitHub tab | "GitHub can't be reached" with Check again; no Sign in offered | |
| 5.34 | Offline: edit and commit locally | Works | |
| 5.35 | Offline: Push | Fails fast with "The connection to GitHub stopped responding." and Try again | |
| 5.36 | Quit Studio, restart, open the GitHub tab | The failed push is still shown ("Last sync: push failed …") | |
| 5.37 | Network on (`nmcli networking on`); Check again; Push | Pushes | |
| 5.38 | Error texts throughout | Never show a token or credentials in a URL | |

## 3. Real-GitHub automated test (opt-in)

| # | Step | Expected | Result |
|---|---|---|---|
| 6.1 | `cd src-tauri && STUDIO_GITHUB_E2E=1 cargo test` (signed in with `gh`) | Passes; leaves one private repository to delete by hand | |

## 4. MVP Definition of Done: items not covered above (PRD Section 12)

| # | Check | Expected | Result |
|---|---|---|---|
| 7.1 | Kill Studio (`pkill -f "AI App Studio"`) during an agent run, a build and a push; restart | Project files, Git history, installed apps and `gh` login are intact; interrupted jobs show as interrupted, not running | Partial (isolated, build only): job shows interrupted, git intact, 1.0.0 still Active; a rustc child ran on briefly as an orphan; Releases tab doesn't show the interrupted build |
| 7.2 | Inspect `~/.local/share/ai-app-studio/` and the database | No tokens or credentials stored | Pass (2026-11-04, read-only scan of the real data dir): no tokens; only env key stored is CLAUDE_CONFIG_DIR |
| 7.3 | Install and uninstall an app; compare `~/.local/share/applications` and `~/.local/share/icons` before and after | Only Studio-owned files were added and removed | Pass for the isolated XDG_DATA_HOME only (real ~/.local/share not tested): after uninstall only an empty applications/ remains |
| 7.4 | Read the README on a fresh machine (or a fresh user) and follow setup | Studio builds and runs from the README alone | |

## 5. Visual and accessibility review

| # | Check | Expected | Result |
|---|---|---|---|
| 8.1 | Run `/design-review` on the GitHub tab, Source merge mode, Clone dialog and wizard | Matches `DESIGN.md` "New in Milestone 5" and `docs/design/m5/` | |
| 8.2 | Narrow the window below 900 px | GitHub tab buttons wrap under the sentence; commit times move into a caption line | |
| 8.3 | Keyboard only: sign in, create, push, merge, clone | Every action reachable; focus visible; disabled buttons explain why | Partial (attachments only): Shift+Tab reaches the last chip's ×; Delete removes it and focuses the textarea |
| 8.4 | Screen reader (Orca) on the GitHub tab | Tab name reads "GitHub, 2 to push, 1 to pull"; sync results announced; sign-in code labelled; Copy announces "Code copied" | |

## 6. Chat attachments (decisions 0010 and 0011)

Use the packaged Studio and a throwaway project. Have a small PNG, a PDF, a `.txt` file and a `.docx` file ready. Rows 9.5 and 9.6 need pandoc removed from `PATH` (or a build started with a `PATH` without it); the others need it installed (`sudo pacman -S pandoc`).

| # | Step | Expected | Result |
|---|---|---|---|
| 9.1 | Chat → Attach files; pick the PNG, the PDF and the `.txt` | Three chips with name and size; the PNG chip shows a preview | Pass after fix (isolated, 2026-11-04): PNG chip shows its thumbnail. First run failed: staged files were in `.staging/`, which the `$DATA/**` scopes don't match |
| 9.2 | With pandoc installed: attach the `.docx` | "Converting…", then "converted"; Run waits until it is done | Not tested: pandoc not installed |
| 9.3 | Plan first: ask "what do these files say?" and press Run | Timeline shows the chips under the prompt; Claude reads the files and answers; no approval card appears | Pass (isolated): chips under the prompt, "Read 3 files", no approval card, correct answer |
| 9.4 | Press "Make these changes" on the plan | The follow-up run still reads the files; no approval card for them | Not tested (run budget); a new composer prompt starts a new conversation, as designed |
| 9.5 | Without pandoc: attach the `.docx`; open the environment doctor | Red chip "Install pandoc to attach Word files"; the doctor shows pandoc as optional, not as an error | Pass: red chip "Install pandoc to attach Word files"; doctor lists pandoc as optional (not under Warnings) but its Missing badge is red |
| 9.6 | With that chip present, look at Run | Run is disabled; the caption reads "Remove 1 file that can't be attached"; removing the chip enables Run | Pass: "Remove 3 files that can't be attached"; clears when removed |
| 9.7 | Drag a file from Files onto the Chat tab | Dashed "Drop files to attach" overlay over Chat only; the drop adds a chip | Pass (isolated, Xvnc, GTK drag source with two files dropped on the message box): overlay shown, two chips. Recheck from Files on GNOME Wayland |
| 9.7a | In Files, select two files, Ctrl+C; click in the message box; Ctrl+V | One chip per file; nothing is typed into the box. Copying plain text still pastes text | Pass (isolated, Xvnc, text/uri-list clipboard): two chips; text paste and image paste unchanged. Recheck from Files on GNOME Wayland |
| 9.8 | Drag a file over the Source tab and the Settings view | No overlay; nothing is attached | Not tested (no drag source headless) |
| 9.9 | During a Make-changes run, drag a file over Chat | Overlay reads "Attach files after this run finishes"; a drop attaches nothing | Not tested (no drag source headless) |
| 9.10 | Take a screenshot to the clipboard; click in the textarea; Ctrl+V | A PNG chip with a preview appears | Pass after fix (isolated, Xvnc): "Pasted image ….png" chip with thumbnail. WebKitGTK 2.52 gives an image paste an empty clipboardData; Studio now reads it through the Clipboard API. Recheck on GNOME Wayland |
| 9.11 | Attach an `.xlsx` and a file over the size limit | Red chips with "This file type isn't supported yet" and "Too large: …"; Run is blocked until they are removed | Pass: "This file type isn't supported yet" and "Too large: text files up to 2 MB"; Run blocked |
| 9.12 | Click an image chip (composer, then timeline); press Esc | Preview modal with the file name and "Open in default app"; Esc closes it and focus returns to the chip | Pass after fix: composer and timeline previews show the image; Esc returns focus to the chip |
| 9.13 | Click a PDF chip, then "Open in default app" in the modal | The file opens in the system app | Pass after fix (scope): "Open in default app" raises no error; no viewer window in the isolated session, so the app opening itself is unchecked |
| 9.14 | Narrow the window below 900 px | Attach becomes a paperclip icon button with a tooltip; Run's reason moves to its own line | Pass: paperclip icon with tooltip; Run's reason on its own line |
| 9.15 | Settings > Attachments: lower the image limit to 1 MB; attach a larger image; then reset | The new limit applies on the next attach ("Too large: images up to 1 MB"); an out-of-range value is refused | Pass: 300 refused ("Enter a whole number from 1 to 256."); 1 MB limit applied on next attach; reset saved |
| 9.16 | Attach a file and restart Studio before pressing Run | The chip is gone (drafts are not kept) | Pass: chip gone after restart |
| 9.17 | Run 21 or more runs in the project (or use a project with that history); reopen the first conversation | Its chips read "File removed" and are not clickable; the folder under `~/.local/share/ai-app-studio/attachments/` is gone | Not tested (needs 21 runs) |

## 7. Auto-approve

| # | Check | Expected | Result |
|---|-------|----------|--------|
| 10.1 | Settings > Approvals: tick "Auto-approve … by default"; open a project on Make changes | The Auto-approve box under the message is checked; on Plan first it is unchecked and disabled | Pass (isolated, Xvnc) |
| 10.2 | Auto-approve on, Make changes: ask Claude to create a file and run a command inside the project | No approval card; the timeline shows "Allowed automatically: …" lines; no desktop notification | Pass (isolated, Xvnc, real Claude): Write auto-allowed, file created |
| 10.3 | Same run: ask for `rm -r ../some-folder` (or `git push`) | An approval card appears as usual; Deny works | Pass (isolated, Xvnc, real Claude): card for `rm -r ../qa-auto-test-nonexistent`, denied |
| 10.4 | Settings > Approvals: untick "Still ask for risky actions"; repeat 10.3 with a harmless risky command | The warning callout shows in Settings; the command is allowed automatically | Not tested |

## 8. App databases

| # | Check | Expected | Result |
|---|-------|----------|--------|
| 11.1 | New project with "Store data in a database (SQLite)" ticked; run Preview | `~/.local/share/<identifier>/dev/app.db` exists with schema version 1; Database tab shows it | Not tested by hand (real build of the database template passes in `cargo test`) |
| 11.2 | Database tab on a project with Preview data (tables incl. a quoted name, NULL and BLOB cells, 142 rows) at 1100px and 800px | Tables list (select below 900px), right-aligned numbers, muted NULL, pager "Rows 1–50 of 142", sideways scroll inside the table only | Pass (isolated, Xvnc): fixed the path caption wrapping over 4 lines |
| 11.3 | Back up…, Restore…, Copy installed data to Preview…, Reset preview data… | Confirms as in DESIGN.md; restore makes a "before restore" backup; disabled with "Stop Preview first" while Preview runs | Not tested by hand (Vitest + Rust tests) |
| 11.4 | Install 1.1.0 with a schema change, then roll back to 1.0.0 | Caption "Database changes: schema …"; backup "before 1.1.0"; rollback dialog offers Restore data | Not tested |

## 9. App icons

| # | Check | Expected | Result |
|---|-------|----------|--------|
| 12.1 | Project header: click the icon, pick a 600 × 400 PNG | All sizes written to `src-tauri/icons/` (centered on transparent), header and card update, toast "App icon changed…", files show in Source | Pass (isolated, Xvnc) |
| 12.2 | Pick a 200 × 120 picture | Toast "This image is 200 × 120 pixels. Pick one at least 256 × 256."; old icon kept | Pass (isolated, Xvnc) |
| 12.3 | New project wizard: Choose… an SVG logo with text, create | 48px preview with the text; project created with the icon in its initial commit (clean tree) | Pass (isolated, Xvnc) |
| 12.4 | Build and install a release of a project with a custom icon | Installed Apps and the GNOME launcher show that icon | Pass (isolated, Xvnc): release 0.1.0 built (test Passed) and installed; release icon identical to the project's 256 px icon; launcher `Icon=` set; Installed Apps row shows it. GNOME Activities not checked |

## 10. Project plan

| # | Check | Expected | Result |
|---|-------|----------|--------|
| 13.1 | New project with "What should this app do?" filled | Project opens on Chat in Plan first with the plan request in the composer; nothing runs | Not tested by hand (Vitest covers the handoff) |
| 13.2 | Run it; on the plan press Use as project plan | Toast "Saved as the project plan (N tasks)" with Open Plan; tab reads "Plan 0/N"; Source shows `.ai-app-studio/plan.md` | Pass (isolated, Xvnc, real Claude; started from the Plan tab empty state): toast with Open Plan, "Plan 0/6", saved caption on the card |
| 13.3 | Plan tab: Start on the first task; let the run succeed | Chat opens; the end row shows "From the plan: …" and Mark done; Plan tab shows "In progress" while it runs and disables ticks with "Wait until the current run finishes" | Pass (isolated): In progress badge, controls disabled with the caption; end row "From the plan: …" + Mark done |
| 13.4 | Mark done, then Start next | "Marked done · 1 of N tasks done"; the next task starts | Pass for Mark done ("Marked done · 1 of 7 tasks done", file ticked); Start next shown, not pressed |
| 13.5 | Cancel a task run | End row reads "From the plan: … · not done", no Mark done | Not tested by hand (Vitest) |
| 13.6 | Edit plan.md by hand (rename a task), then tick the old name in a stale tab | "This task is no longer in the plan." and the list reloads | Not tested by hand (Rust + Vitest) |
| 13.7 | Make a second plan and Use it | "Replace the project plan?" names how many tasks are done | Not tested by hand (Vitest) |
| 13.8 | Window at 760 px; Orca on the checklist | Titles wrap, no sideways scroll; each checkbox reads its task, Start reads "Start: <task>" | Pass at 760 px (wraps, no sideways scroll; the scrollbar overlaps the Start buttons' right edge slightly); Orca not tested |
| 13.9 | Tick every task | "All N tasks done" with Build release… | Not tested by hand (Vitest) |

## 11. Exports for other systems (F22)

| # | Check | Expected | Result |
|---|-------|----------|--------|
| 11.1 | Before each Studio release, with Docker running: `STUDIO_EXPORT_E2E=1 cargo test real_export_test -- --nocapture` (first run builds the export image, several GB, and compiles the template twice) | Passes: AppImage, deb, rpm and setup.exe exist, are nonempty and match `SHA256SUMS`; the Linux binary needs at most GLIBC 2.35; setup.exe starts with `MZ`. Clear the image and caches afterwards in Settings > Exports if the disk is tight | Pass (2026-11-06, Docker 29.8.2): both targets in 232 s with warm caches, Linux binary needs GLIBC 2.34. First run failed: Ubuntu 22.04's NSIS 3.08 lacks `Win\RestartManager.nsh`, now copied from 24.04 in the Dockerfile |
| 11.2 | In Settings > General, scroll to Exports right after Studio starts | Shows the cache size or "No export caches", not "Couldn't read Docker volumes" | Fixed after QA (2026-11-06): the first read took over 10 s on a host with ~700 images; limit now 30 s |

## 13. Project ⋮ menu and Remove from Studio (F01)

| # | Step | Expected | Result |
|---|------|----------|--------|
| 13.1 | Open a project, click ⋮ next to the title; use ↑/↓, then Esc | Show in Files, Copy path, Change app icon…, Remove from Studio…; Open on GitHub with a GitHub remote; Launch and Uninstall from GNOME… when installed. The focused item is highlighted. Esc closes the menu and stays on the project | Pass (isolated, Xvnc, 2026-11-07) after a fix: WebKit drew no highlight on the arrow-key focus (`:focus-visible`); now `:focus`. Copy path fills the clipboard. GitHub/Launch/Uninstall items: Vitest only |
| 13.2 | Remove from Studio… on a project with runs, attachments and kept releases, Trash unticked | Back on Projects with a toast; the folder is still there; `installed/<id>`, `app-data-backups/<id>`, `snapshots`, attachments and the job logs are gone | Pass (isolated) for a new project: gone from the list and the DB, folder kept, toast. Runs/releases cleanup: Rust tests. Fixed: a stale "Created …" notice stayed above the list |
| 13.3 | Same with "Also move the project folder to Trash" ticked | The folder is in the GNOME Trash and can be restored | Partial (isolated): on a tmpfs folder gio refuses ("Trashing on system internal mounts is not supported"); the dialog shows it and nothing is removed. A successful trash was not tried (it would use the real Trash). Fixed: the filled red button had ~2:1 contrast in dark mode |
| 13.4 | Remove from Studio… while the app is installed, or while a run or build is running | The dialog stays open with the reason (uninstall first / stop it first); nothing is removed | Not tested by hand (Rust + Vitest) |

## Known gaps (not failures)

- For a repository that existed before (not created in this session), the GitHub tab caption doesn't show private or public: the backend's remote info has no visibility field.
- The Source tab's "N commits not on GitHub yet" line also shows for remotes that are not on GitHub.
- Word chips show "converted" without an image count: the backend has no field for it yet.

## Run log

| Date | Tester | Build (commit) | Sections run | Notes |
|---|---|---|---|---|
| 2026-11-04 | Claude (isolated Studio on headless Xvnc :77, own XDG dirs and D-Bus; no real desktop, no GitHub) | f73ce28 | 1 (no GNOME Activities), 6 (no drag and drop, no pandoc), 7.1–7.3 partly, 8.3 partly | Bugs found and fixed the same day: composer previews/Open for staged files (`.staging` not matched by the `$DATA/**` scopes), Ctrl+V image paste, draft lost on leaving the Chat tab, "QA Notes1.0.0", doctor "(Milestone 2)" text. Still open: window-wide scroll hides the sidebar. "NVIDIA on Wayland" on X11 was a test-setup artifact (inherited XDG_SESSION_TYPE). 3 Claude runs. Screenshots in the session scratchpad (not kept). |
