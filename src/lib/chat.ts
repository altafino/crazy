import type { AgentEvent, Denial_Deserialize, Outcome, SessionMode, SessionSummary } from "../bindings";

/** Text of the adapter's fallback warning (src-tauri/.../claude/mod.rs choose_transport). */
export const FALLBACK_WARNING_PREFIX = "Running without live approvals";

const READ_TOOLS = ["Read", "Glob", "Grep", "LS"];
const EDIT_TOOLS = ["Edit", "Write", "MultiEdit", "NotebookEdit"];

export type Step = { id: string; name: string; summary: string; input: unknown; output: string | null; isError: boolean };
export type Approval = { requestId: string; tool: string; input: unknown; description: string; answered: boolean | null };

/** `at` is the unix-ms time the event happened; 0 when unknown (logs from before it was recorded). */
export type Row =
  | { kind: "text"; key: string; at: number; text: string }
  | { kind: "tool"; key: string; at: number; step: Step }
  | { kind: "reads"; key: string; at: number; steps: Step[] }
  | { kind: "approval"; key: string; at: number; approval: Approval }
  | { kind: "plan"; key: string; plan: string }
  | { kind: "warning"; key: string; text: string };

/** Turns the event stream into timeline rows: tool calls paired with results, reads collapsed. */
export function buildRows(events: AgentEvent[]): Row[] {
  const rows: Row[] = [];
  const steps = new Map<string, Step>();
  const approvals = new Map<string, Approval>();
  for (const e of events) {
    const key = `e${e.seq}`;
    const at = e.atMs ?? 0;
    if (e.kind === "text") rows.push({ kind: "text", key, at, text: e.text });
    else if (e.kind === "planReady") rows.push({ kind: "plan", key, plan: e.plan });
    else if (e.kind === "warning" && !e.text.startsWith(FALLBACK_WARNING_PREFIX)) rows.push({ kind: "warning", key, text: e.text });
    else if (e.kind === "toolCall") {
      const step: Step = { id: e.id, name: e.name, summary: e.summary, input: e.input, output: null, isError: false };
      steps.set(e.id, step);
      const last = rows[rows.length - 1];
      if (READ_TOOLS.includes(e.name)) {
        if (last?.kind === "reads") last.steps.push(step);
        else rows.push({ kind: "reads", key, at, steps: [step] });
      } else rows.push({ kind: "tool", key, at, step });
    } else if (e.kind === "toolResult") {
      const s = steps.get(e.id);
      if (s) Object.assign(s, { output: e.output, isError: e.isError });
    } else if (e.kind === "approvalNeeded") {
      const approval = { requestId: e.requestId, tool: e.tool, input: e.input, description: e.description, answered: null };
      approvals.set(e.requestId, approval);
      rows.push({ kind: "approval", key, at, approval });
    } else if (e.kind === "approvalAnswered") {
      const a = approvals.get(e.requestId);
      if (a) a.answered = e.allowed;
    }
  }
  return rows;
}

const lines = (v: unknown) => (typeof v === "string" && v.length ? v.split("\n").length : 0);

/**
 * Rough +/- line counts from an edit tool's input, for the timeline row (D55). A display
 * hint only: the Changes tab's snapshot diff is the truth.
 */
export function editCounts(name: string, input: unknown): { add: number; del: number } | null {
  if (!EDIT_TOOLS.includes(name) || typeof input !== "object" || input === null) return null;
  const i = input as Record<string, unknown>;
  if (name === "Write") return { add: lines(i.content), del: 0 };
  const edits = name === "MultiEdit" && Array.isArray(i.edits) ? (i.edits as Record<string, unknown>[]) : [i];
  return edits.reduce<{ add: number; del: number }>(
    (acc, e) => ({ add: acc.add + lines(e.new_string), del: acc.del + lines(e.old_string) }),
    { add: 0, del: 0 },
  );
}

/** The backend summary is "<Tool>: <detail>"; rows show their own verb, so drop the prefix. */
export const stepDetail = (s: Step) => (s.summary.startsWith(`${s.name}: `) ? s.summary.slice(s.name.length + 2) : s.summary);

export const hasFallbackWarning = (events: AgentEvent[]) =>
  events.some((e) => e.kind === "warning" && e.text.startsWith(FALLBACK_WARNING_PREFIX));

export const changedFiles = (events: AgentEvent[]) =>
  new Set(events.flatMap((e) => (e.kind === "fileChange" ? [e.path] : []))).size;

/** The command or edit shown in full on an approval card (D56). */
export function approvalDetail(tool: string, input: unknown): string {
  const i = (input ?? {}) as Record<string, unknown>;
  const str = (v: unknown) => (typeof v === "string" ? v : "");
  if (tool === "Bash" && str(i.command)) return str(i.command);
  if (EDIT_TOOLS.includes(tool) && str(i.file_path)) {
    const body = str(i.content) || (str(i.old_string) || str(i.new_string) ? `- ${str(i.old_string)}\n+ ${str(i.new_string)}` : "");
    return body ? `${str(i.file_path)}\n${body}` : str(i.file_path);
  }
  return JSON.stringify(input, null, 2);
}

export type EndAction = "review" | "settings" | null;
export type End = { sentence: string; files: number; action: EndAction; details: string | null; denials: Denial_Deserialize[] };

const BLOCKED: Partial<Record<Outcome, string>> = {
  notInstalled: "Blocked: Claude Code isn't installed or isn't on your PATH.",
  unauthenticated: "Blocked: Claude Code isn't signed in.",
  expiredLogin: "Blocked: your Claude login has expired.",
  incompatibleCli: "Blocked: this Claude Code version isn't supported.",
  missingDependency: "Blocked: a program Claude Code needs is missing.",
};

/** Reason-to-copy map for every outcome and job state (D49). `idleMinutes` is the profile's inactivity limit. */
export function endCopy(outcome: Outcome | null, state: SessionSummary["jobState"], idleMinutes: number | null): string {
  if (outcome && BLOCKED[outcome]) return BLOCKED[outcome]!;
  switch (outcome) {
    case "timeout":
      return `Stopped: no activity for ${idleMinutes ? `${idleMinutes} minute${idleMinutes === 1 ? "" : "s"}` : "too long"}.`;
    case "budgetExceeded": return "Stopped: the run reached its budget limit.";
    case "cancelled": return "Cancelled.";
    case "failed": return "Failed.";
    case "success": return "Done.";
    default:
  }
  if (state === "interrupted") return "Interrupted when Studio closed.";
  if (state === "cancelled") return "Cancelled.";
  if (state === "failed") return "Failed.";
  return "Done.";
}

/** The final row for a finished run, or null while it is still going. */
export function buildEnd(s: SessionSummary, events: AgentEvent[], idleSecs: number | null): End | null {
  if (["queued", "running", "waitingForApproval"].includes(s.jobState)) return null;
  const done = [...events].reverse().find((e) => e.kind === "completed");
  const err = [...events].reverse().find((e) => e.kind === "error");
  const outcome = done?.kind === "completed" ? done.outcome : err?.kind === "error" ? err.errorKind : null;
  const files = changedFiles(events);
  const sentence = endCopy(outcome ?? (s.jobState === "succeeded" ? "success" : null), s.jobState, idleSecs ? Math.round(idleSecs / 60) : null);
  const blocked = !!outcome && outcome in BLOCKED;
  const action: EndAction = blocked ? "settings" : files > 0 || s.jobState === "interrupted" ? "review" : null;
  const details = err?.kind === "error" ? err.message : (s.errorMessage ?? null);
  return { sentence, files, action, details, denials: done?.kind === "completed" ? done.denials : [] };
}

export const filesText = (n: number) => (n === 0 ? "No files changed" : `${n} file${n === 1 ? "" : "s"} changed`);

// --- remembered mode per project (D48) ---
export function loadMode(projectId: string): SessionMode | null {
  try {
    const v = localStorage.getItem(`studio.mode.${projectId}`);
    return v === "plan" || v === "changes" ? v : null;
  } catch {
    return null;
  }
}
export function saveMode(projectId: string, mode: SessionMode) {
  try {
    localStorage.setItem(`studio.mode.${projectId}`, mode);
  } catch {
    /* storage unavailable */
  }
}
