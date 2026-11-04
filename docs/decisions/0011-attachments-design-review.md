# 0011: Chat attachments design decisions

- **Status:** Accepted
- **Date:** 2026-11-04
- **Scope:** the attachments UI: chips, the chip area, drop overlay, Attach button, preview and open, and Settings > Attachments.

This file records why the attachments UI looks the way it does. It is a record, not a spec: the spec is `DESIGN.md` ("New: Chat attachments"). Where the two disagree, `DESIGN.md` wins. Decision IDs continue from the engineering review ([0010](0010-attachments-eng-review.md)).

## Context

- Before the review the plan listed buttons only; it rated 2/10 and ended at 8/10.
- The review generated 4 mockups; the owner approved one. It was not kept in the repo. Its chips are taller than 28px and its timeline uses avatars: the written spec wins over the picture. The "tools.read_file" lines in it are fake.

## Decisions

| # | Decision | Why |
|---|---|---|
| ATT-DR2 | The chip area wraps and holds at most 2 rows (2 × 28px plus a 6px gap), then scrolls inside itself. Names are middle-truncated at 24 characters; the full name is in the tooltip and the accessible name. | The textarea and Run never move further down. |
| ATT-DR3 | Chip states: adding, converting, ready (image, PDF or text, Word), error, timeline, timeline pruned. The exact table and error texts are in `DESIGN.md`. A PDF shows its size, never a page count. | Every state has a look and an accessible name. |
| ATT-DR4 | An error chip blocks Run. The caption beside Run reads "Remove 1 file that can't be attached" (plural "Remove 2 files…"); while a chip is adding or converting it reads "Waiting for 1 file to finish converting". Error wins over waiting. | The reason sits next to the disabled button. |
| ATT-DR5 | The whole Chat tab is the drop target: an `--info-bg` overlay with a dashed `--accent` outline and "Drop files to attach". During a Make-changes run it reads "Attach files after this run finishes" and a drop attaches nothing. Other tabs ignore drops. | The Tauri drag-drop event is window-wide; only the visible Chat tab handles it. |
| ATT-DR6 | `DESIGN.md` gains "New: Chat attachments": Attachment chip, Chip area, Error reasons, Drop overlay, Attach button, Settings > Attachments. Existing tokens only. | Builders follow specs, not pictures. |
| ATT-DR7 | Below 900px the Attach button becomes a 32px paperclip icon button (tooltip and `aria-label` "Attach files"), and Run's reason caption moves to its own line. Mode, profile and Run stay on one line. | Fits the 760 px window. |
| ATT-DR8 | The chip area is `role="list"` labelled "Attached files". Each chip's name follows the DR3 table; the × is named "Remove <name>". Delete or Backspace on a focused chip removes it and moves focus to the next chip, or to the textarea after the last. The status strip announces "<name> attached", "<name> converted" and "<name> can't attach: <reason>". The overlay is `aria-hidden`. Run's reason is tied with `aria-describedby`. | The Attach button is the keyboard path for drop. |
| ATT-DR9 | Image chips (composer and timeline) are buttons named "Preview <name>" that open `Modal.vue` with the image fitted, the name as title and "Open in default app". Esc closes and focus returns to the chip. Other chips are buttons named "Open <name>" that open the file in the system app through `@tauri-apps/plugin-opener`. The opener permission is scoped to `$DATA/ai-app-studio/attachments/**`, with a config test that fails if the scope is wider. Pruned chips are not clickable. | One click from a chip to the file, without a wider permission. |

## Journey storyboard

| Step | User does | User feels | Specified by |
|---|---|---|---|
| 1 | Takes a screenshot, presses Ctrl+V in the composer | quick | paste makes a chip with a preview |
| 2 | Drags a PDF and a Word file onto the chat | unsure it will work | the drop overlay (DR5) |
| 3 | Sees "Converting…" and a disabled Run with its reason | informed | DR3, DR4 |
| 4 | An .xlsx shows "This file type isn't supported yet" | clear what to do | error chip, DR4 |
| 5 | Removes it, presses Run | confident | chips under the prompt in the timeline |
| 6 | Presses "Make these changes" | trusts the follow-up sees the same files | conversation folder (ATT-D5) |
| 7 | Weeks later reopens the conversation | oriented | pruned files show "File removed" |

## As built

- **Chip padding** is `0 8px`, not `4px 8px`, so the 24px preview fits in a 28px chip.
- **Word chips** show "converted" without an image count: the backend has no field for it yet.
- **Extra error texts** not in DESIGN.md: "Too large: N MB per prompt in total", "Couldn't convert this file", "This file is no longer there".

## Not in scope

- PDF page counts on chips: Studio has no PDF reader.
- Attaching while a Make-changes run is active: the overlay says so.
- Visual QA of the built screens: run `/design-review` after the build.
