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
| 4.1 | Create a new project; in Releases, build 1.0.0 | Build ends Passed; the release row shows 1.0.0 | |
| 4.2 | Install 1.0.0 | Installed badge on 1.0.0 | |
| 4.3 | Quit Studio completely; search the app in Activities and start it | Its own icon; one dock entry; it starts without Studio | |
| 4.4 | Change the app with Claude; build 1.1.0 and install it | 1.1.0 runs from Activities; still only one launcher | |
| 4.5 | Roll back to 1.0.0 | GNOME starts 1.0.0; project source is unchanged (Source tab shows no changes) | |
| 4.6 | Break the build on purpose (for example a syntax error) and build | Build fails with diagnostics; the installed release still launches | |
| 4.7 | Uninstall the app | Launcher is gone from Activities; the project remains on the Dashboard and on disk | |

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
| 7.1 | Kill Studio (`pkill -f "AI App Studio"`) during an agent run, a build and a push; restart | Project files, Git history, installed apps and `gh` login are intact; interrupted jobs show as interrupted, not running | |
| 7.2 | Inspect `~/.local/share/ai-app-studio/` and the database | No tokens or credentials stored | |
| 7.3 | Install and uninstall an app; compare `~/.local/share/applications` and `~/.local/share/icons` before and after | Only Studio-owned files were added and removed | |
| 7.4 | Read the README on a fresh machine (or a fresh user) and follow setup | Studio builds and runs from the README alone | |

## 5. Visual and accessibility review

| # | Check | Expected | Result |
|---|---|---|---|
| 8.1 | Run `/design-review` on the GitHub tab, Source merge mode, Clone dialog and wizard | Matches `DESIGN.md` "New in Milestone 5" and `docs/design/m5/` | |
| 8.2 | Narrow the window below 900 px | GitHub tab buttons wrap under the sentence; commit times move into a caption line | |
| 8.3 | Keyboard only: sign in, create, push, merge, clone | Every action reachable; focus visible; disabled buttons explain why | |
| 8.4 | Screen reader (Orca) on the GitHub tab | Tab name reads "GitHub, 2 to push, 1 to pull"; sync results announced; sign-in code labelled; Copy announces "Code copied" | |

## Known gaps (not failures)

- For a repository that existed before (not created in this session), the GitHub tab caption doesn't show private or public: the backend's remote info has no visibility field.
- The Source tab's "N commits not on GitHub yet" line also shows for remotes that are not on GitHub.

## Run log

| Date | Tester | Build (commit) | Sections run | Notes |
|---|---|---|---|---|
| | | | | |
