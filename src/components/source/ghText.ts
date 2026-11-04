import type { GhError } from "../../bindings";

/** DR10 wording for GitHub errors (M5-DR10). Raw text is not shown here. */
export function ghErrorText(e: GhError, name = "that name"): string {
  switch (e.kind) {
    case "signedOut": case "tokenInvalid": return "GitHub needs you to sign in again.";
    case "unreachable": return "The connection to GitHub stopped responding.";
    case "nameTaken": return `You already have a repository named ${name}.`;
    case "insideLargerRepo": return `This project is inside the repository at ${e.top}; create it there instead.`;
    case "targetExists": return `${e.path} already exists.`;
    case "ghMissing": return "GitHub CLI is not installed.";
    case "invalidName": return e.message;
    case "invalidRepoSpec": return "That doesn't look like a GitHub repository. Use owner/name or a GitHub URL.";
    case "stopped": return "Stopped.";
    default: return "Git reported a problem.";
  }
}
