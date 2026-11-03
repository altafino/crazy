import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import type { AgentEvent } from "../bindings";

const { commands, ev } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>>, ev: { current: null as unknown } }));
vi.mock("../bindings", () => ({ commands, get events() { return (ev.current as { events: unknown }).events; } }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(), revealItemInDir: vi.fn() }));
vi.mock("@tauri-apps/plugin-notification", () => ({ isPermissionGranted: vi.fn(), requestPermission: vi.fn(), sendNotification: vi.fn() }));

import { mockCommands, mockEvents, ok, project, session } from "../test-helpers";
import { useNavStore } from "../stores/nav";
import { useRunsStore } from "../stores/runs";
import ChatTab from "./ChatTab.vue";

const profile = { id: "pr1", name: "Claude", provider: "claude", executable: "claude", env: [], model: null, inactivitySecs: 600, instructions: "", isDefault: true, enabled: true };
const edit: AgentEvent = { kind: "fileChange", seq: 2, path: "a.ts" };
const done: AgentEvent = { kind: "completed", seq: 9, outcome: "success", denials: [] };

async function setup(sessions = [session()], evs: AgentEvent[] = [edit, done]) {
  commands.listSessions!.mockImplementation(() => ok(sessions));
  commands.getSessionEvents!.mockImplementation(() => ok(evs));
  commands.listAgentProfiles!.mockImplementation(() => ok([profile]));
  const pinia = createPinia();
  setActivePinia(pinia);
  await useRunsStore().loadSessions("p1");
  const w = mount(ChatTab, { props: { project: project() }, global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return { w, nav: useNavStore() };
}
const link = (w: ReturnType<typeof mount>, label: string) => w.findAll(".end button").find((b) => b.text() === label);

beforeEach(() => {
  document.body.innerHTML = "";
  localStorage.clear();
  Object.assign(commands, mockCommands());
  ev.current = mockEvents();
});

describe("end row (M3-D42)", () => {
  it("a successful Make-changes run keeps Review changes and adds Preview and Commit…", async () => {
    const { w, nav } = await setup();
    expect(link(w, "Review changes")).toBeTruthy();
    await link(w, "Preview")!.trigger("click");
    expect(nav.tab).toBe("preview");
  });

  it("Commit… hands the run's files and a drafted message to Source", async () => {
    commands.runDiff!.mockImplementation(() => ok([{ path: "a.ts" }, { path: "b.ts" }, { path: "big.bin", skippedReason: "tooLarge" }]));
    const long = `${"Add a dark theme ".repeat(8)}\nsecond line`;
    const { w, nav } = await setup([session({ prompt: long })]);
    await link(w, "Commit…")!.trigger("click");
    await flushPromises();
    expect(commands.runDiff).toHaveBeenCalledWith("s1");
    expect(nav.commitHandoff!.paths).toEqual(["a.ts", "b.ts"]);
    expect(nav.commitHandoff!.projectId).toBe("p1");
    expect(nav.commitHandoff!.message.length).toBeLessThanOrEqual(72);
    expect(nav.commitHandoff!.message.startsWith("Add a dark theme")).toBe(true);
    expect(nav.tab).toBe("source");
  });

  it("no Preview or Commit… for plan runs or failed runs", async () => {
    const plan = await setup([session({ mode: "plan" })]);
    expect(link(plan.w, "Preview")).toBeUndefined();
    document.body.innerHTML = "";
    const failed = await setup([session({ jobState: "failed" })], [edit, { kind: "completed", seq: 9, outcome: "failed", denials: [] }]);
    expect(link(failed.w, "Commit…")).toBeUndefined();
  });

  it("shows the checkpoint ref as a copyable mono line in the details (M3-D25)", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.assign(navigator, { clipboard: { writeText } });
    const { w } = await setup([session({ checkpointRef: "refs/studio/checkpoints/abc" })]);
    expect(w.find(".end .ref code.mono").text()).toBe("refs/studio/checkpoints/abc");
    await w.find(".end .ref button").trigger("click");
    expect(writeText).toHaveBeenCalledWith("refs/studio/checkpoints/abc");
  });
});

describe("warning details", () => {
  it("expands the detail behind a warning row", async () => {
    const { w } = await setup([session()], [{ kind: "warning", seq: 1, text: "No Git checkpoint for this run", details: "fatal: not a git repository" }, done]);
    expect(w.text()).toContain("No Git checkpoint for this run");
    expect(w.text()).not.toContain("fatal: not a git repository");
    await w.findAll(".timeline button").find((b) => b.text() === "Details")!.trigger("click");
    expect(w.text()).toContain("fatal: not a git repository");
  });
});

describe("composer handoff (M3-D43)", () => {
  it("fills the composer in Make changes mode, focuses it, clears the handoff and runs nothing", async () => {
    const { w, nav } = await setup([]);
    nav.askClaude({ projectId: "p1", text: "The preview build failed with this error" });
    await flushPromises();
    const area = w.find("textarea");
    expect((area.element as HTMLTextAreaElement).value).toBe("The preview build failed with this error");
    expect(w.findAll<HTMLInputElement>("input[type=radio]")[1]!.element.checked).toBe(true);
    expect(document.activeElement).toBe(area.element);
    expect(nav.composerHandoff).toBeNull();
    expect(commands.startRun).not.toHaveBeenCalled();
  });

  it("applies a handoff that was set before the tab mounted, and ignores other projects", async () => {
    const pinia = createPinia();
    setActivePinia(pinia);
    commands.listSessions!.mockImplementation(() => ok([]));
    commands.listAgentProfiles!.mockImplementation(() => ok([profile]));
    const nav = useNavStore();
    nav.askClaude({ projectId: "other", text: "not for you" });
    const w = mount(ChatTab, { props: { project: project() }, global: { plugins: [pinia] }, attachTo: document.body });
    await flushPromises();
    expect((w.find("textarea").element as HTMLTextAreaElement).value).toBe("");
    expect(nav.composerHandoff).not.toBeNull();
    nav.askClaude({ projectId: "p1", text: "mine" });
    await flushPromises();
    expect((w.find("textarea").element as HTMLTextAreaElement).value).toBe("mine");
  });
});
