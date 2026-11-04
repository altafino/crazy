# 0007: Milestone 4 design decisions

- **Status:** Accepted
- **Date:** 2026-11-03
- **Scope:** the Milestone 4 UI: the Releases tab, the Installed Apps view, the Chat end row, the composer, and the project card.

This file records why the M4 UI looks the way it does. It is a record, not a spec: the spec is PRD Section 11, Milestone 4, step 7, and `DESIGN.md`. Where the two disagree, the PRD wins. Decision IDs continue from the engineering review ([0006](0006-milestone-4-eng-review.md)); DR1 (review all 7 areas) and DR2 (no outside design voices) were process choices.

## Context

- Before the review the plan listed buttons and release states only; it rated 3/10.
- `DESIGN.md` and the M2/M3 components exist, so every decision is calibrated against them.
- The review generated 6 mockups; the owner picked one per screen on comparison boards. They were not regenerated (M4-DR24): the text wins where they differ.

## Approved mockups

| Screen | In the repo | Notes |
|---|---|---|
| Releases tab | `docs/design/m4/releases.png` | The path stays in the title row (M4-DR14), Unverified is amber (M4-DR15), plus the tab caption (M4-DR3) and the notes caption (M4-DR21). Drawn oversized; tokens win |
| Installed Apps | `docs/design/m4/installed-apps.png` | Drawn oversized; tokens win |

## Decisions

| # | Decision | Why |
|---|---|---|
| M4-DR3 | A fifth tab "Releases" after Preview, captioned "Builds of this app and what GNOME launches"; no run picker. | Matches the mockup and the M3 caption rule. |
| M4-DR4 | While building: status row with elapsed time and Stop, stepper "Dependencies — Compile — Package — Test launch" (warns the app opens for 5 s), log view; the log collapses afterwards. | Reuses the Preview pattern; no surprise window. |
| M4-DR5 | One sentence and one primary action per state: "Build first release…", "Install 1.0.0", or Launch + "Build new release…". | One obvious next step. |
| M4-DR6 | "Build new release…" opens a dialog: version (pre-filled, "Highest so far"), field errors, amber note for uncommitted changes, "Build <version>". | Confirm the two facts that matter before minutes of building. |
| M4-DR7 | A failed build uses the M3 `.callout.danger` with "Ask Claude to fix" or "Open Doctor", and says "GNOME still starts <active>". | Specific cause, and the installed app is safe. |
| M4-DR8 | A crashed test launch shows the crash line; that row offers Details and Delete only; Unverified installs need "Install anyway". | Explains the install gate where the user looks. |
| M4-DR9 | Empty Installed Apps: how apps get here + "Go to Projects". | Empty states give context and one action. |
| M4-DR10 | Install, Activate and Roll back act at once with a toast and Undo; the first install mentions the Activities delay. | Switching is instant and reversible. |
| M4-DR11 | Removed builds hide behind "Show N removed builds". | The table shows only what you can act on. |
| M4-DR12 | A newer tested build makes "Install 1.2.0" the primary action. | Build → install is the core loop. |
| M4-DR13 | The Chat end row gains "Build release…", opening the build dialog. | Closes the change → ship loop. |
| M4-DR14 | Releases keeps the shared project header (path in the title row). | No header jump between tabs. |
| M4-DR15 | Test badges: Passed ok, Failed danger, Unverified warn (both screens); Active uses the info badge. | No new grey; amber means "look at this". |
| M4-DR16 | DESIGN.md gains "New in Milestone 4": release table, app list row, test badge mapping. | Builders follow specs, not oversized pictures. |
| M4-DR17 | Below 900 px, releases become two-line rows with the main action visible and a ⋯ menu. | Fits the 760 px minimum window. |
| M4-DR18 | Accessibility follows DESIGN.md's general rules only (owner's choice); the screen-specific spec is in `TODOS.md` (M4-DR25). | Owner's scope call. |
| M4-DR19 | A missing launcher entry shows "Launcher missing" with Repair. | One-click recovery using the install steps. |
| M4-DR20 | ProjectCard shows "Installed <v>" or "Building…". | Install state where projects are chosen. |
| M4-DR21 | The build dialog has optional notes, shown under the version. | Readable rollback choices (F11 notes). |
| M4-DR22 | Delete opens a named ConfirmDialog stating the size and that it can't be reinstalled without rebuilding. | The removal is irreversible. |
| M4-DR23 | While building, Make changes is disabled with the reason in the status strip; Build is disabled while a Make-changes run is live. | The reason shows before the click. |
| M4-DR24 | The picked mockups are kept, not regenerated. | Text overrides are small and listed. |

## Not in scope

- Screen-specific screen-reader spec (M4-DR18, tracked in `TODOS.md`).
- Restore source UI (M4-D1).
- Dark-theme mockups: tokens cover dark; check with `/design-review` after the build.

## Completion summary

```
System audit         | DESIGN.md present; UI scope: Releases tab, Installed Apps, chat end row, composer, card
Step 0               | 3/10; all 7 areas
Pass 1 (Info Arch)   | 4/10 → 8/10
Pass 2 (States)      | 2/10 → 9/10
Pass 3 (Journey)     | 4/10 → 9/10
Pass 4 (AI Slop)     | 8/10 → 8/10
Pass 5 (Design Sys)  | 5/10 → 9/10
Pass 6 (Responsive)  | 3/10 → 6/10 (accessibility spec declined)
Pass 7 (Decisions)   | 5 resolved, 0 deferred
Approved mockups     | 6 generated, 2 approved
Decisions made       | 21 (M4-DR3 to M4-DR23)
Overall design score | 2/10 → 6/10
```
