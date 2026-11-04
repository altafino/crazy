# 0010: Chat attachments engineering decisions

- **Status:** Accepted
- **Date:** 2026-11-04
- **Scope:** attaching files to a Chat prompt (images, PDF, text and code, Word). Built after Milestone 5; not a numbered milestone in [AI_App_Studio_PRD.md](../AI_App_Studio_PRD.md).

This file records why chat attachments work the way they do. It is a record, not a spec. Where it disagrees with the code, the code is right: "As built" below lists the places where the build differs from the review.

Decision IDs are prefixed `ATT-`. The engineering review used D2 to D13; the design review uses ATT-DR2 to ATT-DR9 ([0011](0011-attachments-design-review.md)). The review's own ledger numbers the same choices R1 to R11; the table gives the mapping. D1 (skip /office-hours) is not recorded below.

## Context

The owner's request was one line: let the user send files with a prompt, starting with images and common formats like PDF and Word.

Facts that shaped the decisions:
- Studio writes the first prompt as one stream-json user message with a plain string as `content`.
- `--add-dir` exists in the installed CLI, and `--allowedTools` is already passed per run.
- No zip or XML crate is installed, so Word needs a converter.
- `is_secret` in `services/git` already knows secret-looking file names.

Spike (T1, CLI 2.1.318, claude-opus-5-5), with Studio's own argv (`--permission-prompt-tool stdio`, `--setting-sources project,local --strict-mcp-config`), the project folder as cwd and the files outside it:
- **Plan:** Read of a PNG and of a PDF worked with zero permission requests. The answers matched the files.
- **Make changes:** the same, zero requests.
- **`--resume`:** the PNG was read again, zero requests.
- **Control without the Read rule** (`--add-dir` only): also zero requests. So `--add-dir` alone already grants Read today. The `Read(//<dir>/**)` rule stays as a guard against a CLI default changing.

No outside voice (the owner turned off second-opinion reviewers).

## Decisions

| # | Review | Decision | Why |
|---|---|---|---|
| ATT-D2 | scope | v1 types: PNG, JPEG, GIF, WebP; PDF; plain text and source code; Word (.docx). XLSX, PPTX and ODT/ODS are refused with "This file type isn't supported yet" and listed in `TODOS.md` (P3). | The common cases first; each extra office format needs its own converter. |
| ATT-D3 | structure | A separate `services/attachments` service, not code inside the agent adapter. | It hides type detection, limits, conversion, naming and storage. |
| ATT-D4 | R1 | Files plus `--add-dir`. Files are staged in a folder under Studio's data dir; `build_argv` adds `--add-dir <folder>` and `Read(//<folder>/**)` to `--allowedTools` (last); the prompt ends with "Attached files:" and the absolute paths, one per line. Claude Code reads them with its own Read tool. | No base64 on stdin, no size blow-up, the same path works on `--resume`. Inline image and document blocks were rejected: not documented for Claude Code. The T1 spike proved this shape. |
| ATT-D5 | R5 | The folder key is the conversation, not the run: `<data>/attachments/<conversation>/`. The conversation is the first run's session id, followed through `resumed_from`. A resumed run, and "Make these changes" after a plan, keep the same folder readable. | A later run must still read what an earlier prompt attached. |
| ATT-D6 | R2 | Word files are converted with `pandoc -t gfm --extract-media=.` when pandoc is installed. The Markdown and its `media/` folder sit beside the original. Without pandoc a Word file is refused: "Install pandoc to attach Word files". The doctor has an optional pandoc row; missing is informational, not an error. | No new crate and no home-made DOCX parser. |
| ATT-D7 | R7 | Image previews use the asset protocol, enabled with scope `$DATA/ai-app-studio/attachments/**` only. A Rust config test fails if the scope is wider. | Previews without reading bytes into the page. |
| ATT-D8 | R8 | Three structural rules: one type list in Rust (the picker filter comes from the `attachment_types` command, and a test checks that every listed type is accepted); the adapter owns Claude syntax (the service returns only the conversation folder); the attachments service alone writes `run_attachments` rows and deletes files. | One owner per fact, so a second adapter can reuse the folder. |
| ATT-D9 | R9 | Files wait in `<data>/attachments/.staging/<draft-id>/`. On Run they move (rename, same filesystem) into the conversation folder and rows are written. Removing a chip deletes its file. | Copying, type checks and pandoc happen when the user adds the file, so Run is fast and errors show on the chip. |
| ATT-D10 | R10 | Regression contract: with no attachments, argv has no `--add-dir` and no Read rule, and the user content is the plain prompt. Existing argv tests are extended, not changed. | Runs without attachments must not change. |
| ATT-D11 | R11 | Besides the one-time spike, an opt-in test against the real CLI: `STUDIO_CLAUDE_E2E=1 cargo test claude_attachments_e2e`. It is skipped by default. | The Read behavior belongs to the CLI and can change in an update. |
| ATT-D12 | R3 | Limits have defaults and are configurable in Settings > Attachments, with bounds enforced in the backend. Defaults: 10 files per prompt, images 20 MB, PDF 32 MB, text 2 MB, Word 20 MB (converted Markdown 2 MB), 64 MB per prompt in total, pandoc 60 s per file. | Defaults for the common case; the owner can raise them. |
| ATT-D13 | R4 | A conversation's folder is deleted when none of its runs is among the project's last 20 runs, in the same prune pass as snapshots. Project removal deletes all of the project's attachments. | The folders do not grow forever. Twenty matches the snapshot retention. |

Other rules decided in the review:
- Secret-looking files (by `is_secret`) are refused: "Secret-looking files can't be attached".
- Files are copied, never linked.
- Types are checked by content as well as by extension.
- An error chip blocks Run until it is removed.
- `start_run` checks the files again before it starts.
- Timeline chips come from one query.

## As built

The code differs from the review in these places.

- **Pruning keeps rows.** Prune deletes the files but keeps the `run_attachments` rows, marked `removed`, so timeline chips can show "File removed" (ATT-D13 said rows are deleted too).
- **Drafts live in memory.** Staged drafts are held in memory only. A restart drops the chips; the 24 h startup sweep deletes the leftover `.staging` folders.
- **Store owner.** `AttachmentStore` is owned by the job supervisor, which calls prune after a run, next to snapshot pruning.
- **No project removal command yet.** Studio has no user-facing "remove project" command. The cleanup hook is on the reconcile rollback path. A future removal command must call `remove_project` on the attachments service.
- **Word chips.** A converted Word chip shows "converted" without an image count. The backend has no field for it yet.
- **Composer previews.** The frontend computes the staging path from `dataDir()`. It follows the same XDG rule as Studio's data dir and lies inside the `$DATA` scopes of the asset protocol and the opener permission.
- **Chip padding.** Chips use `0 8px` padding, so the 24px preview fits in the 28px chip. DESIGN.md says `4px 8px`.
- **Extra error texts.** Three texts are not in DESIGN.md: "Too large: N MB per prompt in total", "Couldn't convert this file" and "This file is no longer there".

## Rejected

- Inline base64 content blocks on stdin (ATT-D4).
- An in-process DOCX converter with the `zip` and `quick-xml` crates (ATT-D6).
- Keeping files in the composer and copying on Run (ATT-D9): errors would show too late.
- Deleting a conversation's folder with the project only (ATT-D13): it grows forever.

## Not in scope

- XLSX, PPTX, ODT and ODS (ATT-D2, `TODOS.md`).
- Attachments for agents other than Claude Code (PRD F14). The Claude syntax stays in the adapter for that reason.
- Searching attachments across runs. The `run_attachments` table allows it later; no UI is planned.
- Screenshot capture inside Studio (PRD optional P1). Pasting an image covers the common case.
- Attaching while a Make-changes run is active.

## Failure modes

| Path | Failure | Handling | User sees |
|---|---|---|---|
| Add file | unsupported type | typed error | "This file type isn't supported yet" |
| Add file | over a limit | typed error | "Too large: …" |
| Add file | secret-looking name | refused | "Secret-looking files can't be attached" |
| Add Word file | no pandoc | typed error | "Install pandoc to attach Word files" |
| Add Word file | pandoc fails or is too slow | typed error, timeout from settings | "Couldn't convert this file" or "Conversion took too long" |
| Run | a staged file is gone | `start_run` recheck | "This file is no longer there" |
| Run | CLI stops allowing Read without a prompt | the Read rule; otherwise an approval card | a normal approval card, not a hang |
| Timeline | files pruned | rows kept, marked removed | "File removed" |

## Consequences

- **Prompt injection:** attached content is untrusted input to the agent. A document can contain instructions. Studio's approval prompts still apply to every tool beyond Read of the attachments folder.
- **Revisit:**
  - the Read behavior when the Claude Code CLI updates (run the opt-in test);
  - an image count on Word chips, when the backend reports it;
  - the cleanup hook, when a "remove project" command is added.
