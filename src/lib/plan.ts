/** Project plan (F15): the prompts Studio prepares and how a task run is recognized again. Task parsing lives in Rust only. */

export const PLAN_FILE = ".ai-app-studio/plan.md";

/** A task as Studio sends it to Claude. */
export type TaskRef = { text: string; detail: string };

/** "Make a plan" (Plan tab empty state, New Project wizard): a Plan first prompt that asks for a checklist. */
export function planPrompt(description: string): string {
  return [
    `Make a plan for this app: ${description.trim()}`,
    "",
    'Write the plan in Markdown: a short summary paragraph first, then one line per task in the form "- [ ] Task", in the order to build them. Put any details for a task on lines indented by two spaces under it. Each task should be small enough to build and try in one Make changes run.',
  ].join("\n");
}

/** Start (Plan tab) and Start next (Chat): the prompt of a Make changes run for one task. */
const TASK_PREFIX = "Do this task from the project plan: ";

export function taskPrompt(task: TaskRef): string {
  return task.detail ? `${TASK_PREFIX}${task.text}\n${task.detail}` : `${TASK_PREFIX}${task.text}`;
}

/** The task title a run was started for, or null when the run wasn't started from the plan. */
export function taskOfPrompt(prompt: string): string | null {
  if (!prompt.startsWith(TASK_PREFIX)) return null;
  const title = prompt.slice(TASK_PREFIX.length).split("\n", 1)[0]!.trim();
  return title || null;
}

/** Problem state "Studio can't read …": a Make changes request to repair the file. */
export function fixPlanPrompt(reason: string): string {
  return `Studio can't read the project plan in ${PLAN_FILE} (${reason}). Fix the file so it is readable Markdown with tasks as "- [ ] Task" lines. Don't change anything else.`;
}

/** Problem state "This plan has no tasks yet.": a Plan first request for a checklist. */
export function addTasksPrompt(): string {
  return `The project plan in ${PLAN_FILE} has no checklist lines. Write it again as a short summary followed by tasks in the form "- [ ] Task", each small enough for one Make changes run.`;
}
