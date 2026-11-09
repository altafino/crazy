import { ref } from "vue";
import { defineStore } from "pinia";
import { commands } from "../bindings";
import type { Plan_Serialize, PlanTask } from "../bindings";
import { call } from "../lib/call";
import { taskOfPrompt, taskPrompt } from "../lib/plan";
import { useBuildsStore } from "./builds";
import { useNavStore } from "./nav";
import { isActive, useRunsStore } from "./runs";

/** The project plan (F15): one copy per project, shared by the Plan tab, the tab count and Chat. */
export const usePlanStore = defineStore("plan", () => {
  /** undefined: not loaded; null: no plan file. */
  const plans = ref<Record<string, Plan_Serialize | null | undefined>>({});
  /** Raw reason when the plan file can't be read (shown under Details). */
  const readErrors = ref<Record<string, string | null>>({});

  async function load(projectId: string) {
    const r = await call(commands.projectPlan(projectId));
    if (r.ok) {
      plans.value[projectId] = r.data;
      readErrors.value[projectId] = null;
    } else {
      plans.value[projectId] = null;
      readErrors.value[projectId] = r.error.message;
    }
    return r;
  }

  /** "Use as project plan". */
  async function save(projectId: string, text: string) {
    const r = await call(commands.saveProjectPlan(projectId, text));
    if (r.ok) {
      plans.value[projectId] = r.data;
      readErrors.value[projectId] = null;
    }
    return r;
  }

  /** Tick or untick by task text (D8); on any refusal the plan is read again so the list is current. */
  async function setTask(projectId: string, text: string, done: boolean) {
    const r = await call(commands.setPlanTask(projectId, text, done));
    if (r.ok) plans.value[projectId] = r.data;
    else await load(projectId);
    return r;
  }

  function progress(projectId: string): { done: number; total: number } | null {
    const p = plans.value[projectId];
    if (!p || p.tasks.length === 0) return null;
    return { done: p.tasks.filter((t) => t.done).length, total: p.tasks.length };
  }

  /** The first open task, or null. */
  function nextTask(projectId: string, after?: string): PlanTask | null {
    return plans.value[projectId]?.tasks.find((t) => !t.done && t.text !== after) ?? null;
  }

  function isOpen(projectId: string, text: string): boolean {
    return !!plans.value[projectId]?.tasks.some((t) => t.text === text && !t.done);
  }

  /** The task an active Start run works on ("In progress", 2D). */
  function runningTask(projectId: string): string | null {
    const s = useRunsStore().sessions[projectId]?.find(isActive);
    return s && s.mode === "changes" ? taskOfPrompt(s.prompt) : null;
  }

  /** Why ticking and Start are off right now (2C), or null. */
  function blockedReason(projectId: string): string | null {
    if ((useRunsStore().sessions[projectId] ?? []).some(isActive)) return "Wait until the current run finishes";
    if (useBuildsStore().isBuilding(projectId)) return "Wait until the release build finishes";
    return null;
  }

  /** Start and Start next: a Make changes run for one task in a new conversation (D14), then Chat shows it. */
  async function startTask(projectId: string, task: PlanTask) {
    const a = await call(commands.getApprovalSettings());
    const r = await useRunsStore().start(projectId, taskPrompt(task), "changes", null, undefined, a.ok && a.data.autoApprove, true);
    if (r.ok) useNavStore().tab = "chat";
    return r;
  }

  return { plans, readErrors, load, save, setTask, progress, nextTask, isOpen, runningTask, blockedReason, startTask };
});
