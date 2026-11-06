import type { BuildPhase, BuildStatus, ExportFileKind, ExportJob, ExportTarget, InstallState, ReleaseView } from "../../bindings";
import type { Step } from "../../lib/preview";

export const STEP_LABELS = ["Dependencies", "Compile", "Package", "Test launch"];
const PHASE_STEP: Record<BuildPhase, number> = { preflight: -1, installing: 0, compiling: 1, packaging: 2, storing: 2, testing: 3, environment: -1, exportLinux: -1, exportWindows: -1 };

/** Backend phases onto the four steps users see (M4-DR4); an export job gets its own stepper (F22). */
export function buildSteps(phase: BuildPhase | null | undefined, job?: Pick<ExportJob, "targets" | "firstRun"> | null): Step[] {
  if (job) return exportSteps(phase, job);
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

// --- exports for other systems (F22) ---

export const TARGET_LABELS: Record<ExportTarget, string> = { linux: "Other Linux", windows: "Windows 11" };
/** The short name in sentences and failure lines ("Windows export failed"). */
export const TARGET_SHORT: Record<ExportTarget, string> = { linux: "Other Linux", windows: "Windows" };
const TARGET_ORDER: ExportTarget[] = ["linux", "windows"];

export const FILE_PURPOSE: Record<ExportFileKind, string> = {
  appImage: "Runs on most distros (glibc 2.35+)",
  deb: "Ubuntu, Debian, Mint",
  rpm: "Fedora, openSUSE",
  nsis: 'Installer. Unsigned: Windows shows "More info → Run anyway".',
};
export const TARGET_CAPTION: Record<ExportTarget, string> = {
  linux: "Not started by Studio.",
  windows: "Studio can't start Windows apps; try it on a Windows 11 PC.",
};

/** "Other Linux", "Other Linux and Windows"; only the chosen targets. */
export const targetList = (ts: ExportTarget[], labels: Record<ExportTarget, string> = TARGET_SHORT) =>
  TARGET_ORDER.filter((t) => ts.includes(t)).map((t) => labels[t]).join(" and ");

const EXPORT_PHASE: Partial<Record<BuildPhase, "environment" | "deps" | ExportTarget>> = {
  environment: "environment", installing: "deps", exportLinux: "linux", exportWindows: "windows",
};
/** Environment (first export only), Dependencies, then the chosen targets. */
export function exportSteps(phase: BuildPhase | null | undefined, job: Pick<ExportJob, "targets" | "firstRun">): Step[] {
  const keys: { key: string; label: string }[] = [
    ...(job.firstRun ? [{ key: "environment", label: "Environment" }] : []),
    { key: "deps", label: "Dependencies" },
    ...TARGET_ORDER.filter((t) => job.targets.includes(t)).map((t) => ({ key: t, label: t === "linux" ? "Linux" : "Windows" })),
  ];
  const now = phase ? EXPORT_PHASE[phase] : undefined;
  const at = now ? keys.findIndex((k) => k.key === now) : -1;
  return keys.map((k, i) => ({ label: k.label, state: i < at ? "done" : i === at ? "active" : "pending" }));
}

/** The Ask Claude to fix prompt after a failed target: Claude can't run the export here, so it says how the fix is checked (RS5). */
export function exportFixPrompt(target: ExportTarget, name: string, version: string, message: string | null | undefined, lines: string[]): string {
  const err = firstLine(message);
  const i = err ? lines.findIndex((l) => l.includes(err)) : -1;
  const quoted = [err, ...(i < 0 ? [] : lines.slice(i + 1, i + 5).filter((l) => l.trim()))].filter(Boolean).join("\n");
  return `The ${TARGET_SHORT[target]} export of ${name} ${version} failed${quoted ? ` with this error:\n\n${quoted}\n` : "."}\n` +
    `Please fix it in the project. You can't run the ${TARGET_SHORT[target]} export here; I will check the fix by exporting again.`;
}

const sameDay = (a: Date, b: Date) => a.toDateString() === b.toDateString();
/** "Today 10:41", "Yesterday 10:41" or "3 Nov 10:41". */
export function exportTime(iso: string, now = new Date()): string {
  const d = new Date(iso);
  const clock = d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  if (sameDay(d, now)) return `Today ${clock}`;
  if (sameDay(d, new Date(now.getTime() - 86_400_000))) return `Yesterday ${clock}`;
  return `${d.toLocaleDateString(undefined, { day: "numeric", month: "short" })} ${clock}`;
}

/** The fix per Docker case, shared by the export dialog and the Doctor row (design 5, 10). */
export const DOCKER_FIX = {
  notRunning: { text: "Docker isn't running. Start it with:", command: "systemctl start docker" },
  noPermission: { text: "Docker is installed, but your user can't use it. Add yourself to the docker group, then log out and back in.", command: null },
  missing: { text: "Exports need Docker. Install the docker package.", command: null },
} as const;
