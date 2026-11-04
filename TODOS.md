# TODOS

## Releases

### Restore source to this release

**What:** An explicitly confirmed "Restore source to this release" action on a release row that restores the project's files from the source snapshot recorded at build time.

**Why:** Completes PRD F11 ("Restore source is a separate, explicitly confirmed Git operation"): a user can get back the exact code of a release that worked, including builds made from uncommitted edits.

**Context:** Deferred from Milestone 4 (engineering review 2026-11-03, D1). M4 records `source_commit`, `dirty_source` and `snapshot_ref` for every release (`refs/studio/builds/<artifact-id>` in the project repo; the shadow ref for non-Git projects), kept as long as the release files are kept. Start from the named-file restore in `src-tauri/src/services/git/repo.rs` and `services/git/checkpoint.rs`. Refuse on a dirty tree and list the files that will change in the confirmation.

**Effort:** M
**Priority:** P2
**Depends on:** Milestone 4 build source record (review decision R1)

## Chat attachments

### Attach spreadsheets, slide decks and OpenDocument files

**What:** Accept XLSX, PPTX, ODT and ODS as prompt attachments by converting them to text Claude Code can read, the same way DOCX goes through pandoc.

**Why:** The first version of attachments (eng review 2026-11-04, D2) covers images, PDF, text/code and DOCX only; these formats are refused with "This file type isn't supported yet".

**Pros:** Spreadsheets of requirements and slide decks with mockups work like Word files.

**Cons:** Each format needs its own conversion and edge cases (formulas, merged cells, slide images); pandoc covers ODT but not XLSX or PPTX well.

**Context:** Start from the DOCX path in the attachments service (pandoc with a timeout, extracted media listed as attachments). The supported-type list is one Rust list (D8 R8a); add the formats there and the picker follows.

**Effort:** M
**Priority:** P3
**Depends on:** Chat attachments v1 (eng review 2026-11-04)

## Accessibility

### Screen-reader spec for Releases and Installed Apps

**What:** Give the M4 Releases table and Installed Apps list screen-specific accessibility: a semantic table with column headers, row buttons whose names include the version ("Roll back to 1.0.0"), the WAI-ARIA menu-button pattern for ⋯ menus, and a polite live status row.

**Why:** The M4 design review (2026-11-03, DR-D18) kept only DESIGN.md's general rules; without these, screen-reader users hear several identical "Activate" buttons and may miss build outcomes.

**Context:** Decided in the M4 design review, Pass 6. Specs: `DESIGN.md` "New in Milestone 4" (release table, app list row) and the review file's Pass 6 notes. Start in the Releases tab and Installed Apps view components; add Vitest checks on accessible names.

**Effort:** S
**Priority:** P3
**Depends on:** Milestone 4 UI (eng task T7)

## Completed
