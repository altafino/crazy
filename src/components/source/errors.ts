import type { SourceError } from "../../bindings";

export type Res<T> = { ok: true; data: T } | { ok: false; error: SourceError };

/** Unwraps a bindings result whose error is a `SourceError`; a thrown IPC error becomes `git`. */
export async function src<T>(p: Promise<{ status: "ok"; data: T } | { status: "error"; error: SourceError }>): Promise<Res<T>> {
  try {
    const r = await p;
    return r.status === "ok" ? { ok: true, data: r.data } : { ok: false, error: r.error };
  } catch (e) {
    return { ok: false, error: { kind: "git", message: e instanceof Error ? e.message : String(e) } };
  }
}

/** Plain-language text for a Source error the UI has no dedicated state for. */
export function sourceErrorText(e: SourceError): string {
  switch (e.kind) {
    case "runInProgress": return "Claude is changing files right now. Try again when the run ends.";
    case "buildRunning": return "A release build is running.";
    case "conflictsPresent": return "Some files have conflicts. Resolve them first.";
    case "dirtyTree": return `Commit or restore your ${e.count} uncommitted ${e.count === 1 ? "change" : "changes"} before switching branches.`;
    case "nothingToCommit": return "There is nothing to commit.";
    case "branchExists": return "A branch with that name already exists.";
    case "branchNotFound": return "That branch no longer exists.";
    case "invalidBranchName": return "That isn't a valid branch name.";
    case "commitTimedOut": return "The commit took longer than 2 minutes and was stopped.";
    case "statusTimedOut": return "Git took too long to answer.";
    case "repoLocked": return "Another Git program is using this repository.";
    case "noUndo": return "There is nothing to undo.";
    case "authRequired": return "GitHub needs you to sign in again.";
    case "rejected": return "GitHub has commits you don't have yet.";
    case "diverged": return "Your project and GitHub both changed.";
    case "stalled": case "unreachable": return "The connection to GitHub stopped responding.";
    case "syncRunning": return "Another sync is running.";
    case "noUpstream": return "This branch is not on GitHub yet.";
    case "noRemote": return "This project is not on GitHub yet.";
    case "detachedHead": return "Switch to a branch first.";
    case "markersRemain": return `${e.paths.join(", ")} still ${e.paths.length === 1 ? "has" : "have"} conflict markers.`;
    case "cancelled": return "Stopped.";
    case "invalid": case "git": return e.message;
    default: return "Something went wrong with Git.";
  }
}

export type SyncFix = "signIn" | "pull" | "merge" | "retry" | "openSource" | null;

/** M5-DR10: fixed wording for a failed sync action. The raw text, when any, goes under Details. */
export function syncErrorText(e: SourceError): string {
  switch (e.kind) {
    case "authRequired": return "GitHub needs you to sign in again.";
    case "rejected": return "GitHub has commits you don't have yet.";
    case "diverged": return "Your project and GitHub both changed.";
    case "stalled": case "unreachable": return "The connection to GitHub stopped responding.";
    case "dirtyTree": case "conflictsPresent": return "Commit or restore your changes first.";
    case "buildRunning": case "runInProgress": return "Wait for the Make-changes run / release build to finish.";
    case "syncRunning": return "Another sync is running.";
    case "cancelled": return "Stopped.";
    case "noUpstream": case "noRemote": case "detachedHead": case "markersRemain": return sourceErrorText(e);
    default: return "Git reported a problem.";
  }
}

export function syncFix(e: SourceError): SyncFix {
  switch (e.kind) {
    case "authRequired": return "signIn";
    case "rejected": return "pull";
    case "diverged": return "merge";
    case "stalled": case "unreachable": return "retry";
    case "dirtyTree": case "conflictsPresent": return "openSource";
    case "buildRunning": case "runInProgress": case "syncRunning": case "cancelled": case "noUpstream": case "noRemote": case "detachedHead": case "markersRemain": return null;
    default: return "retry";
  }
}

export const syncErrorDetails = (e: SourceError): string => ("message" in e ? e.message : "output" in e ? e.output : "");
