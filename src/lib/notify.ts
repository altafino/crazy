import { getCurrentWindow } from "@tauri-apps/api/window";
import { isPermissionGranted, onAction, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { watch } from "vue";
import { commands, type AgentEvent } from "../bindings";
import { call, type Outcome } from "./call";
import { craftTool } from "./chat";
import { useBuildsStore } from "../stores/builds";
import { useCraftsStore } from "../stores/crafts";
import { useNavStore, type ProjectTab } from "../stores/nav";
import { usePreviewStore } from "../stores/preview";
import { useProjectsStore } from "../stores/projects";
import { useRunsStore } from "../stores/runs";

/** "run a command", "edit a file", ... for an approval request (D54, D56). */
export function wantsTo(tool: string): string {
  if (tool === "Bash") return "run a command";
  if (["Edit", "Write", "MultiEdit", "NotebookEdit"].includes(tool)) return "edit a file";
  const craft = craftTool(tool);
  return craft ? `use ${useCraftsStore().nameOf(craft.id)}` : `use ${tool}`;
}

/** One desktop notification per approval, only while the window is unfocused (D54). Fails silently. */
export async function notifyApproval(projectName: string, e: AgentEvent): Promise<void> {
  if (e.kind !== "approvalNeeded" || document.hasFocus()) return;
  try {
    if (!(await isPermissionGranted()) && (await requestPermission()) !== "granted") return;
    sendNotification({ title: "AI App Studio", body: `${projectName}: Claude wants to ${wantsTo(e.tool)}` });
  } catch {
    /* notifications are a nicety */
  }
}

/**
 * Settings > General "Notify when runs and builds finish" (D12). The backend setting `notify_finish` (D8),
 * read at startup (lib/startup.ts); on until then. Approvals ignore it.
 */
let finishOn = true;
export function notifyFinishOn(): boolean {
  return finishOn;
}
/** Sets the in-memory switch (startup and tests); `saveNotifyFinish` also stores it. */
export function setNotifyFinish(on: boolean) {
  finishOn = on;
}
/** Stores the switch in the backend settings, then applies it. */
export async function saveNotifyFinish(on: boolean): Promise<Outcome<null>> {
  const r = await call(commands.setNotifyFinish(on));
  if (r.ok) finishOn = on;
  return r;
}

/** A finish notification (D12): only while the window is unfocused and the switch is on; a click opens `tab` of the project. */
export async function notifyFinish(projectId: string, tab: ProjectTab, body: string): Promise<void> {
  if (!notifyFinishOn() || document.hasFocus()) return;
  try {
    if (!(await isPermissionGranted()) && (await requestPermission()) !== "granted") return;
    sendNotification({ title: "AI App Studio", body, extra: { projectId, tab } });
  } catch {
    /* notifications are a nicety */
  }
}

const nameOf = (id: string) => useProjectsStore().projects.find((p) => p.id === id)?.name ?? "The app";

/** Run ended: "3 files changed" or failed; cancelled runs stay quiet. */
function runEnded(projectId: string, jobId: string, outcome: string) {
  if (outcome === "cancelled") return;
  const runs = useRunsStore();
  const sid = runs.sessions[projectId]?.find((s) => s.jobId === jobId)?.id;
  const files = new Set((runs.eventsBySession[sid ?? ""] ?? []).flatMap((e) => (e.kind === "fileChange" ? [e.path] : []))).size;
  const done = files ? `run finished · ${files} ${files === 1 ? "file" : "files"} changed` : "run finished";
  void notifyFinish(projectId, "chat", `${nameOf(projectId)}: ${outcome === "success" ? done : "run failed"}`);
}

/** First preview start per job and build ends, from the stores' status maps (no edits to the tabs). */
function installFinishNotifications(): void {
  const preview = usePreviewStore();
  const builds = useBuildsStore();
  const announced = new Set<string>();
  watch(() => preview.status, (all) => {
    for (const [pid, s] of Object.entries(all)) {
      if (!s?.live || s.phase?.phase !== "appRunning" || announced.has(s.jobId)) continue;
      announced.add(s.jobId);
      void notifyFinish(pid, "preview", `${nameOf(pid)} preview is running`);
    }
  }, { deep: true });
  const live: Record<string, boolean> = {};
  watch(() => builds.status, (all) => {
    for (const [pid, s] of Object.entries(all)) {
      if (!s) continue;
      const was = live[pid];
      live[pid] = s.live;
      if (!was || s.live || s.export || s.state === "cancelled") continue;
      void (async () => {
        await builds.loadInstall(pid);
        const v = s.version ? `${nameOf(pid)} ${s.version}` : nameOf(pid);
        const text = s.state !== "succeeded" ? "build failed" : builds.installedVersion(pid) === s.version ? "installed" : "is built";
        void notifyFinish(pid, "releases", `${v} ${text}`);
      })();
    }
  }, { deep: true });
  // Clicking a notification brings Studio to that project's tab (desktop actionId "tap").
  try {
    void onAction((n) => {
      const { projectId, tab } = (n.notification?.extra ?? {}) as { projectId?: string; tab?: ProjectTab };
      if (!projectId || !tab) return;
      void getCurrentWindow().setFocus().catch(() => {});
      const nav = useNavStore();
      nav.openProject(projectId);
      nav.tab = tab;
    }).catch(() => {});
  } catch {
    /* no plugin (tests, browser) */
  }
}

/** Hooks approval and finish notifications into the stores; call once at app start. */
export function installApprovalNotifications(): void {
  const runs = useRunsStore();
  const projects = useProjectsStore();
  runs.setOnEvent((jobId, e) => {
    const pid = Object.keys(runs.sessions).find((p) => runs.sessions[p]!.some((s) => s.jobId === jobId));
    const name = projects.projects.find((p) => p.id === pid)?.name ?? "A project";
    void notifyApproval(name, e);
    if (pid && e.kind === "completed") runEnded(pid, jobId, e.outcome);
  });
  installFinishNotifications();
}
