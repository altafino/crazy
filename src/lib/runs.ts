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

const MODE_LABELS = { plan: "Plan first", changes: "Make changes" } as const;

/** "Today 14:32 · Make changes · Succeeded" (D47 run picker). */
export function runLabel(s: SessionSummary, now = new Date()): string {
  const d = new Date(s.startedAt ?? s.createdAt);
  const time = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  const sameDay = d.toDateString() === now.toDateString();
  const day = sameDay ? "Today" : d.toLocaleDateString([], { day: "numeric", month: "short" });
  return `${day} ${time} · ${MODE_LABELS[s.mode]} · ${STATE_LABELS[s.jobState]}`;
}
