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

## App databases

### Phase 2: other engines and configured connections

**What:** Let a generated app use a database engine other than SQLite, either bundled with the app (embedded, or a server packaged into the AppImage) or reached through a connection the user configures (PostgreSQL, MySQL, ...). Needs its own design review first.

**Why:** The owner's requirement R3 for app databases; deferred in the design review of 2026-11-04 (D17, DR-16) so Phase 1 (SQLite) can ship first.

**Pros:** Apps can share data across machines or use an existing company database.

**Cons:** Bundled servers make AppImages large and need lifecycle handling; connections add credentials and network failure states.

**Context:** Plan and decisions: `~/.gstack/projects/altafino-crazy/app-databases-plan-20261104.md`. Fixed rule: a connection password lives in the system keyring (GNOME Keyring via Secret Service), stored by the generated app; never in Studio's database, project files, Git, prompts or logs.

**Effort:** L
**Priority:** P3
**Depends on:** App databases Phase 1 (SQLite) built

## Exports for other systems

### Flatpak export

**What:** Add a Flatpak target to "Export for other systems": a flatpak-builder manifest on the GNOME runtime, generated cargo and node source lists for an offline build, and an AppStream metainfo file.

**Why:** Deferred in the F22 eng review (2026-11-06, D4). AppImage, deb and rpm cover the big distro families; Flatpak is the one package that runs the same everywhere and can go to Flathub.

**Pros:** One package for every distro; sandboxed by default (fits PRD F24); a path to Flathub.

**Cons:** A second pipeline next to the Tauri build: flatpak-builder, flatpak-cargo-generator and flatpak-node-generator (run without node_modules present), a runtime download, AppStream metadata. Flathub has its own review.

**Context:** Start from Tauri's guide (https://v2.tauri.app/distribute/flatpak/) and the export container from F22 (`services/builds/export.rs`). The export layout `releases/<ver>/exports/<target>/` with SHA256SUMS takes a new `flatpak` target folder.

**Effort:** L
**Priority:** P3
**Depends on:** F22 exports built

### macOS export via CI

**What:** Build a macOS `.app`/`.dmg` for a release on a macOS GitHub Actions runner (tauri-action), and download the result into the release's exports folder.

**Why:** The owner asked for a TODO in the F22 eng review (2026-11-06, D5). A Linux container can't build Mac apps; it needs a Mac or a macOS runner, plus Apple signing and notarization to open without warnings.

**Pros:** Mac users can get the apps; the same workflow could also produce a native Windows `.msi`.

**Cons:** Needs the project on GitHub, a workflow file in the user's repo, network and Actions minutes; signing needs an Apple developer account. Needs its own design review first (CI was not chosen for F22, D2).

**Context:** F22 chose local Docker (D2). Start from tauri-action and Studio's existing gh sign-in (`services/github`). Exports go to `releases/<ver>/exports/macos/` with SHA256SUMS like the other targets.

**Effort:** L
**Priority:** P3
**Depends on:** F22 exports built; F19 (GitHub Releases) helps

### Sign the Windows installer

**What:** Sign the exported Windows `setup.exe` with Authenticode (osslsigncode in the export container, or a cloud signing service), so Windows doesn't show the SmartScreen "Windows protected your PC" screen as often.

**Why:** Decided in the F22 eng review (2026-11-06, D13). F22 ships unsigned installers and says so on the release row.

**Pros:** Fewer scary warnings for the people who receive the app.

**Cons:** Needs a paid code-signing certificate; reputation still builds up over time.

**Context:** Fixed rule: Studio never stores the certificate or its password (system keyring or the signing service only, never Studio's database, project files, Git, prompts or logs). Hook in after the NSIS bundle is located in `services/builds/export.rs`, before SHA256SUMS is written.

**Effort:** M
**Priority:** P3
**Depends on:** F22 exports built; a code-signing certificate

## Completed
