import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import type { AgentEvent } from "../bindings";
import { craftTool } from "./chat";
import { useCraftsStore } from "../stores/crafts";
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

/** Hooks approval notifications into the runs store; call once at app start. */
export function installApprovalNotifications(): void {
  const runs = useRunsStore();
  const projects = useProjectsStore();
  runs.setOnEvent((jobId, e) => {
    const pid = Object.keys(runs.sessions).find((p) => runs.sessions[p]!.some((s) => s.jobId === jobId));
    const name = projects.projects.find((p) => p.id === pid)?.name ?? "A project";
    void notifyApproval(name, e);
  });
}
