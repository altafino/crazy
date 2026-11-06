import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import type { Plan_Serialize } from "../bindings";

const { commands, ev } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>>, ev: { current: null as unknown } }));
vi.mock("../bindings", () => ({ commands, get events() { return (ev.current as { events: unknown }).events; } }));

import { err, mockCommands, mockEvents, ok, project, session } from "../test-helpers";
import { taskPrompt } from "../lib/plan";
import { useNavStore } from "../stores/nav";
import { useRunsStore } from "../stores/runs";
import { useToastStore } from "../stores/toast";
import PlanTab from "./PlanTab.vue";

const plan = (done: boolean[], over: Partial<Plan_Serialize> = {}): Plan_Serialize => ({
  summary: "A maze game.",
  savedAt: "2026-11-02T14:02:00Z",
  tasks: done.map((d, i) => ({ text: `Task ${i + 1}`, detail: d ? "" : `Detail ${i + 1}`, done: d })),
  ...over,
});

async function setup(p: Plan_Serialize | null | "unreadable", sessions = [] as ReturnType<typeof session>[]) {
  commands.projectPlan!.mockImplementation(() => (p === "unreadable" ? err("line 3: bad") : ok(p)));
  commands.listSessions!.mockImplementation(() => ok(sessions));
  const pinia = createPinia();
  setActivePinia(pinia);
  const nav = useNavStore();
  nav.openProject("p1");
  nav.tab = "plan";
  await useRunsStore().loadSessions("p1");
  const w = mount(PlanTab, { props: { project: project() }, global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return { w, nav, toast: useToastStore() };
}
type W = Awaited<ReturnType<typeof setup>>["w"];
const btn = (w: W, label: string) => w.findAll("button").find((b) => b.text() === label || b.attributes("aria-label") === label);

beforeEach(() => {
  document.body.innerHTML = "";
  Object.assign(commands, mockCommands());
  ev.current = mockEvents();
});

describe("Plan tab states", () => {
  it("shows Reading the plan… while loading", async () => {
    commands.projectPlan!.mockImplementation(() => new Promise(() => {}));
    const pinia = createPinia();
    setActivePinia(pinia);
    useNavStore().openProject("p1");
    const w = mount(PlanTab, { props: { project: project() }, global: { plugins: [pinia] } });
    await flushPromises();
    expect(w.text()).toContain("Reading the plan…");
    expect(w.find(".spin").exists()).toBe(false);
  });

  it("empty: Make a plan is disabled until typed, then hands off to Chat in Plan first", async () => {
    const { w, nav } = await setup(null);
    expect(w.text()).toContain("No plan yet. Describe your app and Claude will break it into tasks.");
    const area = w.get("textarea");
    expect(area.attributes("rows")).toBe("4");
    expect(w.get("label[for='plan-desc']").text()).toBe("What should this app do?");
    expect(btn(w, "Make a plan")!.attributes("disabled")).toBeDefined();
    await area.setValue("a maze game");
    await btn(w, "Make a plan")!.trigger("click");
    expect(nav.composerHandoff!.mode).toBe("plan");
    expect(nav.composerHandoff!.projectId).toBe("p1");
    expect(nav.composerHandoff!.text).toContain("a maze game");
    expect(nav.tab).toBe("chat");
  });

  it("unreadable: danger callout, Ask Claude to fix hands off in Make changes, reason under Details", async () => {
    const { w, nav } = await setup("unreadable");
    expect(w.get(".callout.danger").text()).toContain("Studio can't read .ai-app-studio/plan.md");
    expect(w.get("details pre").text()).toBe("line 3: bad");
    await btn(w, "Ask Claude to fix")!.trigger("click");
    expect(nav.composerHandoff!.mode).toBeUndefined();
    expect(nav.composerHandoff!.text).toContain("line 3: bad");
  });

  it("no tasks: explains and offers Ask Claude to add tasks in Plan first", async () => {
    const { w, nav } = await setup(plan([]));
    expect(w.text()).toContain("This plan has no tasks yet.");
    await btn(w, "Ask Claude to add tasks")!.trigger("click");
    expect(nav.composerHandoff!.mode).toBe("plan");
  });
});

describe("Plan tab checklist", () => {
  it("shows the status row, progress, saved caption, hint and a primary Start only on the first open task", async () => {
    const { w } = await setup(plan([true, false, false]));
    expect(w.get("#plan-sentence").text()).toBe("1 of 3 tasks done");
    const bar = w.get("progress");
    expect(bar.attributes("value")).toBe("1");
    expect(bar.attributes("max")).toBe("3");
    expect(bar.attributes("aria-labelledby")).toBe("plan-sentence");
    expect(w.text()).toMatch(/From \.ai-app-studio\/plan\.md · saved \d/);
    expect(w.text()).toContain("To change the plan, ask in Chat with Plan first, then press Use as project plan.");
    expect(w.find("ul[aria-label='Tasks']").findAll("li")).toHaveLength(3);
    expect(btn(w, "Start: Task 2")!.classes()).toContain("primary");
    expect(btn(w, "Start: Task 3")!.classes()).not.toContain("primary");
    expect(btn(w, "Build release…")).toBeUndefined();
  });

  it("a done row has no Start and no detail; open rows link their detail", async () => {
    const { w } = await setup(plan([true, false]));
    const rows = w.findAll("li");
    expect(rows[0]!.find("button").exists()).toBe(false);
    expect(rows[0]!.find(".detail").exists()).toBe(false);
    expect(rows[0]!.get("input").element.checked).toBe(true);
    expect(rows[1]!.get(".detail").text()).toBe("Detail 2");
    expect(rows[1]!.get("input").attributes("aria-describedby")).toBe("task-d1");
    expect(rows[1]!.get("input").attributes("aria-labelledby")).toBe("task-t1");
  });

  it("all done: sentence, ok progress and Build release… opens the build dialog", async () => {
    const { w, nav } = await setup(plan([true, true]));
    expect(w.get("#plan-sentence").text()).toBe("All 2 tasks done");
    expect(w.get("progress").classes()).toContain("done");
    expect(w.findAll("li button")).toHaveLength(0);
    await btn(w, "Build release…")!.trigger("click");
    expect(nav.buildDialogProject).toBe("p1");
    expect(nav.tab).toBe("releases");
  });

  it("shows the first summary paragraph and the rest behind Show full summary", async () => {
    const { w } = await setup(plan([false], { summary: "First paragraph.\n\nSecond paragraph." }));
    const d = w.get("details");
    expect(d.get("summary").text()).toBe("Show full summary");
    expect(d.text()).toContain("Second paragraph.");
    expect(w.get(".summary").findAll(".md")[0]!.text()).toBe("First paragraph.");
    const single = await setup(plan([false]));
    expect(single.w.text()).not.toContain("Show full summary");
  });
});

describe("Plan tab actions", () => {
  it("ticking saves by title, announces politely and keeps the box checked", async () => {
    const { w } = await setup(plan([true, false, false]));
    commands.setPlanTask!.mockImplementation(() => ok(plan([true, true, false])));
    await w.findAll("input[type='checkbox']")[1]!.setValue(true);
    await flushPromises();
    expect(commands.setPlanTask).toHaveBeenCalledWith("p1", "Task 2", true);
    expect(w.get("#plan-sentence").text()).toBe("2 of 3 tasks done");
    const live = w.get(".sr-only[role='status']");
    expect(live.attributes("aria-live")).toBe("polite");
    expect(live.text()).toBe("Task 2 marked done, 2 of 3");
  });

  it("a task that is gone shows a toast and the list reloads", async () => {
    const { w, toast } = await setup(plan([false, false]));
    commands.setPlanTask!.mockImplementation(() => err("This task is no longer in the plan."));
    commands.projectPlan!.mockImplementation(() => ok(plan([false])));
    await w.findAll("input[type='checkbox']")[0]!.setValue(true);
    await flushPromises();
    expect(toast.current?.text).toBe("This task is no longer in the plan.");
    expect(w.findAll("li")).toHaveLength(1);
  });

  it("Start runs the task in Make changes and switches to Chat", async () => {
    const { w, nav } = await setup(plan([true, false]));
    commands.startRun!.mockImplementation(() => ok("j9"));
    await btn(w, "Start: Task 2")!.trigger("click");
    await flushPromises();
    expect(commands.startRun!.mock.calls[0]!.slice(0, 3)).toEqual(["p1", taskPrompt({ text: "Task 2", detail: "Detail 2" }), "changes"]);
    expect(nav.tab).toBe("chat");
  });

  it("a Start that fails shows a toast", async () => {
    const { w, toast } = await setup(plan([false]));
    commands.startRun!.mockImplementation(() => err("Claude isn't installed"));
    await btn(w, "Start: Task 1")!.trigger("click");
    await flushPromises();
    expect(toast.current?.text).toBe("Claude isn't installed");
  });
});

describe("Plan tab while a run is active", () => {
  const running = session({ jobState: "running", mode: "changes", prompt: taskPrompt({ text: "Task 2", detail: "Detail 2" }) });

  it("disables checkboxes and Start, explains why once and marks the Start run's task In progress", async () => {
    const { w } = await setup(plan([true, false, false]), [running]);
    expect(w.text()).toContain("Wait until the current run finishes");
    expect(w.findAll("input[type='checkbox']").every((i) => i.attributes("disabled") !== undefined)).toBe(true);
    expect(w.findAll(".caption").filter((c) => c.text() === "Wait until the current run finishes")).toHaveLength(1);
    expect(w.get("#task-1").attributes("aria-describedby")).toContain("plan-why");
    const rows = w.findAll("li");
    expect(rows[1]!.get(".badge.info").text()).toBe("In progress");
    expect(rows[1]!.find("button").exists()).toBe(false);
    expect(btn(w, "Start: Task 3")!.attributes("disabled")).toBeDefined();
    expect(btn(w, "Start: Task 3")!.attributes("aria-describedby")).toBe("plan-why");
  });

  it("reloads the plan when the run ends", async () => {
    const { w } = await setup(plan([true, false]), [running]);
    commands.projectPlan!.mockClear();
    commands.projectPlan!.mockImplementation(() => ok(plan([true, true])));
    useRunsStore().sessions.p1 = [{ ...running, jobState: "succeeded" }];
    await flushPromises();
    expect(commands.projectPlan).toHaveBeenCalledTimes(1);
    expect(w.get("#plan-sentence").text()).toBe("All 2 tasks done");
  });
});
