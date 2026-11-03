import type { PreviewLimits, PreviewPhase, PreviewStatus } from "../bindings";

/** The backend writes this note (services/preview/mod.rs INSTALL_SKIPPED) into the log when it skips the install (M3-D28). */
export const INSTALL_SKIPPED_PREFIX = "Dependencies not refreshed";

export type StepState = "done" | "active" | "pending" | "failed" | "skipped";
export type Step = { label: string; state: StepState };

/** Installing — Compiling — App running; the lock-wait phase renames the active step (M3-D28). */
export function stepStates(phase: PreviewPhase | null | undefined, installSkipped: boolean): Step[] {
  const p = phase?.phase ?? "starting";
  const install: StepState = installSkipped ? "skipped" : "done";
  const steps: Step[] = [
    { label: "Installing", state: "pending" },
    { label: "Compiling", state: "pending" },
    { label: "App running", state: "pending" },
  ];
  const set = (a: StepState, b: StepState, c: StepState) => steps.forEach((s, i) => (s.state = [a, b, c][i]!));
  if (p === "installing") set("active", "pending", "pending");
  else if (p === "compiling") set(install, "active", "pending");
  else if (p === "waitingForRustBuild") {
    set(install, "active", "pending");
    steps[1]!.label = "Waiting for another Rust build";
  } else if (p === "appRunning") set(install, "done", "done");
  else if (p === "buildFailed") set(install, "failed", "pending");
  return steps;
}

export const installSkipped = (lines: string[]) => lines.some((l) => l.startsWith(INSTALL_SKIPPED_PREFIX));

/** Dot on the Preview tab and badge on the card (M3-D48): the tone plus text, never color alone. */
export type Indicator = { tone: "accent" | "ok" | "danger"; label: string };
export function indicator(s: PreviewStatus | null | undefined): Indicator | null {
  if (!s?.live) return null;
  const p = s.phase?.phase;
  if (p === "buildFailed") return { tone: "danger", label: "build failed" };
  if (p === "appRunning") return { tone: "ok", label: "running" };
  return { tone: "accent", label: "starting" };
}

/** How an ended preview is described (M3-D40). */
export function endedText(s: PreviewStatus): string {
  switch (s.state) {
    case "cancelled": return "Stopped";
    case "succeeded": return "Closed";
    case "interrupted": return "Interrupted when Studio closed";
    default: return s.errorMessage || "Crashed";
  }
}

/** Text for the composer handoff: the first error plus its `--> file:line:col` location, found in the log (M3-D43). */
export function fixPrompt(firstError: string | null | undefined, lines: string[]): string {
  const err = firstError?.trim();
  if (!err) return "The preview build failed. Please look at the build output and fix it.";
  const i = lines.findIndex((l) => l.includes(err));
  const loc = i < 0 ? null : lines.slice(i + 1, i + 6).map((l) => /^\s*-->\s*(.+)$/.exec(l)?.[1]).find(Boolean);
  return `The preview build failed with this error:\n\n${err}\n${loc ? `Location: ${loc}\n` : ""}\nPlease fix it.`;
}

/** First line of the prompt, about 72 characters, as the draft commit message (M3-D42). */
export function commitMessage(prompt: string): string {
  const first = (prompt.split("\n").find((l) => l.trim()) ?? "").trim();
  return first.length <= 72 ? first : `${first.slice(0, 71).trimEnd()}…`;
}

export const DEFAULT_LIMITS: PreviewLimits = { maxLines: 5000, maxLogMb: 10, keepLogs: 5 };
export const LIMIT_RANGES = {
  maxLines: { min: 1000, max: 50000, label: "Lines kept in memory" },
  maxLogMb: { min: 1, max: 100, label: "Log file size (MB)" },
  keepLogs: { min: 1, max: 50, label: "Logs kept per project" },
} as const;

/** Field errors for out-of-range or non-integer limits; empty when valid. */
export function validateLimits(l: Record<keyof PreviewLimits, unknown>): Partial<Record<keyof PreviewLimits, string>> {
  const out: Partial<Record<keyof PreviewLimits, string>> = {};
  for (const k of Object.keys(LIMIT_RANGES) as (keyof PreviewLimits)[]) {
    const { min, max } = LIMIT_RANGES[k];
    const v = l[k];
    if (typeof v !== "number" || !Number.isInteger(v) || v < min || v > max) out[k] = `Enter a whole number from ${min.toLocaleString("en-US")} to ${max.toLocaleString("en-US")}.`;
  }
  return out;
}
