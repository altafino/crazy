# 0009: Milestone 5 design decisions

- **Status:** Accepted
- **Date:** 2026-11-03
- **Scope:** the Milestone 5 UI: the GitHub tab, the Source merge state, the Clone dialog, the new-project wizard option and the Dashboard header.

This file records why the M5 UI looks the way it does. It is a record, not a spec: the spec is PRD Section 11, Milestone 5, step 7, and `DESIGN.md` ("New in Milestone 5"). Where the two disagree, the PRD wins. Decision IDs continue from the engineering review ([0008](0008-milestone-5-eng-review.md)). The review also made two process choices: review all 7 areas, and no outside design voices.

## Context

- Before the review the plan listed buttons only; it rated 2/10.
- `DESIGN.md` and the M1 to M4 components exist, so every decision is calibrated against them.
- The review generated 6 mockups; the owner picked one per screen. They were not regenerated (M5-DR13 note below): the text wins where they differ.

## Approved mockups

| Screen | In the repo | Notes |
|---|---|---|
| GitHub tab, ahead state | `docs/design/m5/github.png` | Sixth tab "GitHub". Status row "2 commits to push to GitHub" with Fetch and "Push 2 commits". "Not on GitHub yet" commit list, last sync caption, "Signed in as …" and Open on GitHub. Window chrome is generator output: ignore it. Tokens win |
| Source tab, merge in progress | `docs/design/m5/merge.png` | Amber merge callout with Abort merge. Conflict rows with "Mark resolved". "Commit merge" disabled until resolved. Conflict markers highlighted in the diff. Add the "Ask Claude to resolve" link (M5-DR8). Tokens win |

## Decisions

| # | Decision | Why |
|---|---|---|
| M5-DR1 | The GitHub tab label carries a sync count when a remote exists: "GitHub ↑2", "↓3", "↑2 ↓1" (caption size, muted, tabular-nums; accessible name "GitHub, 2 to push, 1 to pull"). Source's "Nothing to commit" state adds "N commits not on GitHub yet · Open GitHub tab". | Awareness outside the tab without nagging. |
| M5-DR2 | The Dashboard header holds Import folder and Clone from GitHub (both secondary) and New project (primary). The empty state lists Create Your First App, Import a folder and Clone from GitHub. | Clone is discoverable where projects start. |
| M5-DR3 | The GitHub tab follows the state table below: one sentence in label weight, a caption and exactly one primary action. | One obvious next step per state. |
| M5-DR4 | Sign-in shows its code inline: the row reads "Enter this code on GitHub", the code in mono at 18px (selectable), Open GitHub (primary), Copy code, Stop and an elapsed caption. The tab label shows a waiting dot. The row flips to signed in by itself. | The user never leaves the app to find the code. |
| M5-DR5 | The visible "Not on GitHub yet" list plus the "Push N commits" button is the push approval. No dialog. Push never forces. | The user sees exactly what goes out. |
| M5-DR6 | The Create GitHub Repository dialog has Name (pre-filled with the project slug, validated live), Description (optional) and Private (default) or Public radios. Public shows `.callout.warn` "Anyone on the internet can see this code" and a required checkbox "I understand this repository will be public". The button becomes "Create public repository". | PRD: private by default, public only after explicit confirmation. |
| M5-DR7 | Each conflict row has "Mark resolved", which stages that file. If conflict markers remain, "Mark resolved anyway?" asks first. During a merge the file checkboxes are hidden and the commit button reads "Commit merge" (whole-merge commit, message pre-filled "Merge GitHub's changes into <branch>"), enabled when no conflicts remain. | Finishes a merge inside Studio. |
| M5-DR8 | The merge callout has an "Ask Claude to resolve" link (the existing `nav.askClaude` handoff, Make changes, pre-filled with the conflicted file list). Nothing runs until Run. | The agent helps, but never acts unasked. |
| M5-DR9 | DESIGN.md gains "New in Milestone 5": Sync status row, Commit list, Tab sync count, Sign-in code, Visibility choice, Merge callout, Conflict row, written with existing tokens only. | Builders follow specs, not pictures. |
| M5-DR10 | Error wording is fixed (list below), each with its fix button. Raw redacted text sits under Details. | Plain cause, one next step. |
| M5-DR11 | Below 900px the status row's buttons move under the sentence and commit-list times move into a caption line. Sync results are announced politely ("Pushed 2 commits"). The sign-in code has a label, and Copy announces "Code copied". Disabled buttons give their reason through `aria-describedby`. Per-file buttons name the file ("Mark src/App.vue resolved"). Vitest checks the accessible names. | Fits the 760 px window and screen readers. |
| M5-DR12 | The Clone dialog has a Repository field (visible label, example "stefan/quicknotes or a GitHub URL") and a filterable list of your GitHub repositories (`gh repo list --json`, newest first, with loading, empty and error states; any owner/name or URL can still be typed). Save in (default projects folder, Change…) shows the final path as a caption. Signed out shows the inline sign-in code (M5-DR4) first. Cloning shows "Cloning <repo> · N s" with Stop. Success closes the dialog and opens the project. Errors show inline. | Pick or type, with the same states as everything else. |
| M5-DR13 | The wizard gets a checkbox "Also create a private GitHub repository" under "Initialize Git repository". Off by default; disabled with "Needs Git" when Git is off; caption "as <login>" when signed in. Signed out: ticking shows "You'll sign in on the project's GitHub tab". The project is created locally and opens on its GitHub tab to sign in and finish. A GitHub failure keeps the project, and the GitHub tab offers retry. | A local project is never lost to a GitHub problem. |

The picked mockups were kept, not regenerated. Text overrides are small and listed in the mockup table.

### GitHub tab states (M5-DR3)

| State | Sentence | Primary | Secondary and notes |
|---|---|---|---|
| gh missing | GitHub CLI is not installed | Open Doctor | none |
| signed out or token invalid | Sign in to GitHub | Sign in | caption: Studio uses your GitHub CLI login and stores no token |
| signing in | Enter this code on GitHub (M5-DR4) | Open GitHub | Copy code, Stop; elapsed caption |
| offline | GitHub can't be reached | Check again | Details collapsed |
| not a Git repo, or no commit | This project isn't using Git yet, or Commit first | Open Source tab | none |
| inside a larger repo | This project is inside a larger repository | none | explains where to create it |
| no remote | Not on GitHub yet | Create GitHub repository… | none |
| branch never pushed | Branch main is not on GitHub yet | Push branch | Fetch |
| synced | Up to date with GitHub | Fetch | Open on GitHub |
| ahead | N commits to push to GitHub, plus the "Not on GitHub yet" list | Push N commits | Fetch |
| behind | GitHub has N new commits, plus the incoming list | Pull N commits | Fetch |
| diverged | Your project and GitHub both have new commits | Merge GitHub's changes | Fetch |
| merging | Merge in progress | Open Source tab | Abort merge |
| running | Pushing… / Pulling… / Fetching… / Merging… N s | Stop | other buttons disabled |
| last action failed | danger callout under the row: reason (M5-DR10) and its fix button | per M5-DR10 | raw redacted text under Details |
| blocked (dirty tree, run or build live) | the action is disabled; a muted line says why | none | none |
| non-GitHub remote | same states; the caption shows the remote URL | none | no Open on GitHub, no Create |

Every row with a remote shows the caption "<owner/repo> · private|public · branch <name> · fetched <time>". The footer reads "Signed in as <login>" with "Open on GitHub".

### Error wording (M5-DR10)

| Situation | Text | Action |
|---|---|---|
| Sign-in needed | GitHub needs you to sign in again. | Sign in |
| Push rejected | GitHub has commits you don't have yet. | Pull |
| Diverged | Your project and GitHub both changed. | Merge GitHub's changes |
| Stalled or unreachable | The connection to GitHub stopped responding. | Try again |
| Dirty tree | Commit or restore your changes first. | Open Source tab |
| Run or build live | Wait for the Make-changes run / release build to finish. | none (disabled) |
| Sync running | Another sync is running. | none |
| Name taken | You already have a repository named <name>. | focus Name |
| Subfolder | This project is inside the repository at <top>; create it there instead. | none |
| Clone target exists | <path> already exists. | Change folder |
| Other | Git reported a problem. | Try again |

## Journey storyboard

| Step | User does | User feels | Plan specifies |
|---|---|---|---|
| 1 | Opens the GitHub tab, signed out | unsure | "Sign in to GitHub", no token stored (M5-DR3) |
| 2 | Signs in, types the code in the browser | brief friction | inline code, Open GitHub, Copy, auto-flip (M5-DR4) |
| 3 | "Not on GitHub yet", then Create… | in control | private default, public needs a tick (M5-DR6) |
| 4 | Commits in Source, sees "2 commits not on GitHub yet" | reminded, not nagged | M5-DR1 |
| 5 | Push 2 commits | confident | the list is the approval (M5-DR5) |
| 6 | Fetch shows diverged, then Merge GitHub's changes | anxious | one plain sentence, one action (M5-DR3) |
| 7 | Conflicts: Ask Claude or fix, Mark resolved, Commit merge, Push | relieved | M5-DR7, M5-DR8, Abort merge always visible |

## Not in scope

- A sync badge on Dashboard project cards: the tab count and the Source line cover awareness inside a project.
- New mockups for the sign-in state and the Clone dialog: M5-DR4 and M5-DR12 define them.
- Visual QA of the built screens: run `/design-review` after the build.

## Completion summary

```
System audit         | DESIGN.md present (M1-M4); 7 UI surfaces
Step 0               | 2/10; all 7 passes
Pass 1 (Info Arch)   | 3/10 → 8/10
Pass 2 (States)      | 2/10 → 9/10
Pass 3 (Journey)     | 3/10 → 9/10
Pass 4 (AI Slop)     | 9/10 → 9/10
Pass 5 (Design Sys)  | 5/10 → 10/10
Pass 6 (Responsive)  | 4/10 → 9/10
Pass 7 (Decisions)   | 3 resolved, 0 deferred
Approved mockups     | 6 generated, 2 approved
Decisions made       | 13 (M5-DR1 to M5-DR13)
Overall design score | 2/10 → 8/10
```
