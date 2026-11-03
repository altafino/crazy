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
    case "invalid": case "git": return e.message;
    default: return "Something went wrong with Git.";
  }
}
