# 0008: Milestone 5 engineering decisions

- **Status:** Accepted
- **Date:** 2026-11-03
- **Scope:** Milestone 5 (GitHub) of [AI_App_Studio_PRD.md](../AI_App_Studio_PRD.md)

This file records why Milestone 5 looks the way it does. It is a record, not a spec: the implementation spec is PRD Section 11, "Milestone 5 — GitHub". Where the two disagree, the PRD wins.

Decision IDs are prefixed `M5-`. The engineering review used M5-D1 to M5-D15; the design review uses M5-DR1 to M5-DR13 ([0009](0009-milestone-5-design-review.md)). The review's own ledger numbers the same choices R1 to R11; the tables below give the mapping.

## Context

Milestone 4 shipped builds, the GNOME launcher and rollback. M5 adds GitHub: sign in, create a private repository, clone, push, fetch, pull, and a way through conflicts.

Facts that shaped the decisions (measured on this machine unless noted):
- `gh` 2.102.0 is signed in through the keyring. `gh auth status --json hosts` returns `{"hosts":{}}` with exit 0 when signed out. Offline, it exits 0 with a host state of `error`, so "signed out" and "offline" must be told apart.
- `gh auth status` takes about 350 ms and calls api.github.com.
- git has the `gh auth git-credential` helper in the global config here, and the GNOME session has an ssh agent. Other machines may have neither.
- `RepoService` already runs git with a cleared environment and `GIT_TERMINAL_PROMPT=0`. That stops only git's own HTTPS prompt, not an ssh passphrase or host-key prompt.
- `RepoService::git()` stops commands after 30 s. `require_idle` checks Make-changes runs only, not builds.
- Two secret-pattern lists exist (`services/agents/secrets.rs` and `services/git/repo.rs`) and have already drifted.
- The signed-in token lacks `delete_repo`, so a test cannot remove the repositories it creates.
- Unverified: whether `opener:default` allows https URLs (needed for Open on GitHub).

No outside voice: the owner turned off second-opinion reviewers, and Codex is blocked by the host permission classifier.

## Decisions

### Scope

| # | Decision | Why |
|---|---|---|
| M5-D1 | The review ran against the PRD, without /office-hours. | The PRD already defines F12 and Scenario C. |
| M5-D2 | Include in-app sign-in: `gh auth login --web`, the one-time code shown in Studio, cancellable. A fake-gh test pins the parsed stderr line. | The first run is not blocked on a terminal. |
| M5-D3 | The new-project wizard gets "Create private GitHub repository". Off by default, only with Initialize Git, runs after the local project exists. A failure keeps the project and points to retry. | PRD F01 offers a GitHub remote in the wizard. |
| M5-D4 | Original arrangement: new `services/github` and `commands/github.rs` plus a github store. `RepoService` gains upstream status, fetch, pull, merge and push. | Four `gh` calls in one service. A `RemoteHost` trait would have one implementor. |

### Architecture

| # | Review | Decision | Why |
|---|---|---|---|
| M5-D5 | R1 | Network git commands run non-interactively. `GIT_SSH_COMMAND="ssh -o BatchMode=yes -o ConnectTimeout=15"` unless the user set their own ssh command (`GIT_SSH_COMMAND` or `core.sshCommand`). HTTPS to github.com adds `-c credential.https://github.com.helper=!<gh> auth git-credential` on the command line. Nothing is written to git config. Auth failures become a typed "sign in to GitHub" error. | A GUI must not wait on a prompt nobody sees. Works on any machine signed in to `gh`. |
| M5-D6 | R2 | Pull is `git pull --ff-only`. When the branch has diverged, an explicit "Merge GitHub's changes" runs `git merge --no-edit @{u}`. A conflicted merge leaves a merge-in-progress state in Source with the conflicted files and "Abort merge" (`git merge --abort`). The user resolves by editing (or Ask Claude) and commits. | Keeps the user in Studio through a conflict (PRD: "handle conflicts cleanly") without a force push or reset. |
| M5-D7 | R3 | `RepoService::require_idle` also refuses while a build is live ("A release build is running"). Restore, Undo restore, Switch branch, Pull, Merge and Abort merge all go through it. | One guard fixes an existing M3/M4 gap and covers the new operations. |
| M5-D8 | R4 | Migration 0005 adds `git_sync` (one row per project: `operation`, `ok`, redacted `message`, `finished_at`). Table only, no rebuild. Each push, pull, merge, fetch and create upserts its result. | A failed push stays visible after a restart. A new job kind would rebuild the `jobs` table again. |
| M5-D9 | R5 | Create GitHub Repository refuses when the project is a subfolder of a larger repo or already has an `origin` remote (the remote is shown). With no commit it asks for a first commit. With no repo it runs the existing Initialize Git flow first. | `gh repo create --source` works on the repo top level and would publish the parent repository. |
| M5-D10 | R6 | Clone goes into a hidden temp sibling `.ai-app-studio-clone-<uuid>` of `<parent>/<repo name>` and is renamed when complete, then imported. An existing target is refused. Stop or failure deletes only the temp folder. Startup removes leftovers in the default projects folder. | A project folder exists only when complete. Same pattern as New Project. |
| M5-D11 | R7 | No overall time limit on network commands. A stalled transfer fails after about 60 s: ssh `ServerAliveInterval=15` and `ServerAliveCountMax=4`, HTTPS `http.lowSpeedLimit=1000` and `http.lowSpeedTime=60`. Every network command has Stop, which kills its process group. | Big, moving transfers finish. Dead connections fail fast. |
| M5-D12 | R8 | The secret-pattern table moves from `repo.rs` into `services/agents/secrets.rs` as the single list. `redact`, `looks_secret` and `scan_line` all read it. `redact` also replaces URL credentials (`scheme://user:token@` becomes `scheme://[redacted]@`). Every git and gh message M5 shows or stores passes through `redact`. | Ends the drift (`ghs_`, `ghu_`, `ghr_`, Google keys and private keys were missed). Roughly line-neutral. |
| M5-D13 | R9 | One network operation per project at a time, tracked in a per-project registry (modeled on the install lock). A second one is refused with "Another sync is running". Stop uses the registry. Different projects run in parallel. Clone is per destination. | No raw "cannot lock ref" errors. Stop needs the record anyway. |
| M5-D14 | R10 | Automated tests use local bare repositories and a fake `gh`. One more test runs only with `STUDIO_GITHUB_E2E=1`: it creates a private repo, pushes, fetches, pulls and clones against github.com, and prints the repo URL for manual deletion. | Offline tests cover every Studio branch. The opt-in test catches `gh` changes. |
| M5-D15 | R11 | `gh auth status --json hosts` runs when the GitHub tab opens, after a sign-in, and after a network operation fails with an auth error. Otherwise the last result is reused. A "Check again" button re-runs it. Ordinary status refreshes never call `gh`. | No network call per refresh, and offline editing stays quiet. |

### Typed auth state (required work under M5-D2)

`auth_status()` returns `SignedIn{login, git_protocol}`, `SignedOut`, `Unreachable{message}` or `TokenInvalid{message}`. Only `SignedOut` and `TokenInvalid` offer Sign in. Treating every non-success as signed out would ask an offline user to sign in again.

### Required behavior carried from F12 and Section 7 without separate questions

- **No force, no reset:** never force-push, never reset automatically. A rejected push returns a typed "GitHub has commits you do not have" error.
- **Dirty tree:** Pull refuses on a dirty tree (existing `SourceError::DirtyTree`).
- **User-initiated only:** Fetch, Pull and Push run only on a user action. There are no automatic network calls. Ahead/behind is relative to the last fetch, and the UI shows when that was (`.git/FETCH_HEAD` mtime).
- **Visibility:** private is the default. Public needs an explicit confirmation.
- **Redaction:** git and gh text is redacted before it reaches the UI or the database.
- **First push:** `git push -u <remote> HEAD:<branch>` sets the upstream. A detached HEAD refuses Push.
- **No `GitRemote` table:** PRD 6.4 listed one, but the PRD also says "Database is a metadata index, not a replacement for Git". The remote URL and branch are read from the repo's own config on every status. PRD 6.4 now lists `GitSync` instead.

### Design additions that changed the engineering scope

The design review ([0009](0009-milestone-5-design-review.md)) added backend items:
- `RepoService::mark_resolved(path)` and a merge-mode commit (M5-DR7). Mid-merge, `git commit --only` fails with "cannot do a partial commit during a merge". The merge commit is a plain `git commit`, refused while conflicts remain.
- `GhService::list_repos()` (M5-DR12).

## Rejected

- A `RemoteHost` trait with GitHub and GitLab adapters (M5-D4): one implementor, and git push, fetch and pull already work for any remote.
- `gh auth setup-git` (M5-D5): it edits the user's global git config, which PRD 7.1 treats as a system change needing approval.
- Sync operations as jobs (M5-D8): a fourth `jobs` rebuild, and a two-second fetch would claim a job slot.

## Not in scope

- F12 P1: tags and GitHub Releases, AppImage attachments, branches and pull requests on GitHub, CI results, issue links, changelogs.
- Creating a GitHub repo for a project inside a larger repository (M5-D9 refuses with an explanation).
- Rebase pulls, force push, automatic fetch, deleting GitHub repositories.
- GitHub Enterprise hosts, and sign-in or create for other git hosts. Push, fetch and pull still work for any remote through git.
- Editing the user's global git config.

## Failure modes

| Path | Failure | Handling | User sees |
|---|---|---|---|
| Push, Pull, Fetch | no ssh key or HTTPS helper | BatchMode, typed AuthRequired, re-check auth | "Sign in to GitHub" |
| Push | GitHub has newer commits | typed Rejected, no force | Pull |
| Pull | diverged | typed Diverged | "Merge GitHub's changes" |
| Merge | conflicts | merge-in-progress state | conflicted files and Abort merge |
| Any network op | connection drops | stall abort, or Stop | "Connection stalled" or stopped |
| Any network op | second op started | registry refusal | "Another sync is running" |
| Create | name taken | typed error, redacted | name-taken message |
| Create | project in a larger repo | guard refusal | explanation, nothing uploaded |
| Wizard create | GitHub fails after local create | project kept | retry on the GitHub tab |
| Clone | network fails or Stop | temp folder deleted | error or stopped, no folder |
| Sign-in | `gh` wording changes | fake-gh test pins the line; parse failure is typed | "Could not read the sign-in code" and a terminal hint |
| Sign-in, auth | offline | `Unreachable` state | "GitHub unreachable", no Sign in |
| Pull, Merge, Restore, Switch | build running | M5-D7 guard | "A release build is running" |
| Messages | token in a URL or output | redact | text with `[redacted]` |

Exact user-facing wording is set by the design review (M5-DR10). Critical gaps (no test, no handling, silent): none.

## Consequences

- **Tasks:** T1 to T11, plus T12 to T16 from the design review. Shared prep and the migration first, then the git and gh lanes in parallel, then commands, UI, tests and acceptance.
- **M3 behavior changes:** Restore and Switch branch now also refuse during a build (M5-D7). The commit-warning list and log redaction now share one table (M5-D12). Their existing tests must pass unchanged.
- **Opt-in test leaves a repo:** each `STUDIO_GITHUB_E2E=1` run leaves a private repository to delete by hand (M5-D14).
- **Revisit:**
  - `gh auth login` output wording, if the fake-gh test starts failing against a new `gh`;
  - the stall numbers after real use;
  - the `opener:default` https permission, checked when Open on GitHub is built.
