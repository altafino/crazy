---
name: AI App Studio
version: 1
mode: operate   # app UI: people finish tasks; native expectations beat expression
colors:
  light:
    bg: "#fafafa"          # window background
    sidebar: "#ebebeb"
    card: "#ffffff"        # raised surfaces: cards, dialogs, inputs on cards
    text: "#241f31"        # 15.3:1 on bg
    muted: "#5e5c64"       # secondary text, 6.3:1 on bg
    border: "#d8d8d8"      # decorative dividers and card edges only
    border-strong: "#8f8f8f"  # form-control edges, rail nodes: 3.1:1 (WCAG 1.4.11)
    accent: "#3584e4"      # focus rings, selection, active tab underline, rail "running"
    accent-strong: "#1c71d8"  # filled primary buttons: white text 4.8:1
    accent-text: "#1c71d8" # links and accent-colored text, 4.6:1 on bg
    on-accent: "#ffffff"
    ok: "#1e7a3c"
    ok-bg: "#e1f3e6"
    warn: "#8a5a00"
    warn-bg: "#fdf1d4"
    danger: "#c01c28"
    danger-bg: "#fbe4e5"
    danger-strong: "#c01c28"
    info-bg: "#e3edfb"
  dark:
    bg: "#242424"
    sidebar: "#303030"
    card: "#323232"
    text: "#f6f5f4"
    muted: "#b8b6bd"
    border: "#4a4a4a"
    border-strong: "#7a7a7a"
    accent: "#3584e4"
    accent-strong: "#1c71d8"
    accent-text: "#78aeed"
    on-accent: "#ffffff"
    ok: "#8ff0a4"
    ok-bg: "#1f3a28"
    warn: "#f8e45c"
    warn-bg: "#433a14"
    danger: "#ff938c"
    danger-bg: "#4a2427"
    danger-strong: "#c01c28"
    info-bg: "#25384f"
typography:
  font: '"Adwaita Sans", Cantarell, system-ui, sans-serif'   # GNOME 48+ default; Cantarell before that
  mono: '"Source Code Pro", ui-monospace, "DejaVu Sans Mono", monospace'
  title:   { size: 24px, weight: 700, line: 1.25 }   # view and project names (h1)
  heading: { size: 18px, weight: 700, line: 1.3 }    # section headings (h2)
  body:    { size: 15px, weight: 400, line: 1.5 }    # GNOME's 11pt default
  label:   { size: 15px, weight: 600, line: 1.5 }    # form labels, row titles
  caption: { size: 13px, weight: 400, line: 1.4 }    # timestamps, hints, counts
  code:    { size: 13.5px, line: 1.5 }               # mono: commands, paths, diffs
spacing: [4, 8, 12, 16, 24, 32]   # px; --space = 16
rounded:
  sm: 6px      # chips inside text, diff hunk headers
  md: 8px      # buttons, inputs, segmented control
  lg: 10px     # cards, dialogs, approval and plan cards (--radius)
  pill: 999px  # badges
motion:
  duration: 150ms
  easing: cubic-bezier(0.2, 0, 0, 1)   # ease-out; no bounce, no spring
  reduced: respect prefers-reduced-motion (durations go to 0, spinner becomes static dots)
icons:
  set: lucide-vue-next
  size: 16px (rail nodes 14px, sidebar 18px)
  stroke: 1.75
  color: currentColor
---

# AI App Studio design system

Studio is a GNOME desktop tool for building apps with an AI agent. It should feel like a calm, native
GNOME app (libadwaita, GNOME Builder), not a web dashboard. Everything on screen is there to help you
direct the agent, see what it did, and stay in control.

The tokens above live in `src/style.css` as CSS custom properties (`--bg`, `--border-strong`, …).
Components use `var(--…)` only: **no hex values, font sizes or radii hard-coded in components.**
Light and dark come from `prefers-color-scheme`, matching the GNOME setting.

## Principles

1. **Native first.** Follow GNOME conventions: header with title, sidebar navigation, grouped forms,
   amber for warnings, red only for destructive actions. The face is GNOME's own: Adwaita Sans
   (GNOME 48+), Cantarell on older desktops.
2. **Control is visible.** Anything the agent wants to do that changes your machine is shown in full
   (the exact command or edit) before it happens, and nothing is approved by accident.
3. **Layout, not cards.** Use alignment, spacing and dividers to structure screens. A card is used only
   when the card *is* the interaction: a project on the dashboard, an approval, a plan, a dialog.
4. **One accent.** Blue marks selection, focus and the primary action. Amber means "needs you". Green
   and red appear only for diff lines and outcomes. No other colors.
5. **Quiet by default.** Routine steps (reads, searches) are one muted line. Emphasis is reserved for
   what needs a decision or summarizes an outcome.

## Layout

- **Window.** Minimum 760×560. Sidebar 220px on `--sidebar`, main area on `--bg` with 24px/32px padding.
- **Below 1100px**, the sidebar becomes a 56px icon rail: 18px icons, tooltip and `aria-label` per item.
- **Below 900px**, two-column views stack. For example, the Changes file list becomes a dropdown above the diff.
- **Project view.** The breadcrumb "Projects › Name" sits above the title row. The title row holds the
  name (title style), the path (caption, mono, muted), the template badge and the run picker on the right.
  Below it, the tabs, then the tab content filling the remaining height.
- **Chat tab.** The timeline scrolls, and the composer is docked to the bottom. The timeline content is
  at most 860px wide, left-aligned with the tabs.
- Vertical rhythm: 24px between sections, 16px between a heading and its content, 8px inside groups.
  There is more space above a heading than below it.

## Typography

- Headings are left-aligned. Nothing is centered except empty-state blocks and dialog icons.
- Paths, commands, model names and diff content use `--mono`. Prose never uses mono as decoration.
- Numbers in counts (+24 −2, durations) use `font-variant-numeric: tabular-nums`.
- Body text is 15px (GNOME's 11pt), deliberately below the web's 16px. The contrast of every text token
  is at least 4.5:1, as measured in the token comments.

## Components

### Existing (M1)

| Component | Spec |
|---|---|
| Button (`.btn`) | 8px radius, 6px 14px padding, `--card` bg, 1px `--border-strong`. Hover: `--accent` border. **Primary:** `--accent-strong` bg, `--on-accent` text. **Destructive:** `--danger` text and border on `--card`; filled `--danger-strong` bg with `--on-accent` text (the dark `--danger` is too light for white text), only inside a confirmation dialog. `.small` for inline actions. |
| Input (`.input`) | 8px radius, 1px `--border-strong`, `--bg` fill. The label is always visible above it; a placeholder is an example, never the label. Invalid: `--danger` border plus `.field-error` text below. |
| Badge (`.badge`) | Pill, caption size, weight 600. Variants: `ok`, `warn`, `danger`, info (default). |
| Callout (`.callout`) | 10px radius, `--info-bg`; `.warn` and `.danger` variants. One sentence and at most one action. |
| Modal / ConfirmDialog | `--card`, 10px radius, focus trap, Esc closes, max 480px wide. The title says what will happen ("Revert this run?"); the confirm button names the action ("Revert 3 files"). |
| ProjectCard | Clickable as a whole (opens the project). Its secondary buttons stop propagation. An amber "Needs approval" badge appears while a run waits. |

### New in Milestone 2

| Component | Spec |
|---|---|
| Breadcrumb | Caption size, `--accent-text` link "Projects", then `›` and the current name in `--muted`. Esc triggers it. |
| Tabs | Text tabs with a 2px `--accent` underline and `--accent-text` on the active tab, `--muted` on the others. An optional count badge or amber "Needs approval" badge follows the label. ARIA tabs pattern, arrow keys move between tabs. |
| Run picker | A native `<select>` styled as an input: "Today 14:32 · Make changes · Succeeded". The newest run is first. |
| Segmented control | Two options ("Plan first", "Make changes") in an 8px-radius group with a 1px `--border-strong` edge. The selected option is filled `--accent-strong` with `--on-accent` text. `role="radiogroup"`, arrow keys switch. |
| Timeline | `role="log"`. A 2px `--border` rail at 12px from the left edge. Each row has a 24px node on the rail (a circle with `--card` fill and a 1px `--border-strong` edge, holding a 14px `--muted` icon) and content starting 40px from the left edge. **Node states:** running uses an `--accent` edge and a slow spinner; waiting uses a `--warn` fill; failed uses a `--danger` icon. |
| Timeline rows | **Prompt:** your text in label weight. **Agent text:** body paragraphs (safe Markdown subset). **Tool step:** one line, 32px min height: icon, verb, `--mono` path, tabular +/- counts in `--ok`/`--danger`, caption timestamp right-aligned; it expands inline to show the input or result in a `--mono` block on `--sidebar` bg. Consecutive reads collapse into "Read 6 files". **Final row:** the outcome sentence, a files-changed note, and one action button. |
| Approval card | `--warn-bg` surface, 1px `--warn` border, 10px radius, 16px padding. The title has a warning icon and states the request ("Claude wants to run a command"). The full command or edit appears in a `--mono` block on `--card`, followed by the description and the working directory (caption). Actions on the right: Deny, then Allow once (primary). "Deny with a note…" is a link on the left. No default button; Enter does nothing. |
| Plan card | `--card` surface, 1px `--border`, 10px radius, 16px padding. The title is "Plan", followed by the Markdown body. Actions: Revise plan, then Make these changes (primary). |
| Status strip | One line above the composer: a 8px state dot (accent, warn or ok), the state text, and the elapsed time or "Inactivity timer is paused" in caption. `aria-live="polite"`. |
| Composer | `--card` surface with a 1px `--border` top edge. A textarea (3 lines, grows to 10) with a visible label for screen readers ("Describe what to change"). The controls row holds the segmented control, the profile select, and Run (primary) or Cancel (secondary). Ctrl+Enter runs. |
| Diff view | Unified, `--mono` code size. A 48px line-number gutter in `--muted`. Added lines on `--ok-bg` with a `+` marker, removed lines on `--danger-bg` with `−`. Hunk headers in caption on `--sidebar`. The file header shows the path, then +/- right-aligned. |
| File list | Rows of status letter (M/A/D in `--accent-text`/`--ok`/`--danger`), path (`--mono`, middle-truncated with a full-path tooltip), and +/- counts. The selected row has an `--info-bg` fill. |
| Env editor | Rows of key input, `=`, value input, and a remove icon button. "Add variable" is a small button below. A hint below in caption: "Secret-looking variables (tokens, keys) are refused; sign in through the Claude CLI instead." |
| Icon rail sidebar | 56px wide; 40px square hit targets; the active item has an `--info-bg` fill. |

### New in Milestone 3

| Component | Spec |
|---|---|
| Toast | Bottom-center, 16px above the window edge, max 480px wide. `--card` surface, 1px `--border`, 10px radius, body text, an optional action link in `--accent-text` ("Undo"). Shown 5 s, paused on hover or focus; one at a time (a new one replaces the old). `aria-live="polite"`. Never takes focus. |
| Phase stepper | One line in caption size: "Dependencies — Compile — App". Each step has a 14px lucide icon: check in `--ok`, x in `--danger`, spinner in `--accent` (static dots with reduced motion), pending in `--muted`. The separators are "—" in `--muted`. |
| Log view | `--card` surface, 1px `--border`, 10px radius, `--mono` at code size, 48px `--muted` line-number gutter. Error lines in `--danger`, warnings in `--warn`, cargo progress lines in `--muted`. Toolbar: caption "Log · last N lines", a "Find in log" field (matches highlighted with `--info-bg`), a "Follow output" switch, and a Copy icon button. `role="log"`, not live (the status row announces changes). |
| Switch | 40×22 track: off `--border-strong`, on `--accent-strong`, white knob. Always a visible label. `role="switch"`, Space toggles. |
| Branch control | An input-styled button: git-branch icon, branch name, chevron. Its menu starts with "New branch…", then the local branches with the current one checked; a filter field appears when there are more than 10. Detached HEAD shows "Detached at <hash>". |
| Commit box | Docked under the Source file list. Visible label "Commit message" above a 3-line textarea. "Restore…" is a destructive text link on the left; "Commit N files" (primary) is on the right. Ctrl+Enter commits. |
| Checkbox file list | The M2 File list plus a 16px leading checkbox per row; the header shows "N changed files" and "Select all". Keyboard: a multi-select listbox with one Tab stop; ↑/↓ move and show the diff, Space ticks, Ctrl+A ticks all. Conflict rows carry an amber "conflict" badge. |
| Error callout (build) | `.callout.danger`: `--danger-bg`, 1px `--danger` border on all sides, 10px radius, an alert-circle icon, the first error line in `--mono`, and one secondary action on the right ("Ask Claude to fix"). No left stripe. |
| Preview tab dot | 8px dot after the "Preview" tab label: `--accent` while starting or compiling, `--ok` when the app runs, `--danger` when the build failed; no dot when idle. The tab's accessible name includes the state ("Preview, running"). |

### New in Milestone 4

| Component | Spec |
|---|---|
| Release table | A semantic `<table>`, no cards. Header row in caption size and `--muted`. 48px rows separated by 1px `--border` dividers. Version and commit hash in `--mono`; dates and sizes `tabular-nums`. A release's notes appear as a one-line `--muted` caption under the version. One right-aligned action column with `.btn.small` (the danger variant for Delete). Below 900px: two-line rows (version, then a "Built · Source" caption), the badges, the row's main action, and a ⋯ menu for the rest. |
| Test badge | Passed `.badge.ok`, Failed `.badge.danger`, Unverified `.badge.warn`. The "Active" status uses the default info badge; "Previous" and "Built" are plain `--muted` text. Never a grey badge. |
| App list row | 64px rows with 1px `--border` dividers. A 40px app icon (the app's own), the name in label weight, and a `--muted` caption ("Active 1.1.0 · installed today 14:52 · 3 releases kept"). Right-aligned: an "Open project" link in `--accent-text`, Launch (`.btn.small`), and a ⋯ icon button with a menu (Show in Files, Uninstall…). Warning badges follow the caption ("Unverified release" warn, "Launcher missing" danger). |
| Releases status row | The Preview status-row pattern: one sentence in label weight, a caption below, and actions on the right with exactly one primary. A build adds the phase stepper ("Dependencies — Compile — Package — Test launch") and the log view; a failure adds `.callout.danger`. |

### New in Milestone 5

| Component | Spec |
|---|---|
| Sync status row | The Releases status row pattern: one sentence in label weight ("2 commits to push to GitHub"), a caption below ("owner/repo · private · branch main · fetched 3 min ago"), and actions on the right with exactly one primary. While an operation runs the sentence reads "Pushing… 4 s", Stop is the only enabled button, and the others are disabled. A failed last action adds `.callout.danger` under the row with the reason and one fix button; the raw redacted text sits under a Details toggle. A blocked action is disabled with a `--muted` caption saying why, tied to the button with `aria-describedby`. A footer line in caption reads "Signed in as <login>" with an "Open on GitHub" link in `--accent-text`. Below 900px the buttons move under the sentence. |
| Commit list | A semantic list under a label-weight heading ("Not on GitHub yet" or "Incoming from GitHub"). 40px rows separated by 1px `--border` dividers: the short hash in `--mono`, the subject (single line, ellipsis, full text in a tooltip), and the time right-aligned in caption, `--muted`, `tabular-nums`. No avatars, no cards. Below 900px the time moves into a caption line under the subject. |
| Tab sync count | Caption size, `--muted`, `tabular-nums`, one space after the "GitHub" tab label: "↑2", "↓3" or "↑2 ↓1"; absent when nothing is pending or there is no remote. While signing in, an 8px `--accent` dot replaces it. The tab's accessible name spells it out ("GitHub, 2 to push, 1 to pull"). |
| Sign-in code | Inside the status row, on its own line: the one-time code in `--mono` at 18px, selectable, in a `--card` box with a 1px `--border` and 8px radius, with a visible label "Code" for screen readers. Beside it Open GitHub (primary), Copy code (secondary, announces "Code copied") and Stop, then an elapsed caption in `--muted`. The row flips to signed in without a click. |
| Visibility choice | Two radios in a `fieldset` with the legend "Visibility": "Private" (default) and "Public". Choosing Public shows a `.callout.warn` ("Anyone on the internet can see this code") and a required checkbox "I understand this repository will be public". The dialog's primary button reads "Create repository" and becomes "Create public repository", disabled until the box is ticked. |
| Merge callout | `.callout.warn`: `--warn-bg`, 1px `--warn` border on all sides, 10px radius, an alert-triangle icon, the sentence "Merge in progress: N files need your attention" in label weight. On the right: "Abort merge" (secondary) and an "Ask Claude to resolve" link in `--accent-text`. No left stripe. It sits above the file list in Source and stays visible until the merge is committed or aborted. |
| Conflict row | The M3 checkbox file list row without the checkbox: the amber "conflict" badge, the path in `--mono` (middle-truncated, full path in a tooltip), and a "Mark resolved" `.btn.small` on the right whose accessible name includes the file ("Mark src/App.vue resolved"). A resolved row swaps the badge for a `--ok` check and "Resolved". In the diff, conflict marker lines (`<<<<<<<`, `=======`, `>>>>>>>`) sit on `--warn-bg` in `--warn`. The commit button above reads "Commit merge" and stays disabled until no row is in conflict. |

### New: Chat attachments

| Component | Spec |
|---|---|
| Attachment chip | 28px tall, 8px radius, 1px `--border` on `--card`, 4px 8px padding, caption size. A 24px preview (4px radius) for images, else a 16px lucide type icon; the name (middle-truncated at 24 characters, full name in a tooltip), then a `--muted` detail: size in `tabular-nums` ("1.2 MB"), "Adding…" or "Converting…" with a 14px spinner, "converted · 2 images" for Word files, or "File removed" for a pruned timeline chip (no preview, not clickable). A PDF shows its size, never a page count. The remove × is a 20px icon button with a 28px hit area, named "Remove <name>"; timeline chips have no ×. Error variant: `--danger-bg`, 1px `--danger` border, "name · reason" in `--danger`. Image chips are buttons named "Preview <name>" that open `Modal.vue` (image fitted, file name as title, "Open in default app"); other chips are buttons named "Open <name>" that open the file in the system app. Accessible names: "screenshot.png, 1.2 MB", "requirements.docx, converting", "notes.xlsx, can't attach: <reason>". |
| Chip area | Above the composer textarea and under a prompt in the timeline. `role="list"` labelled "Attached files"; chips 6px apart, wrapping to at most 2 rows, then the area scrolls. Tab reaches each chip's ×; Delete or Backspace on a focused chip removes it and moves focus to the next chip, or to the textarea after the last one. The status strip announces "<name> attached", "<name> converted" and "<name> can't attach: <reason>". |
| Error reasons | Exact text: "This file type isn't supported yet", "Too large: images up to 20 MB" (per type and current limit: images, PDFs, text files, Word files), "Secret-looking files can't be attached", "Install pandoc to attach Word files", "Conversion took too long", "Couldn't copy this file", "Limit: 10 files per prompt". An error chip blocks Run: the caption beside Run (tied with `aria-describedby`) reads "Remove 1 file that can't be attached" (plural "Remove 2 files…"); while a chip is adding or converting it reads "Waiting for 1 file to finish converting". Error wins over waiting. |
| Drop overlay | While files hover the Chat tab: `--info-bg` at 85% opacity over the whole chat panel, 2px dashed `--accent` outline, 10px radius, "Drop files to attach" centered in label weight. During a running Make-changes run it reads "Attach files after this run finishes" and a drop attaches nothing. `aria-hidden` (visual only); other tabs ignore drops. |
| Attach button | `.btn` (secondary) with a 16px paperclip icon and "Attach files", first in the composer controls row. Below 900px it becomes a 32px paperclip icon button (tooltip and `aria-label` "Attach files"), and Run's reason caption moves to its own line above the controls row. Ctrl+V in the textarea pastes an image. |
| Settings > Attachments | The `PreviewLimits.vue` pattern: one number field per limit with a visible label and its range as a `--muted` hint; invalid values refused inline. |

As built: chips use `0 8px` padding so the 24px preview fits the 28px height; Word chips read "converted" without an image count; three more error texts exist: "Too large: N MB per prompt in total", "Couldn't convert this file" and "This file is no longer there" (decisions record 0010).

### New: App databases

| Component | Spec |
|---|---|
| Database tab | The seventh project tab, after GitHub, on every project. Top to bottom: a status row, then the tables list beside the rows. View only: Studio never writes to an app's database; changes go through Claude. |
| Database status row | The Sync status row pattern. The sentence in label weight ("Preview data · SQLite · 3 tables · 148 KB"); a caption below with the path in `--mono` and "schema version 4 · updated 2 min ago", followed by a 32px "Refresh data" icon button. On the right: the segmented control (Preview data / Installed app data, Preview by default), **Back up…** (the one primary, `--accent-strong`) and a ⋯ icon button (the Release table menu-button pattern, a native `<details>`) holding "Copy installed data to Preview…" and "Reset preview data…" (`--danger` text, Preview only). While an action runs the primary reads "Backing up…" / "Copying…" / "Deleting…" and the others are disabled. Copy, Reset and Restore are disabled while the app using that file runs, with a `--muted` caption ("Stop Preview first", "Quit Pac-Muncher first") tied to them with `aria-describedby`. Polite announcements go through a visually hidden `role="status"` in the row ("Showing installed app data", "Backed up preview data"); focus never moves on its own. Below 900px the controls move under the sentence. |
| Tables list | A single-select listbox, one Tab stop, 220px wide, 40px options: the table name in `--mono`, the row count right-aligned in `tabular-nums`. ↑/↓ (and Home/End) move and show the table. Options read "scores, 42 rows" ("many rows" past the query limit). The selected option is filled `--accent-strong`. Below 900px it is replaced by a "Table" select whose options read "scores · 42 rows". |
| Data table | A semantic `<table>` with a visually hidden `<caption>` ("scores, 42 rows, showing 1 to 50") and `<th scope="col">`. 40px rows, 1px `--border` dividers. Cells use the body font. Numbers are right-aligned with `tabular-nums`. `--mono` is only for table names, column names and the file path. Column headers in caption size, `--muted`, sticky. NULL is a `--muted` italic "NULL", a BLOB is a `--muted` "BLOB · 2.1 KB" (bytes are never sent). Long text is ellipsized with the full text in the DOM and a tooltip capped at 2 000 characters. The table scrolls sideways inside its own area only; the window never does. 50 rows per page. |
| Rows pager | Under the table: "Rows 51–100 of 4 210" (`--muted`, `tabular-nums`), then "Previous page" and "Next" (`.btn.small`; the Next button's name is "Next page, rows 51 to 100"). With a counting timeout it reads "of many". |
| Rows filter | The Log view "Find in log" field pattern with the accessible name "Filter rows", debounced 300 ms. Over the query limit: "Filter took too long; narrow it". Esc clears it. |
| Backups dialog | `Modal.vue` titled "Backups of Preview data" (or "…Installed app data"). "Back up now" (primary) on top, then a list: label or date, reason, size, and Restore… (plus Delete… for manual and "before restore" backups; automatic release backups follow their release). Restore opens the ConfirmDialog "Replace installed app data with the backup from 3 Nov 14:02?" with the button "Replace data". Empty list: "No backups yet. Back up now keeps a copy you can restore later." |
| Data states | Empty or problem states use one sentence and one action, no illustration: "This app doesn't store data yet…" + Ask Claude to add a database; "No preview data yet…" + Open Preview; "Not installed…" + Open Releases; "The installed app hasn't created its data yet…" + Launch; "scores has no rows yet."; `No rows match "ada".` + Clear filter; Loading reads "Reading preview data…" (the 12px spinner only after 400 ms); unreadable files use `.callout.danger` "Studio can't read this database" + Ask Claude to fix, raw error under Details. Every "Ask Claude…" action fills the composer and runs nothing. |
| Releases schema note | A caption line under the version, "Database changes: schema 3 → 4", on a release whose schema differs from the active one's. Install/Activate then confirms with "Studio backs up the app's data first (before 1.1.0)." Rolling back to a release older than the data opens "Version 1.0.0 is older than your data (schema 4 vs 3). It may not read it correctly." with "Restore data from before 1.1.0" and "Keep current data". |
| Settings > Database tab | The `PreviewLimits.vue` pattern: one number field "Query time limit (seconds)", 0 to 60, hint "0 = no limit", default 2. |

### New: App icons

- **App icon** (`AppIcon`): the project's own icon (`src-tauri/icons/128x128.png`), shown as-is with no
  rounding or crop, 48px in the project header and 32px on project cards; decorative (`aria-hidden`),
  the project name sits next to it. Without an icon: the first letter on `{colors.info-bg}`, rounded.
- **Change app icon**: the header icon is a button ("Change app icon of <name>", tooltip "Change app
  icon…"). It opens the file picker (PNG, JPEG, WebP at least 256 px, or SVG); Studio writes every size into `src-tauri/icons`,
  so Preview, Releases and GNOME use it. Toast "App icon changed. Preview and the next release use it.";
  a refused picture (too small, not an image, run or build in progress) is a toast with the reason.
- **New Project wizard**: optional "App icon" field: Choose… opens the same picker, a 48px preview shows
  the picture as it will look, Change…/Remove; a refused picture shows its reason under the field. The
  icon is in the project's first commit.
- **Installed Apps**: each row shows the launcher's icon (the active release's), 40px, as-is.

### New: Project plan

| Component | Spec |
|---|---|
| Plan tab | The second project tab, after Chat. Its label carries a Tab sync count style "3/9" (absent without a plan; accessible name "Plan, 3 of 9 tasks done"). Top to bottom: the status row, the summary, the checklist, then one `--muted` caption "To change the plan, ask in Chat with Plan first, then press Use as project plan." The plan lives in `.ai-app-studio/plan.md`; Studio never parses tasks in the UI. |
| Plan status row | The status row pattern: "3 of 9 tasks done" in label weight, a native `<progress>` under it (6px, full row width, `--border` track, `--accent` fill, `--ok` when all done, pill ends, labelled by the sentence), caption "From .ai-app-studio/plan.md · saved 2 Nov 14:02". All done: "All 9 tasks done" with **Build release…** (the one primary). While a Make-changes run or a release build is active, a `--muted` caption "Wait until the current run finishes" / "…the release build finishes" is tied to every disabled checkbox and Start with `aria-describedby`. |
| Summary | The text above the first task: its first paragraph, then a native `<details>` "Show full summary" for the rest (safe Markdown). |
| Checklist row | `<ul aria-label="Tasks">`; 1px `--border` dividers, no cards. A native checkbox labelled by the title (body text), an optional detail line in caption `--muted` (tied with `aria-describedby`), and Start (`.btn.small`, named "Start: <title>"; primary only on the first open task) at the top right. Done rows: checked, title in `--muted`, no strikethrough, no detail, no Start. The task an active Start run works on shows an info badge "In progress" with the 12px spinner instead of Start. Titles wrap below 900px; never truncated. Ticks are announced politely ("<title> marked done, 4 of 9"); focus stays on the checkbox. |
| Plan states | Loading "Reading the plan…" (spinner after 400 ms). Empty: "No plan yet. Describe your app and Claude will break it into tasks." + labelled textarea "What should this app do?" + **Make a plan** (Chat, Plan first, runs nothing). Unreadable: `.callout.danger` "Studio can't read .ai-app-studio/plan.md" + Ask Claude to fix, reason under Details. No tasks: "This plan has no tasks yet." + Ask Claude to add tasks. A vanished task: toast "This task is no longer in the plan." and the list reloads. |
| Plan card button | "Use as project plan" (secondary) between Revise plan and Make these changes (primary), only on plans with checklist lines. Replacing a plan asks "Replace the project plan?" / "The current plan has 3 of 9 tasks done. Its ticks are replaced by this plan." / **Replace plan**. Saving keeps you in Chat: toast "Saved as the project plan (9 tasks)" with Open Plan; the card then shows a `--muted` "Saved as project plan". |
| Plan line in the run end | Runs started by Start get a second line in the final row: "From the plan: <title>" in caption `--muted`, the row's own action unchanged. After a succeeded run with the task open: `.btn.small` Mark done ("Mark <title> done"), then "Marked done · 4 of 9 tasks done" with Start next: <task> (or "All 9 tasks done" + Build release…). Failed, cancelled or interrupted: "· not done", no button. |
| Wizard field | Field 2, after Name: "What should this app do? (optional)", 3-line textarea, hint "Claude turns this into a plan you can work through. Nothing runs until you press Run." Filled: the new project opens on Chat in Plan first with the plan request. |

### New: Exports for other systems

| Component | Spec |
|---|---|
| Export entry | A visible **Export…** (`.btn.small`, never primary) on the active release row, which has no other action; **Export for other systems…** in the ⋯ menu of every kept row. Both are disabled while a build, an export or a Make-changes run is active. |
| Exports toggle | A real `<button>` under the version, caption size, accent text: "Exports · 4 files" with a chevron; `aria-expanded` and `aria-controls` name the area; focus stays on it. Absent without exports; "Exports · export failed" when only a failure exists. It opens by itself when an export of that release ends. |
| Exports area | A flat `<tr><td colspan>` under the release on `--sidebar`: no border, radius or shadow, aligned with the Version column, the table's 1px `--border` divider below. Per target a heading (label weight) "Other Linux · built on the Ubuntu 22.04 base · Today 10:41" or "Windows 11 · Today 10:52" with a `.badge.warn` "Unverified", a caption "Not started by Studio." / "Studio can't start Windows apps; try it on a Windows 11 PC.", then a `<ul>` of file lines. Ends with **Show in Files** and **Export again…** (`.btn.small`). |
| File line | Name in `--mono` (middle-truncated, full name in the tooltip), purpose in caption `--muted` (AppImage "Runs on most distros (glibc 2.35+)", deb "Ubuntu, Debian, Mint", rpm "Fedora, openSUSE", setup.exe "Installer. Unsigned: Windows shows "More info → Run anyway"."), size with tabular-nums, **Copy SHA-256** (`.btn.small`, named "Copy SHA-256 of <file>"; its text becomes "Copied" for 2 s and a polite live region says so). Below 900px: two lines, name and size, then purpose and Copy. Lines are 32px tall, no cards. |
| Failed target line | "Windows: export failed · Today 10:52" in caption `--danger`, in place of that target's files; a failed retry keeps the older files above it. |
| Export dialog | `Modal.vue` ≤ 480px titled "Export <Name> <version>". Two native checkboxes ("Other Linux", "Windows 11") with a caption each, tied with `aria-describedby`, preset from the last choice (both on first use); "Replaces the current <target> export when it finishes." for targets that already have files. While Windows is ticked and the license is not accepted: an unticked required checkbox "I accept Microsoft's license for the Windows build tools (Visual C++ runtime and Windows SDK)" and a "Read the license" link that opens externally. First export: a caption about the ~3 GB download. Focus starts on the first checkbox. **Cancel** and **Export** (primary, disabled without a target, with the license unticked, or while Docker is not ready). |
| Docker check | Runs when the dialog opens (a 12px spinner and "Checking Docker…" only after 400 ms). Not ready: a `.callout.warn` with `role="alert"`, the case's sentence and fix ("Docker isn't running. Start it with: `systemctl start docker`" / "Docker is installed, but your user can't use it. Add yourself to the docker group, then log out and back in." / "Exports need Docker. Install the docker package.") and **Open Doctor**. Never `.danger`. |
| Export status row | The Releases status row: "Exporting Notes 1.2.0 for Other Linux and Windows · 3m 12s" (only the chosen targets), the stepper Environment — Dependencies — Linux — Windows (Environment only on the first export), a first-run caption, the log and **Stop**; "Build new release…" is disabled with the caption "You can build when the export ends." Progress is announced politely. Stopped: "The last export was stopped." |
| Export failure | One `.callout.danger` per failed target ("Windows export failed" + the first error line + Details) with **Ask Claude to fix** (the prompt names the target, quotes the first error lines and says the fix is checked by exporting again) or, for a setup error, **Open Doctor**. Kept files of the other target stay. |
| Export done | The status row returns to idle, the release's exports area opens and a toast "Notes 1.2.0 exported for Other Linux and Windows" offers **Show in Files**. |
| Settings > General > Exports | A card like the other limits cards: "Export caches use 3.2 GB" ("Calculating…" while loading, "No export caches" at 0, "Couldn't read Docker volumes" + Details on error), **Clear export caches…** → ConfirmDialog "Clear export caches?" with the button "Clear 3.2 GB" (a refusal while an export runs is a toast: "Wait until the export finishes"), and "Microsoft license for Windows builds: accepted 1 Nov 2026" with a **Withdraw** link, or "not accepted". |
| Doctor > Optional tools | An `<h2>` "Optional tools" under the main table, the same table markup, one Docker row: `.badge.ok` "Installed" with the version, or `.badge.warn` "Not installed" / "Not running" / "No permission" with the fix in Details; never `.danger`. Caption "Needed only for exporting apps for other systems". |

### New: ArtCraft apps

| Component | Spec |
|---|---|
| Popover | A small non-modal panel that opens **above** its button and holds a few settings that save as you change them (no Save or Cancel). 360px wide; below 900px min(360px, window width minus 32px). `--card` surface, 1px `--border`, 10px radius, 16px padding, the ⋮ menu's shadow (`0 4px 16px rgb(0 0 0 / 0.2)`), and a caret pointing at the button. The content scrolls inside it at a max height of 60vh; the window never scrolls for it. `role="dialog"` with an `aria-label`; the button has `aria-haspopup="dialog"` and `aria-expanded`. Focus goes to the first control on open. Esc or a click outside closes it and focus returns to the button. |
| Composer ArtCraft button | A `.btn` (secondary) after the segmented control in the composer controls row: a 16px lucide `shapes` icon, "ArtCraft" and, when on, " · 3" in `tabular-nums`; no chevron. Hidden until detection finishes and when no supported app is installed (Doctor is where users learn about ArtCraft). A ticked app that is missing adds an amber `triangle-alert` icon and the name "ArtCraft apps, 1 not found". Disabled during a Make-changes run with the caption "Wait until the current run finishes". Below 900px a 32px icon button with the count after it; tooltip and `aria-label` "ArtCraft apps, 3 on" / "ArtCraft apps, off" / "ArtCraft apps, 1 not found". |
| ArtCraft popover content | Titled "ArtCraft apps for <project>" (`aria-label`). Top to bottom: the switch "Use ArtCraft apps in this project"; a `fieldset` "Apps Claude can use" of native checkbox rows (40px: name in label weight, caption `--muted` "<purpose> · <Kind> <version>", plus " · asks before each action" for apps without a write root; a missing ticked app keeps its row with `.badge.warn` "Not found" and "Reinstall it or untick it."); "Apps not installed", one row each without a checkbox and a "Get it" link in `--accent-text` that opens getartcraft.com/apps externally; a `--muted` line "Found but not usable yet: <apps>"; and a `--muted` caption "Claude saves what it makes in assets/. Studio's undo doesn't cover files over 50 MB or files git ignores." with a "Details in Doctor" link. With the switch off the list is greyed but visible; unticking the last app turns the switch off. Every change saves at once, announced politely ("PhotoCraft on, 3 apps"); a failed save is the toast "Couldn't save ArtCraft apps" with Details. Captions are tied with `aria-describedby`; "Not found" is also in the row's accessible name. |
| Doctor > ArtCraft apps | An `<h2>` "ArtCraft apps" under Optional tools, the same table markup (`caption.sr-only`, `th scope="row"`), one row per supported app: `.badge.ok` "Installed" or `.badge.warn` "Not wired", never `.danger`. The install Studio uses ("Flatpak 0.21.0"), with other installs as an "Also: AppImage 0.7.0" caption; Details say "Asks before each action" where it applies. Captions below the table: the undo note and "More apps at getartcraft.com/apps". While checking, one "Checking…" row. |

## States

Every view specifies its empty, in-progress, problem and done states (PRD M2 step 10, "States").
The rules that apply to all of them:
- **Empty:** a one-line explanation plus the one action that fills it. No illustrations.
- **In progress:** say what is happening and how long it has been ("Running · 1m 12s"). Spinners only
  for work over 400ms.
- **Problem:** a plain-language reason first, then the next action. Raw error text sits under a
  collapsed "Details".
- **Done:** a summary sentence and the natural next step ("3 files changed · Review changes").

## Motion

- Rows and cards appear with a 150ms fade and a 4px rise, from an already visible state (content is
  never hidden waiting for an animation).
- Expanding a tool step animates height over 150ms. Nothing bounces, pulses or loops, except the
  running spinner.
- With `prefers-reduced-motion`, all of this is instant.

## Accessibility

- A visible focus ring on everything: 2px `--accent` outline, 2px offset.
- Hit targets are at least 32px tall in dense rows and 40px in the icon rail.
- Focus never moves on its own. Alt+A focuses a waiting approval card. New approvals are announced
  assertively; status changes politely.
- A color is never the only signal: the waiting state uses an icon plus text, and diff lines carry
  `+`/`−` markers.
- Agent output is untrusted: it is rendered as text or the safe Markdown subset, never as HTML.

## Don't

- No stacked bordered cards for list rows, and no colored left-border stripes on cards.
- No gradients, glows, glassmorphism, emoji, illustrations or marketing copy.
- No new colors. If something seems to need one, it's probably a state that already has a token.
- No centered layouts in working views, and no placeholder-only labels.
- No single-key or Enter-default actions on anything that approves, deletes or reverts.
