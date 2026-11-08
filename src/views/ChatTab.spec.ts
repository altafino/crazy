import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import type { AgentEvent, Outcome } from "../bindings";

const { commands, ev, notif } = vi.hoisted(() => ({
  commands: {} as Record<string, ReturnType<typeof vi.fn>>,
  ev: { current: null as unknown },
  notif: { isPermissionGranted: vi.fn(), requestPermission: vi.fn(), sendNotification: vi.fn() },
}));
vi.mock("../bindings", () => ({ commands, get events() { return (ev.current as { events: unknown }).events; } }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(), revealItemInDir: vi.fn() }));
vi.mock("@tauri-apps/plugin-notification", () => notif);

import { err, mockCommands, mockEvents, ok, project, session } from "../test-helpers";
import { renderMarkdown } from "../lib/markdown";
import { installApprovalNotifications } from "../lib/notify";
import { useNavStore } from "../stores/nav";
import { useRunsStore } from "../stores/runs";
import { useToastStore } from "../stores/toast";
import { taskPrompt } from "../lib/plan";
import ChatTab from "./ChatTab.vue";

const profile = { id: "pr1", name: "Claude", provider: "claude", executable: "claude", env: [], model: null, inactivitySecs: 600, instructions: "", isDefault: true, enabled: true };
let bus: ReturnType<typeof mockEvents>;

async function setup(sessions = [session()], evs: AgentEvent[] = [], profiles = [profile]) {
  commands.listSessions!.mockImplementation(() => ok(sessions));
  commands.getSessionEvents!.mockImplementation(() => ok(evs));
  commands.listAgentProfiles!.mockImplementation(() => ok(profiles));
  const pinia = createPinia();
  setActivePinia(pinia);
  await useRunsStore().loadSessions("p1");
  const w = mount(ChatTab, { props: { project: project() }, global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return w;
}
const completed = (outcome: Outcome, denials: never[] = []): AgentEvent => ({ kind: "completed", seq: 99, outcome, denials });
const approval: AgentEvent = { kind: "approvalNeeded", seq: 1, requestId: "r1", tool: "Bash", input: { command: "pnpm add @vueuse/core" }, description: "Install a package" };

beforeEach(() => {
  document.body.innerHTML = "";
  localStorage.clear();
  Object.assign(commands, mockCommands());
  bus = mockEvents();
  ev.current = bus;
  Object.values(notif).forEach((f) => f.mockReset());
});

describe("empty state and mode memory", () => {
  it("explains both modes, preselects Plan first and remembers the last mode", async () => {
    const w = await setup([]);
    expect(w.text()).toContain("Plan first: Claude proposes, no files change");
    const radios = w.findAll<HTMLInputElement>("input[type=radio]");
    expect(radios[0]!.element.checked).toBe(true);
    await radios[1]!.setValue(true);
    expect(localStorage.getItem("studio.mode.p1")).toBe("changes");
    w.unmount();
    const again = await setup([]);
    expect(again.findAll<HTMLInputElement>("input[type=radio]")[1]!.element.checked).toBe(true);
  });

  it("starts a run with the chosen mode and profile", async () => {
    commands.startRun!.mockImplementation(() => ok("j2"));
    const w = await setup([]);
    await w.find("textarea").setValue("Add dark theme");
    await w.find("select").setValue("pr1");
    await w.find("textarea").trigger("keydown", { key: "Enter", ctrlKey: true });
    expect(commands.startRun).toHaveBeenCalledWith("p1", "Add dark theme", "plan", "pr1", null, null, false, false);
  });

  describe("conversations (D6, D14)", () => {
    const link = (w: Awaited<ReturnType<typeof setup>>, text: string) => w.findAll("button.link").find((b) => b.text() === text);

    it("New conversation turns on, can be undone, and is sent once", async () => {
      commands.startRun!.mockImplementation(() => ok("j2"));
      const w = await setup([session()]);
      expect(w.text()).toContain("Continues the conversation");
      await link(w, "New conversation")!.trigger("click");
      expect(w.text()).toContain("New conversation · on");
      await link(w, "Undo")!.trigger("click");
      expect(w.text()).toContain("Continues the conversation");
      await link(w, "New conversation")!.trigger("click");
      await w.find("textarea").setValue("Fresh start");
      await w.find("textarea").trigger("keydown", { key: "Enter", ctrlKey: true });
      expect(commands.startRun).toHaveBeenCalledWith("p1", "Fresh start", "plan", null, null, null, false, true);
      await flushPromises();
      expect(w.text()).not.toContain("New conversation · on");
    });

    it("captions say how each run began", async () => {
      const cap = async (over: Parameters<typeof session>[0]) => (await setup([session(over)])).find(".conv-caption").text();
      expect(await cap({ resumedFrom: "s0" })).toBe("Continues the conversation");
      expect(await cap({ resumedFrom: "s0", resumeFellBack: true })).toBe("New conversation: the previous one couldn't be continued");
      expect(await cap({ conversationStart: "profile" })).toBe("New conversation: different agent profile");
      expect(await cap({ conversationStart: "new", prompt: taskPrompt({ text: "Draw the maze", detail: "" }) })).toBe("New conversation · From the plan: Draw the maze");
    });
  });

  describe("ArtCraft first use (design D22)", () => {
    const row = (id: string, name: string, firstUse: boolean) => ({
      id, name, purpose: "", kind: "appImage" as const, version: "0.18.0", asksEachAction: false, ticked: true, missing: false, firstUse,
    });
    const view = { enabled: true, apps: [row("cadcraft", "CADCraft", true), row("photocraft", "PhotoCraft", false)], notInstalled: [], notWired: [], defaults: [] };
    async function startPending(mode: "plan" | "changes") {
      commands.craftApps!.mockImplementation(() => ok(view));
      let finish = (_: unknown) => {};
      commands.startRun!.mockImplementation(() => new Promise((r) => (finish = r)).then(() => ({ status: "ok", data: "j2" })));
      const w = await setup([]);
      await w.findAll<HTMLInputElement>("input[type=radio]")[mode === "plan" ? 0 : 1]!.setValue(true);
      await w.find("textarea").setValue("Make a coin");
      await w.find("textarea").trigger("keydown", { key: "Enter", ctrlKey: true });
      await flushPromises();
      return { w, finish };
    }

    it("names the apps that unpack while a Make changes run starts, then clears", async () => {
      const { w, finish } = await startPending("changes");
      expect(w.find(".strip").text()).toContain("Preparing CADCraft (first use)…");
      expect(w.find(".strip").text()).not.toContain("PhotoCraft");
      finish(null);
      await flushPromises();
      expect(w.find(".strip").text()).not.toContain("Preparing");
    });

    it("Plan first never prepares ArtCraft apps", async () => {
      const { w } = await startPending("plan");
      expect(w.find(".strip").text()).not.toContain("Preparing");
    });
  });

  it("offers Set up Claude first when there is no profile", async () => {
    const w = await setup([], [], []);
    const btn = w.findAll("button").find((b) => b.text() === "Set up Claude first")!;
    expect(btn).toBeTruthy();
    await btn.trigger("click");
    expect(useNavStore().view).toBe("settings");
    expect(useNavStore().settingsSection).toBe("agents");
  });
});

describe("timeline", () => {
  it("renders live events and collapses consecutive reads", async () => {
    const w = await setup([session({ jobState: "running" })], [{ kind: "text", seq: 0, text: "Looking **around**" }]);
    expect(w.find("[role=log]").exists()).toBe(true);
    const tool = (seq: number, id: string, name: string, summary: string): AgentEvent => ({ kind: "toolCall", seq, id, name, input: {}, summary });
    for (const e of [tool(1, "a", "Read", "a.ts"), tool(2, "b", "Read", "b.ts"), tool(3, "c", "Grep", "foo"), tool(4, "d", "Edit", "src/x.ts")]) await bus.emit("j1", e);
    await flushPromises();
    expect(w.find("[role=log]").text()).toContain("Read 3 files");
    expect(w.find("[role=log]").text()).toContain("src/x.ts");
    expect(w.find("strong").text()).toBe("around");
    await bus.emit("j1", { kind: "toolResult", seq: 5, id: "d", isError: false, output: "OK-RESULT" });
    await w.findAll("button.step").slice(-1)[0]!.trigger("click");
    expect(w.text()).toContain("OK-RESULT");
  });

  it("shows the local time on text, tool and approval rows, and none for old logs", async () => {
    const at = new Date(2026, 10, 2, 14, 30, 21).getTime();
    const w = await setup([session()], [
      { kind: "text", seq: 0, atMs: at, text: "hello" },
      { kind: "toolCall", seq: 1, atMs: at + 1000, id: "a", name: "Edit", input: {}, summary: "Edit: x.ts" },
      { kind: "approvalNeeded", seq: 2, atMs: at + 2000, requestId: "r", tool: "Bash", input: {}, description: "d" },
      { kind: "text", seq: 3, text: "old row without a time" },
    ]);
    const times = w.findAll("time.time").map((t) => t.text());
    expect(times).toEqual(["14:30:21", "14:30:22", "14:30:23"]);
  });

  it("shows the fallback banner only when the adapter warned", async () => {
    const warn: AgentEvent = { kind: "warning", seq: 0, text: "Running without live approvals (x). Actions that need approval are denied" };
    expect((await setup([session()], [warn])).text()).toContain("Approvals are off for this Claude version");
    document.body.innerHTML = "";
    expect((await setup([session()], [])).text()).not.toContain("Approvals are off");
  });
});

describe("approvals", () => {
  async function waiting() {
    return setup([session({ jobState: "waitingForApproval" })], [approval]);
  }

  it("announces assertively, shows the command, and Allow once answers", async () => {
    commands.answerApproval!.mockImplementation(() => ok(null));
    const w = await waiting();
    expect(w.find("[aria-live=assertive]").text()).toBe("Claude wants to run a command: pnpm add @vueuse/core");
    expect(w.find("[data-approval]").text()).toContain("pnpm add @vueuse/core");
    await w.findAll("button").find((b) => b.text() === "Allow once")!.trigger("click");
    expect(commands.answerApproval).toHaveBeenCalledWith("j1", "r1", { kind: "allow" });
  });

  it("Enter never approves; Deny with a note sends the message", async () => {
    commands.answerApproval!.mockImplementation(() => ok(null));
    const w = await waiting();
    await w.find("[data-approval]").trigger("keydown", { key: "Enter" });
    expect(commands.answerApproval).not.toHaveBeenCalled();
    await w.findAll("button").find((b) => b.text() === "Deny with a note…")!.trigger("click");
    await w.find("[data-approval] textarea").setValue("use npm");
    await w.find("[data-approval] textarea").trigger("keydown", { key: "Enter" });
    expect(commands.answerApproval).not.toHaveBeenCalled();
    await w.findAll("[data-approval] button").find((b) => b.text() === "Deny")!.trigger("click");
    expect(commands.answerApproval).toHaveBeenCalledWith("j1", "r1", { kind: "deny", message: "use npm" });
  });

  it("Alt+A focuses the waiting card", async () => {
    const w = await waiting();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "a", altKey: true }));
    expect(document.activeElement).toBe(w.find("[data-approval]").element);
  });

  it("an auto-allowed request shows a plain line, no card", async () => {
    const auto: AgentEvent = { kind: "autoAllowed", seq: 1, atMs: 0, requestId: "r1", tool: "Bash", input: { command: "pnpm i" }, description: "Install" };
    const w = await setup([session()], [auto]);
    expect(w.find("[data-approval]").exists()).toBe(false);
    expect(w.text()).toContain("Allowed automatically: Install");
  });
});

describe("auto-approve checkbox", () => {
  const box = (w: Awaited<ReturnType<typeof setup>>) => w.findAll<HTMLInputElement>("input[type=checkbox]").find((c) => c.element.closest("label")?.textContent?.includes("Auto-approve"))!;

  it("starts from the Settings default, applies to Make changes runs only", async () => {
    commands.getApprovalSettings!.mockImplementation(() => ok({ autoApprove: true, askForRisky: true }));
    commands.startRun!.mockImplementation(() => ok("j2"));
    const w = await setup([]);
    // Plan first (the default mode): off and disabled, plans always ask.
    expect(box(w).element.checked).toBe(false);
    expect(box(w).element.disabled).toBe(true);
    await w.findAll<HTMLInputElement>("input[type=radio]")[1]!.setValue(true);
    expect(box(w).element.checked).toBe(true);
    await w.find("textarea").setValue("Add dark theme");
    await w.find("textarea").trigger("keydown", { key: "Enter", ctrlKey: true });
    expect(commands.startRun).toHaveBeenCalledWith("p1", "Add dark theme", "changes", null, null, null, true, false);
    await flushPromises();
    await box(w).setValue(false);
    await w.find("textarea").setValue("Again");
    await w.find("textarea").trigger("keydown", { key: "Enter", ctrlKey: true });
    expect(commands.startRun).toHaveBeenLastCalledWith("p1", "Again", "changes", null, null, null, false, false);
  });
});

describe("run endings", () => {
  const cases: [Parameters<typeof session>[0], AgentEvent[], string][] = [
    [{ jobState: "failed" }, [completed("failed")], "Failed."],
    [{ jobState: "failed" }, [completed("timeout")], "Stopped: no activity for 10 minutes."],
    [{ jobState: "interrupted" }, [], "Interrupted when Studio closed."],
    [{ jobState: "cancelled" }, [completed("cancelled"), { kind: "fileChange", seq: 2, path: "a" }, { kind: "fileChange", seq: 3, path: "a" }, { kind: "fileChange", seq: 4, path: "b" }], "Cancelled."],
    [{ jobState: "failed" }, [completed("incompatibleCli")], "Blocked: this Claude Code version isn't supported."],
    [{ jobState: "succeeded" }, [completed("success")], "Done."],
  ];
  it.each(cases)("%j -> %s", async (over, evs, sentence) => {
    const w = await setup([session(over)], evs);
    expect(w.find(".end").text()).toContain(sentence);
  });

  it("counts distinct changed files and offers Review changes", async () => {
    const w = await setup([session({ jobState: "cancelled" })], [completed("cancelled"), { kind: "fileChange", seq: 2, path: "a" }, { kind: "fileChange", seq: 3, path: "a" }, { kind: "fileChange", seq: 4, path: "b" }]);
    expect(w.find(".end").text()).toContain("2 files changed");
    await w.findAll(".end button").find((b) => b.text() === "Review changes")!.trigger("click");
    expect(w.emitted("review-changes")).toHaveLength(1);
  });

  it("blocks with an Open AI Agents settings action and hides raw errors under Details", async () => {
    const w = await setup([session({ jobState: "failed" })], [{ kind: "error", seq: 1, errorKind: "incompatibleCli", message: "RAW-CLI-ERROR" }, completed("incompatibleCli")]);
    expect(w.find("details").attributes("open")).toBeUndefined();
    expect(w.find("details").text()).toContain("RAW-CLI-ERROR");
    await w.findAll(".end button").find((b) => b.text() === "Open AI Agents settings")!.trigger("click");
    expect(useNavStore().settingsSection).toBe("agents");
  });

  it("lists blocked actions and Allow and retry uses the rule", async () => {
    commands.allowAndRetry!.mockImplementation(() => ok("j2"));
    const denials = [{ tool: "Bash", input: { command: "pnpm add x" }, rule: "Bash(pnpm add x)" }, { tool: "Edit", input: { file_path: "a" }, rule: null }];
    const w = await setup([session()], [{ kind: "completed", seq: 9, outcome: "success", denials }]);
    expect(w.find(".blocked").text()).toContain("pnpm add x");
    const buttons = w.findAll(".blocked button");
    expect(buttons).toHaveLength(1);
    await buttons[0]!.trigger("click");
    expect(commands.allowAndRetry).toHaveBeenCalledWith("s1", "Bash(pnpm add x)");
  });
});

describe("plan card", () => {
  it("Make these changes continues the plan session", async () => {
    commands.continuePlan!.mockImplementation(() => ok("j2"));
    const w = await setup([session({ mode: "plan" })], [{ kind: "planReady", seq: 1, plan: "1. Do it" }, completed("success")]);
    expect(w.find(".plan").text()).toContain("Do it");
    await w.findAll(".plan button").find((b) => b.text() === "Make these changes")!.trigger("click");
    expect(commands.continuePlan).toHaveBeenCalledWith("s1", false);
  });
});

describe("project plan (F15)", () => {
  const T = (text: string, done = false) => ({ text, done, detail: "" });
  const plan = (tasks: ReturnType<typeof T>[]) => ({ summary: "", tasks, savedAt: null });
  const planEv: AgentEvent[] = [{ kind: "planReady", seq: 1, plan: "- [ ] A\n- [ ] B" }, completed("success")];
  const btn = (w: Awaited<ReturnType<typeof setup>>, label: string) => w.findAll("button").find((b) => b.text() === label);
  const withTasks = (n = 2) => commands.parsePlan!.mockImplementation(() => Promise.resolve(plan(Array.from({ length: n }, (_, i) => T(`t${i}`)))));
  const planSession = () => [session({ mode: "plan" })];

  it("plan without tasks: Revise and Make stay, no Use button", async () => {
    const w = await setup(planSession(), planEv);
    expect(btn(w, "Revise plan")).toBeTruthy();
    expect(btn(w, "Make these changes")).toBeTruthy();
    expect(btn(w, "Use as project plan")).toBeUndefined();
  });

  it("plan with tasks: Use sits between Revise and Make (primary last)", async () => {
    withTasks();
    const w = await setup(planSession(), planEv);
    expect(w.findAll(".plan .actions button").map((b) => b.text())).toEqual(["Revise plan", "Use as project plan", "Make these changes"]);
  });

  it("no Use button while a run is active", async () => {
    withTasks();
    const w = await setup([session({ mode: "plan", jobState: "running" })], [{ kind: "planReady", seq: 1, plan: "- [ ] A" }]);
    expect(btn(w, "Use as project plan")).toBeUndefined();
  });

  it("saves with no existing plan: toast with Open Plan, caption replaces the button", async () => {
    withTasks(9);
    commands.saveProjectPlan!.mockImplementation(() => ok(plan(Array.from({ length: 9 }, (_, i) => T(`t${i}`)))));
    const w = await setup(planSession(), planEv);
    await btn(w, "Use as project plan")!.trigger("click");
    await flushPromises();
    expect(commands.saveProjectPlan).toHaveBeenCalledWith("p1", "- [ ] A\n- [ ] B");
    const t = useToastStore().current!;
    expect(t.text).toBe("Saved as the project plan (9 tasks)");
    t.action!.run();
    expect(useNavStore().tab).toBe("plan");
    expect(w.find(".plan").text()).toContain("Saved as project plan");
    expect(btn(w, "Use as project plan")).toBeUndefined();
  });

  it("existing plan: asks first with the counts, Cancel saves nothing", async () => {
    withTasks();
    commands.projectPlan!.mockImplementation(() => ok(plan([T("x", true), T("y"), T("z")])));
    commands.saveProjectPlan!.mockImplementation(() => ok(plan([T("a"), T("b")])));
    const w = await setup(planSession(), planEv);
    await btn(w, "Use as project plan")!.trigger("click");
    expect(document.body.textContent).toContain("Replace the project plan?");
    expect(document.body.textContent).toContain("The current plan has 1 of 3 tasks done. Its ticks are replaced by this plan.");
    await btn(w, "Cancel")!.trigger("click");
    expect(commands.saveProjectPlan).not.toHaveBeenCalled();
    await btn(w, "Use as project plan")!.trigger("click");
    await btn(w, "Replace plan")!.trigger("click");
    await flushPromises();
    expect(commands.saveProjectPlan).toHaveBeenCalled();
  });

  it("a failed save toasts the message", async () => {
    withTasks();
    commands.saveProjectPlan!.mockImplementation(() => err("Wait until the current run finishes"));
    const w = await setup(planSession(), planEv);
    await btn(w, "Use as project plan")!.trigger("click");
    await flushPromises();
    expect(useToastStore().current!.text).toBe("Wait until the current run finishes");
    expect(btn(w, "Use as project plan")).toBeTruthy();
  });

  const startRun = (over = {}) => session({ prompt: taskPrompt({ text: "Draw the maze", detail: "" }), ...over });
  const withPlan = (tasks: ReturnType<typeof T>[]) => commands.projectPlan!.mockImplementation(() => ok(plan(tasks)));

  it("ordinary run: no plan line at all", async () => {
    withPlan([T("Draw the maze")]);
    const w = await setup([session()], [completed("success")]);
    expect(w.text()).not.toContain("From the plan");
    expect(btn(w, "Mark done")).toBeUndefined();
  });

  it("succeeded Start run with an open task: Mark done, then Start next", async () => {
    withPlan([T("Draw the maze"), T("Add pellets"), T("Score")]);
    commands.setPlanTask!.mockImplementation(() => ok(plan([T("Draw the maze", true), T("Add pellets"), T("Score")])));
    commands.getApprovalSettings!.mockImplementation(() => ok({ autoApprove: false }));
    commands.startRun!.mockImplementation(() => ok("j2"));
    const w = await setup([startRun()], [completed("success")]);
    expect(w.text()).toContain("From the plan: Draw the maze");
    const mark = w.get("button[aria-label='Mark Draw the maze done']");
    expect(mark.text()).toBe("Mark done");
    await mark.trigger("click");
    await flushPromises();
    expect(commands.setPlanTask).toHaveBeenCalledWith("p1", "Draw the maze", true);
    expect(w.text()).toContain("Marked done · 1 of 3 tasks done");
    await btn(w, "Start next: Add pellets")!.trigger("click");
    await flushPromises();
    expect(commands.startRun).toHaveBeenCalledWith("p1", taskPrompt({ text: "Add pellets", detail: "" }), "changes", null, null, null, false, true);
  });

  it("last task done: All N tasks done and Build release…", async () => {
    withPlan([T("Draw the maze")]);
    commands.setPlanTask!.mockImplementation(() => ok(plan([T("Draw the maze", true)])));
    const w = await setup([startRun()], [completed("success")]);
    await w.get("button[aria-label='Mark Draw the maze done']").trigger("click");
    await flushPromises();
    expect(w.text()).toContain("All 1 tasks done");
    await w.findAll("button").filter((b) => b.text() === "Build release…").pop()!.trigger("click");
    expect(useNavStore().tab).toBe("releases");
    expect(useNavStore().buildDialogProject).toBe("p1");
  });

  it("failed run: not done, no button", async () => {
    withPlan([T("Draw the maze")]);
    const w = await setup([startRun({ jobState: "failed" })], [completed("failed" as never)]);
    expect(w.text()).toContain("From the plan: Draw the maze · not done");
    expect(btn(w, "Mark done")).toBeUndefined();
  });

  it("succeeded but task already done: shows done, no button", async () => {
    withPlan([T("Draw the maze", true)]);
    const w = await setup([startRun()], [completed("success")]);
    expect(w.text()).toContain("From the plan: Draw the maze · done");
    expect(btn(w, "Mark done")).toBeUndefined();
  });
});

describe("Cancel", () => {
  it("replaces Run while a run is active and cancels without a dialog", async () => {
    commands.cancelRun!.mockImplementation(() => ok(null));
    const w = await setup([session({ jobState: "running" })]);
    expect(w.find("textarea").attributes("disabled")).toBeDefined();
    expect(w.findAll("button").some((b) => b.text() === "Run")).toBe(false);
    await w.findAll("button").find((b) => b.text() === "Cancel")!.trigger("click");
    expect(commands.cancelRun).toHaveBeenCalledWith("j1");
  });
});

describe("markdown (D60)", () => {
  it("neutralizes script, img onerror and javascript: links", () => {
    const html = renderMarkdown('<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>\n\n[bad](javascript:alert(1)) ![pic](http://x/y.png) [ok](https://example.com)');
    const dom = document.createElement("div");
    dom.innerHTML = html;
    expect(dom.querySelectorAll("script, img").length).toBe(0);
    expect([...dom.querySelectorAll("*")].some((el) => el.getAttributeNames().some((a) => a.startsWith("on")))).toBe(false);
    expect([...dom.querySelectorAll("a")].map((a) => a.dataset.url)).toEqual(["https://example.com"]);
    expect(dom.textContent).toContain("<img src=x onerror=alert(1)>"); // shown as text, not run
    expect(html).toContain('data-url="https://example.com"');
    expect(html).toContain("(https://example.com)");
  });
});

describe("desktop notification (D54)", () => {
  async function fire(focused: boolean) {
    notif.isPermissionGranted.mockResolvedValue(true);
    vi.spyOn(document, "hasFocus").mockReturnValue(focused);
    commands.listProjects!.mockImplementation(() => ok([project()]));
    const pinia = createPinia();
    setActivePinia(pinia);
    commands.listSessions!.mockImplementation(() => ok([session({ jobState: "running" })]));
    const runs = useRunsStore();
    await runs.loadSessions("p1");
    await runs.listen();
    installApprovalNotifications();
    await bus.emit("j1", approval);
    await flushPromises();
  }
  it("sends one notification when the window is unfocused", async () => {
    await fire(false);
    expect(notif.sendNotification).toHaveBeenCalledTimes(1);
    expect(notif.sendNotification.mock.calls[0]![0].body).toBe("A project: Claude wants to run a command");
  });
  it("stays quiet when focused", async () => {
    await fire(true);
    expect(notif.sendNotification).not.toHaveBeenCalled();
  });
});
