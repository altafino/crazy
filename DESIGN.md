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
| Button (`.btn`) | 8px radius, 6px 14px padding, `--card` bg, 1px `--border-strong`. Hover: `--accent` border. **Primary:** `--accent-strong` bg, `--on-accent` text. **Destructive:** `--danger` text and border on `--card`; filled `--danger` bg only inside a confirmation dialog. `.small` for inline actions. |
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
