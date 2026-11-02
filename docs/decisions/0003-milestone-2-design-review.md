# 0003: Milestone 2 design decisions

- **Status:** Accepted. D57 is done: `DESIGN.md` exists at the repo root.
- **Date:** 2026-11-02
- **Scope:** the Milestone 2 UI (PRD Section 11, Milestone 2, step 10):
  - project view with Chat, approvals, Changes/diff/revert;
  - Settings > AI Agents.

This file records why the M2 UI looks the way it does. It is a record, not a spec: the spec is PRD Section 11, Milestone 2, step 10. Where the two disagree, the PRD wins.

## Context

- Before the review, step 10 was five bullets naming surfaces, with no states, navigation, journey or accessibility. It rated 3/10.
- There is no DESIGN.md. `src/style.css` has a GNOME-like token set (light and dark), and M1 has Modal and ConfirmDialog.
- The review generated 12 mockups (3 per screen). Eleven passed the vision check. The owner picked one per screen on comparison boards.

## Approved mockups

The current build references are committed in [`docs/design/m2/`](../design/m2/). Where they differ from the PRD text or DESIGN.md, the text wins. All generated variants, including the superseded originals, stay in `~/.gstack/projects/altafino-crazy/designs/`.

| Screen | In the repo | Source variant | Notes |
|---|---|---|---|
| Project chat | `docs/design/m2/chat.png` | `project-chat-v2-20261102/variant-A.png` | Regenerated after DESIGN.md (D62); replaces the boxed-row original |
| Approval waiting | `docs/design/m2/approval.png` | `chat-approval-v2-20261102/variant-A.png` | Regenerated after DESIGN.md (D62); replaces the left-stripe original |
| Changes, diff, revert | `docs/design/m2/changes-revert.png` | `changes-revert-20261102/variant-B.png` | The Overview and Logs tabs are excluded; the run picker moves to the project header (D47) |
| Settings > AI Agents | `docs/design/m2/settings-agents.png` | `settings-agents-20261102/variant-A.png` | Used as is |

The owner picked the changes and settings mockups on comparison boards. Claude picked the two regenerated ones as build references; the owner delegated their review to Claude. They were checked against the running app on 2026-11-02, which led to the fixes in commit b0f937e.

## Decisions

| # | Decision | Why |
|---|---|---|
| D44 | Review all 7 dimensions, with mockups. | The plan had no UI detail. |
| D45 | No outside design voices. | The mockups were approved; the passes cover the same rules. |
| D46 | The project card opens the project view. Breadcrumb "Projects › Name"; Projects stays active in the sidebar; Esc goes back. | Trunk test: where am I, how do I get back. |
| D47 | One run picker in the project header, shared by Chat and Changes. | The chat and the diff always show the same run. |
| D48 | Empty Chat explains the two modes. Plan first is the default for a project's first run; afterwards the last mode is remembered per project. | Empty states are features; the safe default earns trust. |
| D49 | A run that doesn't succeed ends with a row giving the plain reason, whether files changed, and one next action. Raw errors go under Details. | Every ending gives a way forward. |
| D50 | In fallback mode, a "Blocked actions" block lists each blocked action with its own Allow and retry; a banner explains fallback at run start. | Otherwise Claude says "done" while nothing changed. |
| D51 | The Changes tab has specified states: no changes, live run, interrupted, binary, huge diff, pruned snapshots, revert report. | No raw git errors and no silently skipped reverts. |
| D52 | AI Agents has specified states: no CLI, unverified version, test progress and result, field errors. Chat shows "Set up Claude first" when no profile exists. | The first run leads to a working profile. |
| D53 | A Plan first run ends with a plan card offering "Make these changes" (resumes the same session) and "Revise plan". | One obvious step from plan to build. |
| D54 | A waiting approval shows amber badges on the Chat tab, project card and sidebar, plus one GNOME notification when the window isn't focused. | Approval waits don't time out, so runs must not stall unnoticed. |
| D55 | Timeline rows are flat on a rail; consecutive reads collapse; only the approval and plan cards are boxed. | Avoids the "stacked cards" app-UI pattern; important rows stand out. |
| D56 | The approval card is a `--warn-bg` surface with a full `--warn` border and an amber rail node. No left stripe. | A native GNOME warning style instead of a template tell. |
| D57 | Write a DESIGN.md with `/design-consultation` before building the M2 UI. Components use its tokens only. | The owner preferred a durable design system over a one-off token mapping. |
| D58 | Minimum window 760×560. Below 1100px the sidebar is an icon rail; below 900px the Changes file list becomes a dropdown and the composer wraps. | Half-screen tiling beside an editor is common. |
| D59 | Accessibility: the timeline is `role=log`; status is announced politely and approvals assertively. No focus stealing; Alt+A focuses the approval; no single-key or Enter-default Allow; Ctrl+Enter runs. | Approvals must be accessible and never accidental. |
| D60 | A safe Markdown subset (markdown-it, `html:false`, no images; links open in the system browser on click), with an injection test. | Plans need structure; agent output is untrusted (PRD 7.9). |
| D61 | Cancel is instant, with no dialog; the final row offers Review changes. | Stopping is an emergency control; revert keeps its per-file checklist (D37). |
| D62 | Regenerate the chat and approval mockups after DESIGN.md, not now. | Avoids redoing images before the design system settles. |

## Not in scope

- Extra project tabs (Files, Commits, Runs, Logs, Overview) from the mockups: M2 has Chat and Changes only.
- A sidebar model or "Connected" chip, and attachments or extra context in the composer: not requested; M3 or later.
- A side-by-side diff: unified only for M2.
- Multiple agent profiles in the composer beyond the profile override: one Claude profile is the M2 path.
- Token-level streaming: rejected in the eng review (D29).

## What already exists

- `src/style.css` tokens: `--bg`, `--card`, `--text`, `--muted`, `--border`, `--accent(-text)`, `--ok/--warn/--danger(-bg)`, `--info-bg`, `--radius`, `--space`, `--font`, `--mono`, in light and dark. These are the seed for DESIGN.md.
- `Modal.vue` (focus trap, Esc) and `ConfirmDialog.vue` (reuse for the Revert dialog), `ProjectCard.vue` (gains Open and the approval badge), Pinia stores, and the nav store (gains a project view).
- Backend from T9:
  - commands for profiles, runs, approvals, allow-and-retry, sessions, events, diff, revert and snapshots;
  - the `agent-run-event` event.

## Completion summary

```
System audit         | no DESIGN.md; UI scope: 4 surfaces
Step 0               | 3/10; all 7 dimensions
Pass 1 (Info Arch)   | 4/10 → 8/10
Pass 2 (States)      | 2/10 → 9/10
Pass 3 (Journey)     | 4/10 → 9/10
Pass 4 (AI Slop)     | 6/10 → 9/10
Pass 5 (Design Sys)  | 5/10 → 9/10 (DESIGN.md written, D57)
Pass 6 (Responsive)  | 3/10 → 9/10
Pass 7 (Decisions)   | 2 resolved, 0 deferred
Approved mockups     | 12 generated, 4 approved
Decisions made       | 17 added to the plan (D46–D62)
Overall design score | 2/10 → 8/10 (lowest pass: Info Arch)
```

**Unresolved:** none.

## Follow-up (2026-11-02)

- `DESIGN.md` was written directly, from `src/style.css` and these decisions, because `/design-consultation` is disabled for model invocation on this machine. It adds two tokens for WCAG contrast: `--accent-strong` (filled buttons; white text was 3.8:1 on `--accent`) and `--border-strong` (form-control edges; `--border` was 1.4:1). It also picks lucide icons and markdown-it.
- D62 mockups were regenerated against DESIGN.md: `project-chat-v2-20261102/variant-A.png` and `chat-approval-v2-20261102/variant-A.png` are the build reference and replace the boxed and striped originals.
