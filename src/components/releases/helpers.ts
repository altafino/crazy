import type { BuildPhase, BuildStatus, InstallState, ReleaseView } from "../../bindings";
import type { Step } from "../../lib/preview";

export const STEP_LABELS = ["Dependencies", "Compile", "Package", "Test launch"];
const PHASE_STEP: Record<BuildPhase, number> = { preflight: -1, installing: 0, compiling: 1, packaging: 2, storing: 2, testing: 3, environment: -1, exportLinux: -1, exportWindows: -1 };

/** Backend phases onto the four steps users see (M4-DR4). */
export function buildSteps(phase: BuildPhase | null | undefined): Step[] {
  const at = phase ? PHASE_STEP[phase] : -1;
  return STEP_LABELS.map((label, i) => ({ label, state: i < at ? "done" : i === at ? "active" : "pending" }));
}

export const compareVersions = (a: string, b: string) => {
  const pa = a.split(/[.+-]/).map((x) => parseInt(x, 10) || 0);
  const pb = b.split(/[.+-]/).map((x) => parseInt(x, 10) || 0);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (d) return d;
  }
  return 0;
};

export const highestVersion = (rs: ReleaseView[]) => rs.reduce<string | null>((h, r) => (h === null || compareVersions(r.version, h) > 0 ? r.version : h), null);

export const activeId = (s: InstallState | null | undefined) => (s && (s.kind === "installed" || s.kind === "launcherMissing") ? s.releaseId ?? null : null);

const byNewest = (a: ReleaseView, b: ReleaseView) => b.createdAt.localeCompare(a.createdAt);
export const kept = (rs: ReleaseView[]) => rs.filter((r) => !r.filesRemoved).sort(byNewest);

/** Previous = the non-active kept release with the latest activation (M4-DR11). */
export function previousId(rs: ReleaseView[], active: string | null): string | null {
  const c = kept(rs).filter((r) => r.id !== active && r.lastActivatedAt);
  return c.sort((a, b) => b.lastActivatedAt!.localeCompare(a.lastActivatedAt!))[0]?.id ?? null;
}

/** A kept release newer than the active one (any, when nothing is installed) whose test launch passed. */
export function installCandidate(rs: ReleaseView[], active: string | null): ReleaseView | null {
  const cur = rs.find((r) => r.id === active);
  return kept(rs).find((r) => r.smoke === "passed" && r.id !== active && (!cur || r.createdAt > cur.createdAt)) ?? null;
}

export type RowAction = "install" | "installAnyway" | "activate" | "rollback" | null;
export function rowAction(r: ReleaseView, active: string | null, installed: boolean, activeCreatedAt: string | null): RowAction {
  if (r.filesRemoved || r.id === active || r.smoke === "failed") return null;
  if (r.smoke === "unverified") return "installAnyway";
  if (!installed) return "install";
  return activeCreatedAt && r.createdAt < activeCreatedAt ? "rollback" : "activate";
}

export const ACTION_LABELS: Record<Exclude<RowAction, null>, string> = {
  install: "Install", installAnyway: "Install anyway", activate: "Activate", rollback: "Roll back",
};

export const firstLine = (s: string | null | undefined) => (s ?? "").split("\n").find((l) => l.trim())?.trim() ?? "";

/** What went wrong in a failed build, as a single line. */
export const failureText = (s: BuildStatus) => firstLine(s.errorMessage) || "The build failed. See the log for details.";

export function elapsed(from: string | null, to: number) {
  if (!from) return "";
  const secs = Math.max(0, Math.floor((to - new Date(from).getTime()) / 1000));
  return secs >= 60 ? `${Math.floor(secs / 60)}m ${secs % 60}s` : `${secs}s`;
}
