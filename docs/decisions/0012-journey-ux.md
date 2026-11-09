# 0012: Journey from Chat to release

- **Status:** Accepted
- **Date:** 2026-11-08
- **Scope:** continuing conversations, the Chat thread, project tab order, the Build dialog, build and install, finish notifications.

This file records why the Chat-to-release journey works the way it does. It is a record, not a spec: the spec is `DESIGN.md` ("New: Journey"). Where the two disagree, `DESIGN.md` wins. It supersedes the "Revise plan … sent as another Plan first run" part of PRD D53.

## Context

- Before the change every Run started a fresh Claude session, Chat showed one run at a time behind a header picker, and building and installing a release were separate steps.
- Design review 2026-11-08 rated the journey 4/10 and ended at 9/10 (decisions D2–D15); the engineering review the same day added R1–R6.

## Decisions

| # | Decision | Why |
|---|---|---|
| J-D2 | Chat is one thread of runs: last 20, "Show earlier runs", day dividers; the run picker leaves Chat. | Follow-ups read as one conversation, not a list of separate runs. |
| J-D3, D4 | Prompt-first run labels; tabs in loop order Chat · Plan · Changes · Preview · Source · Releases · GitHub · Database. | Users find runs by what they asked, and the tabs follow the work. |
| J-D5 | A Jump-to-latest pill, amber when an approval waits; it never moves the scroll. | Reading old runs must not hide a waiting approval. |
| J-D6, D14 | Run continues the conversation by default; "New conversation" starts fresh; Plan Start begins a new conversation. | Claude keeps the context of the plan and earlier fixes. |
| J-R1 | Resume only under the same agent profile; otherwise start fresh and say so. | Profiles can set `CLAUDE_CONFIG_DIR`; Claude keeps sessions per config dir. |
| J-R2 | A resumed run that fails before Claude's init is retried once without `--resume`; other outcomes never retry. | A missing session should not look like a broken app. |
| J-D7 | End rows offer Start preview, or say the preview updates by itself. | The next step after a change is seeing it. |
| J-D8–D11 | Build dialog: next patch selected with bump links, notes from runs since the last release, Commit first…, Install checkbox. | A release takes one dialog, not a page of typing. |
| J-D10, R3, R4 | Build and install runs as an Install phase in the build job: the older-than-data check, then install (data backed up first). Older than data stops and asks. | The safety check that lived only in the UI now guards the unattended install. |
| J-R5 | Older runs load their events when near the screen. | Event logs reach hundreds of KB; 20 eager reads slow opening Chat. |
| J-D12 | Finish notifications only while Studio is unfocused, with a Settings switch; approvals always notify. | Long runs and builds end while the user is elsewhere. |
| J-R6 | Regression contract: Make these changes, Allow and retry, Alt+A, composer handoff, drop overlay, per-run end rows, Build without install. | The thread rewrite touched all of them. |

## Known limits

- An install outcome lives in memory; after a restart the row shows the plain built-but-not-installed state.
- When a resume falls back, the fresh attempt's log replaces the failed one; the reason stays in the warning's Details.
- "Show earlier runs" widens a client-side slice; `list_sessions` still returns every run.
