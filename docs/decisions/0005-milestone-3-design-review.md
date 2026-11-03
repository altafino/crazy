# 0005: Milestone 3 design decisions

- **Status:** Accepted
- **Date:** 2026-11-02
- **Scope:** the Milestone 3 UI: the Source tab, the Preview tab, the card's preview badge, the Preview group in Settings, and the Chat end row.

This file records why the M3 UI looks the way it does. It is a record, not a spec: the spec is PRD Section 11, Milestone 3, step 11, and `DESIGN.md`. Where the two disagree, the PRD wins. Decision IDs continue the engineering review ([0004](0004-milestone-3-eng-review.md)).

## Context

- Before the review the plan named components only (SourceTab, CommitForm, PreviewTab, LogView…), with no layout, states or journey. It rated 3/10.
- `DESIGN.md` and the M2 components exist, so every decision is calibrated against them.
- The review generated 8 mockups. The owner picked one per screen on comparison boards. Both were then regenerated with the review's changes, and the regenerated pair is the build reference.

## Approved mockups

The build references are committed in [`docs/design/m3/`](../design/m3/). Where they differ from the PRD text or `DESIGN.md`, the text wins. All variants stay in `~/.gstack/projects/altafino-crazy/designs/`.

| Screen | In the repo | Source variant | Notes |
|---|---|---|---|
| Source tab | `docs/design/m3/source.png` | `project-source-20261102/final.png` | The board pick (`variant-B.png`) regenerated with M3-D33/D34/D35. The title is 24px per the token, not as drawn |
| Preview tab | `docs/design/m3/preview.png` | `project-preview-20261102/final.png` | The board pick (`variant-A.png`) regenerated with M3-D44/D48. The stepper uses "—" separators (M3-D45), not the drawn lines |

## Decisions

| # | Decision | Why |
|---|---|---|
| M3-D33 | Keep the tab names Chat · Changes · Source · Preview. Changes shows the caption "What this run changed"; Source shows "Uncommitted changes in your repository". | Two diff views must not be mistaken for each other (trunk test). |
| M3-D34 | The run picker shows only on Chat and Changes; its slot stays empty on Source and Preview. | A control appears only where it changes what you see. |
| M3-D35 | Source's History starts collapsed and remembers its state per project; when open it shows the last 50 commits. | The diff is the primary job at 800px height. |
| M3-D36 | A clean tree shows "Nothing to commit" with the last commit and hides the commit box. | An empty list must read as success, not failure. |
| M3-D37 | A non-Git project shows "This project isn't using Git yet", one sentence of value, and "Initialize Git". | An empty state gives context plus one action. |
| M3-D38 | A shared toast confirms commit, restore (with Undo) and branch switch; "Undo last restore" then stays beside "Restore…". | GNOME-native confirmation without blocking. |
| M3-D39 | Preview's start screen: "Run <Project> as a separate window", a first-build expectation, "Start preview"; non-Tauri projects say why preview isn't available. | Sets expectations for a long first compile. |
| M3-D40 | When a preview ends, the log and outcome stay (stopped, closed, crashed, interrupted), with "Start again". | The log is most needed right after a crash. |
| M3-D41 | That end state applies within the current Studio session. After a restart, Preview shows the start screen with "Last preview … · View log". | Recent context while it's fresh; a calm screen later. |
| M3-D42 | A successful Make-changes end row: primary "Review changes", links "Preview" and "Commit…". Commit… pre-ticks the run's files and drafts the message from the prompt. | The run → preview → commit loop without tab hunting. |
| M3-D43 | "Ask Claude to fix" fills the Chat composer with the build error and its location, in Make changes mode. Nothing runs until Run. | Control stays visible (DESIGN.md principle 2). |
| M3-D44 | The build-error callout is `.callout.danger` with a full 1px border and no left stripe. | DESIGN.md "Don't"; the same fix as M2's D56. |
| M3-D45 | DESIGN.md gains "New in Milestone 3": toast, phase stepper, log view, switch, branch control, commit box, checkbox file list. | Specs live where builders look; M4 reuses the log view. |
| M3-D46 | Below 900px Source stacks: branch row, checkbox list (≤40% height), commit box, then the diff. | A dropdown can't hold checkboxes. |
| M3-D47 | The Source list is a multi-select listbox: one Tab stop, arrows move and show the diff, Space ticks. | Keyboard users get there without 80 Tab presses. |
| M3-D48 | The Preview tab gets a state dot (accent while starting, ok when running, danger when the build failed). The card gets a "Previewing" or "Preview: build failed" badge after "Needs approval". | One meaning per color; never color alone. |
| M3-D49 | Settings › General gets a Preview group with the three M3-D20 limits, their ranges, field errors and "Reset to defaults". | Global, rarely changed options. |

## Not in scope

- A conflict-resolution UI (M5): M3 shows conflicts and blocks Commit.
- Dark-theme mockups: the tokens cover dark; check visually after the build with `/design-review`.
- Onboarding hints for long-history repos.
- A dedicated Preview settings section (rejected by M3-D49).

## Completion summary

```
System audit         | DESIGN.md present; UI scope: Source, Preview, card badge, settings, chat end row
Step 0               | 3/10; all 7 dimensions
Pass 1 (Info Arch)   | 3/10 → 9/10
Pass 2 (States)      | 3/10 → 9/10
Pass 3 (Journey)     | 4/10 → 9/10
Pass 4 (AI Slop)     | 7/10 → 9/10
Pass 5 (Design Sys)  | 5/10 → 9/10
Pass 6 (Responsive)  | 4/10 → 9/10
Pass 7 (Decisions)   | 2 resolved, 0 deferred
Approved mockups     | 8 generated, 2 approved, 2 regenerated as build references
Decisions made       | 17 (M3-D33 to M3-D49)
Overall design score | 3/10 → 9/10
```

**Unresolved (low stakes, settle at build time):**
- wording for secondary Source states: unborn branch, detached HEAD, History empty or failed, the dirty-switch refusal, conflicts, and loading;
- the log's Copy button copies all kept lines;
- Ctrl+F for "Find in log".
