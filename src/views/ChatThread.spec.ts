import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import type { AgentEvent, SessionSummary } from "../bindings";

const { commands, ev } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>>, ev: { current: null as unknown } }));
vi.mock("../bindings", () => ({ commands, get events() { return (ev.current as { events: unknown }).events; } }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(), revealItemInDir: vi.fn() }));
vi.mock("@tauri-apps/plugin-notification", () => ({ isPermissionGranted: vi.fn(), requestPermission: vi.fn(), sendNotification: vi.fn() }));

import { err, mockCommands, mockEvents, ok, project, session } from "../test-helpers";
import { useRunsStore } from "../stores/runs";
import ChatTab from "./ChatTab.vue";

const profile = { id: "pr1", name: "Claude", provider: "claude", executable: "claude", env: [], model: null, inactivitySecs: 600, instructions: "", isDefault: true, enabled: true };
const done: AgentEvent = { kind: "completed", seq: 9, outcome: "success", denials: [] };
const edit: AgentEvent = { kind: "fileChange", seq: 2, path: "a.ts" };
let bus: ReturnType<typeof mockEvents>;

/** Observers the runs created: a test fires one to bring that run near the screen. */
const observers: { cb: (e: { isIntersecting: boolean }[]) => void; el: Element; disconnected: boolean }[] = [];
class FakeObserver {
  private entry: (typeof observers)[number] | undefined;
  constructor(private cb: (e: { isIntersecting: boolean }[]) => void) {}
  observe(el: Element) {
    this.entry = { cb: this.cb, el, disconnected: false };
    observers.push(this.entry);
  }
  disconnect() {
    if (this.entry) this.entry.disconnected = true;
  }
}
const intersect = (runId: string) => observers.find((o) => o.el.getAttribute("aria-labelledby") === `run-${runId}-name`)!.cb([{ isIntersecting: true }]);

/** `n` runs, newest first: s1 is the newest, one per hour on 2026-11-02 (older ones on earlier days past 5). */
const runs = (n: number, over: (i: number) => Partial<SessionSummary> = () => ({})): SessionSummary[] =>
  Array.from({ length: n }, (_, i) => {
    const at = new Date(2026, 10, 2 - Math.floor(i / 6), 14 - (i % 6)).toISOString();
    return session({ id: `s${i + 1}`, jobId: `j${i + 1}`, prompt: `Run number ${i + 1}`, createdAt: at, startedAt: at, ...over(i) });
  });

async function setup(list: SessionSummary[], evsFor: (id: string) => AgentEvent[] = () => [done]) {
  commands.listSessions!.mockImplementation(() => ok(list));
  commands.getSessionEvents!.mockImplementation((id: string) => ok(evsFor(id)));
  commands.listAgentProfiles!.mockImplementation(() => ok([profile]));
  const pinia = createPinia();
  setActivePinia(pinia);
  await useRunsStore().loadSessions("p1");
  const w = mount(ChatTab, { props: { project: project() }, global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return w;
}
const loadedIds = () => commands.getSessionEvents!.mock.calls.map((c) => c[0]);

beforeEach(() => {
  document.body.innerHTML = "";
  localStorage.clear();
  observers.length = 0;
  Object.assign(commands, mockCommands());
  bus = mockEvents();
  ev.current = bus;
  vi.stubGlobal("IntersectionObserver", FakeObserver);
});
afterEach(() => vi.unstubAllGlobals());

describe("thread paging and lazy events (D2, R5)", () => {
  it("shows the last 20 runs, oldest first, and loads only the newest run's events", async () => {
    const w = await setup(runs(25));
    const titles = w.findAll("article h3.prompt").map((h) => h.text());
    expect(titles).toHaveLength(20);
    expect(titles[0]).toBe("Run number 20");
    expect(titles[19]).toBe("Run number 1");
    expect(loadedIds()).toEqual(["s1"]);
    // Older runs show their prompt and a one-line placeholder, no end row yet.
    const old = w.findAll("article")[0]!;
    expect(old.text()).toMatch(/Succeeded · .*\d:\d\d/);
    expect(old.find(".end").exists()).toBe(false);
  });

  it("loads an older run once when it comes into range", async () => {
    const w = await setup(runs(5));
    intersect("s3");
    await flushPromises();
    intersect("s3");
    await flushPromises();
    expect(loadedIds()).toEqual(["s1", "s3"]);
    expect(useRunsStore().eventsBySession.s3).toBeTruthy();
    const art = w.findAll("article").find((a) => a.text().includes("Run number 3"))!;
    expect(art.find(".end").exists()).toBe(true);
    expect(art.text()).not.toMatch(/Succeeded · /);
  });

  it("Show earlier runs adds the next page without duplicates and keeps the reading position", async () => {
    const w = await setup(runs(25));
    const scroll = w.get(".scroll").element as HTMLElement;
    let height = 2000;
    Object.defineProperty(scroll, "scrollHeight", { get: () => height, configurable: true });
    scroll.scrollTop = 0;
    const more = w.findAll("button").find((b) => b.text() === "Show earlier runs")!;
    // Five older runs are about to add 500px on top.
    const done5 = more.trigger("click");
    height = 2500;
    await done5;
    await flushPromises();
    expect(w.findAll("article")).toHaveLength(25);
    expect(new Set(w.findAll("article h3.prompt").map((h) => h.text())).size).toBe(25);
    expect(w.findAll("button").some((b) => b.text() === "Show earlier runs")).toBe(false);
    expect(scroll.scrollTop).toBe(height - 2000); // the run that was at the top stays where it was
  });

  it("a run whose log can't be read says so with Retry; the others render", async () => {
    commands.getSessionEvents!.mockImplementation((id: string) => (id === "s2" ? err("boom") : ok([done])));
    const w = await setup(runs(3));
    commands.getSessionEvents!.mockImplementation((id: string) => (id === "s2" ? err("boom") : ok([done])));
    intersect("s2");
    intersect("s3");
    await flushPromises();
    const art = (n: number) => w.findAll("article").find((a) => a.text().includes(`Run number ${n}`))!;
    expect(art(2).text()).toContain("Couldn't load this run");
    expect(art(3).find(".end").exists()).toBe(true);
    expect(art(1).find(".end").exists()).toBe(true);
    expect(w.text()).not.toContain("boom");
    commands.getSessionEvents!.mockImplementation(() => ok([done]));
    await art(2).findAll("button").find((b) => b.text() === "Retry")!.trigger("click");
    await flushPromises();
    expect(art(2).text()).not.toContain("Couldn't load this run");
    expect(art(2).find(".end").exists()).toBe(true);
  });

  it("opens at the newest run and puts a divider with the day before each new day", async () => {
    const w = await setup(runs(8));
    const seps = w.findAll("[role=separator]");
    expect(seps).toHaveLength(2); // two days
    expect(seps.every((s) => !!s.attributes("aria-label"))).toBe(true);
    expect(w.find("[role=log]").exists()).toBe(true);
  });
});

describe("runs are articles with a roving tabindex (D13)", () => {
  it("names each article by its prompt and state, with the prompt as an h3", async () => {
    const w = await setup(runs(2, (i) => ({ prompt: `Add a high-score table\nsecond line ${i}`, jobState: i === 0 ? "failed" : "succeeded" })));
    const arts = w.findAll("article");
    const name = (a: (typeof arts)[number]) => document.getElementById(a.attributes("aria-labelledby")!)!.textContent;
    expect(name(arts[0]!)).toBe("Add a high-score table, succeeded");
    expect(name(arts[1]!)).toBe("Add a high-score table, failed");
    expect(arts[1]!.find("h3").text()).toContain("second line");
  });

  it("only the newest run's controls are tabbable until focus enters an older run", async () => {
    const w = await setup(runs(3), () => [edit, done]);
    intersect("s2");
    intersect("s3");
    await flushPromises();
    const [a3, a2, a1] = w.findAll("article");
    const tabs = (a: typeof a1) => a!.findAll("button").map((b) => b.attributes("tabindex"));
    expect(tabs(a1).every((t) => t === undefined)).toBe(true);
    expect(tabs(a2).length).toBeGreaterThan(0);
    expect(tabs(a2).every((t) => t === "-1")).toBe(true);
    await a2!.find("button").trigger("focusin");
    expect(tabs(a2).every((t) => t === undefined)).toBe(true);
    expect(tabs(a3).every((t) => t === "-1")).toBe(true);
    await a3!.find("button").trigger("focusin");
    expect(tabs(a2).every((t) => t === "-1")).toBe(true);
    expect(tabs(a3).every((t) => t === undefined)).toBe(true);
    expect(tabs(a1).every((t) => t === undefined)).toBe(true);
  });
});

describe("regression contract with many runs (R6)", () => {
  it("Alt+A focuses the waiting approval, and the assertive announcement is unchanged", async () => {
    const approval: AgentEvent = { kind: "approvalNeeded", seq: 1, requestId: "r1", tool: "Bash", input: { command: "pnpm add x" }, description: "Install" };
    const w = await setup(runs(12, (i) => (i === 0 ? { jobState: "waitingForApproval" } : {})), (id) => (id === "s1" ? [approval] : [done]));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "a", altKey: true }));
    expect(document.activeElement).toBe(w.get("[data-approval]").element);
    expect(w.get("[aria-live=assertive]").text()).toBe("Claude wants to run a command: pnpm add x");
  });

  it("each run's end row acts on its own run", async () => {
    commands.runDiff!.mockImplementation(() => ok([{ path: "a.ts" }]));
    commands.allowAndRetry!.mockImplementation(() => ok("j9"));
    const denial = (n: number) => ({ kind: "completed", seq: 9, outcome: "success", denials: [{ tool: "Bash", input: { command: `cmd ${n}` }, rule: `Bash(cmd ${n})` }] }) as AgentEvent;
    const w = await setup(runs(2, (i) => ({ prompt: `Run number ${i + 1}` })), (id) => [edit, denial(id === "s1" ? 1 : 2)]);
    intersect("s2");
    await flushPromises();
    const old = w.findAll("article")[0]!;
    expect(old.text()).toContain("Run number 2");
    await old.findAll("button").find((b) => b.text() === "Commit…")!.trigger("click");
    await flushPromises();
    expect(commands.runDiff).toHaveBeenCalledWith("s2");
    await old.findAll("button").find((b) => b.text() === "Allow and retry")!.trigger("click");
    expect(commands.allowAndRetry).toHaveBeenCalledWith("s2", "Bash(cmd 2)");
    await old.findAll("button").find((b) => b.text() === "Review changes")!.trigger("click");
    expect(w.emitted("review-changes")).toHaveLength(1);
    const newest = w.findAll("article")[1]!;
    await newest.findAll("button").find((b) => b.text() === "Allow and retry")!.trigger("click");
    expect(commands.allowAndRetry).toHaveBeenLastCalledWith("s1", "Bash(cmd 1)");
  });

  it("Make these changes resumes the plan session of its own run", async () => {
    commands.continuePlan!.mockImplementation(() => ok("j9"));
    const plan: AgentEvent = { kind: "planReady", seq: 1, plan: "1. Do it" };
    const w = await setup(runs(2, (i) => ({ mode: i === 0 ? "plan" : "changes" })), (id) => (id === "s1" ? [plan, done] : [done]));
    await w.findAll(".plan button").find((b) => b.text() === "Make these changes")!.trigger("click");
    expect(commands.continuePlan).toHaveBeenCalledWith("s1", false);
  });

  it("a Plan card's actions hide once a later run resumes from it", async () => {
    const plan: AgentEvent = { kind: "planReady", seq: 1, plan: "1. Do it" };
    const list = runs(2, (i) => (i === 0 ? { resumedFrom: "s2" } : { mode: "plan" }));
    const w = await setup(list, (id) => (id === "s2" ? [plan, done] : [done]));
    intersect("s2");
    await flushPromises();
    expect(w.find(".plan").exists()).toBe(true);
    expect(w.find(".plan .actions").exists()).toBe(false);
  });
});

describe("Jump to latest (D5)", () => {
  async function scrolledUp() {
    const w = await setup(runs(3, (i) => (i === 0 ? { jobState: "running" } : {})), (id) => (id === "s1" ? [{ kind: "text", seq: 0, text: "hello" }] : [done]));
    const scroll = w.get(".scroll").element as HTMLElement;
    Object.defineProperty(scroll, "scrollHeight", { value: 2000, configurable: true });
    Object.defineProperty(scroll, "clientHeight", { value: 500, configurable: true });
    const pill = () => w.findAll("button").find((b) => b.text().endsWith("Jump to latest"));
    const scrollTo = async (top: number) => {
      scroll.scrollTop = top;
      await w.get(".scroll").trigger("scroll");
    };
    return { w, scroll, pill, scrollTo };
  }

  it("is hidden at the bottom, appears only when scrolled up and a row arrives, and never moves the scroll itself", async () => {
    const { scroll, pill, scrollTo } = await scrolledUp();
    await scrollTo(1500);
    await bus.emit("j1", { kind: "text", seq: 1, text: "near the bottom" });
    await flushPromises();
    expect(pill()).toBeUndefined();
    await scrollTo(300);
    expect(pill()).toBeUndefined(); // scrolled up, but nothing new yet
    await bus.emit("j1", { kind: "text", seq: 2, text: "new row" });
    await flushPromises();
    expect(pill()!.text()).toBe("New activity · Jump to latest");
    expect(pill()!.classes()).not.toContain("warn");
    expect(scroll.scrollTop).toBe(300);
  });

  it("uses the amber variant for an approval; a click scrolls to the bottom and hides it", async () => {
    const { scroll, pill, scrollTo } = await scrolledUp();
    await scrollTo(300);
    await bus.emit("j1", { kind: "approvalNeeded", seq: 3, requestId: "r1", tool: "Bash", input: { command: "ls" }, description: "d" });
    await flushPromises();
    expect(pill()!.text()).toBe("Waiting for your approval · Jump to latest");
    expect(pill()!.classes()).toContain("warn");
    await pill()!.trigger("click");
    expect(scroll.scrollTop).toBe(2000);
    await flushPromises();
    expect(pill()).toBeUndefined();
  });

  it("hides again when the user scrolls back to the bottom", async () => {
    const { pill, scrollTo } = await scrolledUp();
    await scrollTo(300);
    await bus.emit("j1", { kind: "text", seq: 4, text: "x" });
    await flushPromises();
    expect(pill()).toBeTruthy();
    await scrollTo(1450);
    expect(pill()).toBeUndefined();
  });
});
