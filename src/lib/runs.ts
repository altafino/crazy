import type { JobState, SessionSummary } from "../bindings";

export const STATE_LABELS: Record<JobState, string> = {
  queued: "Starting",
  running: "Running",
  waitingForApproval: "Waiting",
  succeeded: "Succeeded",
  failed: "Failed",
  cancelled: "Cancelled",
  interrupted: "Interrupted",
};

/** Local HH:MM of a run (start time, else creation time). */
export const runClock = (s: SessionSummary) =>
  new Date(s.startedAt ?? s.createdAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });

/** "Today" or "3 Nov": the day a run began (thread dividers, run labels). */
export function runDay(s: SessionSummary, now = new Date()): string {
  const d = new Date(s.startedAt ?? s.createdAt);
  return d.toDateString() === now.toDateString() ? "Today" : d.toLocaleDateString([], { day: "numeric", month: "short" });
}

/** "Add a dark theme · 14:32 · 3 files", the day first for older days, the state last unless Succeeded (D3). */
export function runLabel(s: SessionSummary, now = new Date(), files?: number): string {
  const first = s.prompt.trim().split("\n")[0]!;
  const text = first.length > 48 ? `${first.slice(0, 48).trimEnd()}…` : first;
  const day = runDay(s, now);
  const parts = [text, day === "Today" ? runClock(s) : `${day} ${runClock(s)}`];
  if (files !== undefined) parts.push(`${files} ${files === 1 ? "file" : "files"}`);
  if (s.jobState !== "succeeded") parts.push(STATE_LABELS[s.jobState]);
  return parts.join(" · ");
}
