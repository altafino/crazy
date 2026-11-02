import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import type { PreviewStatus } from "../bindings";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../bindings", () => ({
  commands,
  events: { previewStatusEvent: { listen: vi.fn(async () => () => {}) }, previewLogEvent: { listen: vi.fn(async () => () => {}) } },
}));

import { ok, project } from "../test-helpers";
import { useNavStore } from "../stores/nav";
import PreviewTab from "./PreviewTab.vue";

const status = (over: Partial<PreviewStatus> = {}): PreviewStatus => ({
  jobId: "j1", projectId: "p1", state: "running", phase: { phase: "compiling" }, port: 1420,
  startedAt: "2026-11-02T14:32:00Z", completedAt: null, errorMessage: null, live: true, hasLog: true, ...over,
});

async function setup(st: PreviewStatus | null, lines: string[] = [], p = project()) {
  commands.getPreviewStatus!.mockImplementation(() => ok(st));
  commands.getPreviewLog!.mockImplementation(() => ok(st ? { jobId: st.jobId, lines, nextSeq: lines.length, logTruncated: false } : null));
  const pinia = createPinia();
  setActivePinia(pinia);
  const w = mount(PreviewTab, { props: { project: p }, global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return w;
}

beforeEach(() => {
  document.body.innerHTML = "";
  Object.assign(commands, {
    getPreviewStatus: vi.fn(), getPreviewLog: vi.fn(), getPreviewSettings: vi.fn(() => ok({ maxLines: 5000, maxLogMb: 10, keepLogs: 5 })),
    startPreview: vi.fn(), stopPreview: vi.fn(() => ok(null)), restartPreview: vi.fn(),
  });
});

describe("PreviewTab", () => {
  it("shows the start screen with the first-build expectation and starts the preview", async () => {
    const w = await setup(null);
    expect(w.text()).toContain("Run Notes as a separate window");
    expect(w.text()).toContain("first build");
    commands.startPreview!.mockImplementation(() => ok(status({ phase: { phase: "starting" } })));
    await w.findAll("button").find((b) => b.text() === "Start preview")!.trigger("click");
    await flushPromises();
    expect(commands.startPreview).toHaveBeenCalledWith("p1");
    expect(w.text()).toContain("Stop");
    expect(w.find("ol.stepper").text()).toContain("Installing");
  });

  it("explains why imported folders have no preview (M3-D39)", async () => {
    const w = await setup(null, [], project({ supportsOperations: false, templateId: "existing" }));
    expect(w.text()).toContain("Preview isn't available");
    expect(w.text()).not.toContain("Start preview");
  });

  it("shows the running phase, port and the install-skipped note", async () => {
    const w = await setup(status(), ["Dependencies not refreshed while the agent is working; restart"]);
    expect(w.text()).toContain("Port 1420");
    expect(w.text()).toContain("Restart");
    expect(w.text()).toContain("Dependencies weren't refreshed");
    expect(w.find(".step.skipped").exists()).toBe(true);
  });

  it("build failed: danger callout with the first error; Ask Claude to fix hands off error and location (M3-D43)", async () => {
    const err = "error[E0425]: cannot find value `x` in this scope";
    const w = await setup(status({ phase: { phase: "buildFailed", firstError: err } }), ["Compiling app", err, " --> src/main.rs:5:3"]);
    const callout = w.find(".callout.danger");
    expect(callout.text()).toContain(err);
    const nav = useNavStore();
    await callout.find("button").trigger("click");
    expect(nav.composerHandoff).toEqual({ projectId: "p1", text: expect.stringContaining(err) });
    expect(nav.composerHandoff!.text).toContain("Location: src/main.rs:5:3");
    expect(nav.tab).toBe("chat");
  });

  it("keeps the log and outcome after it ends this session, with Start again (M3-D40)", async () => {
    const w = await setup(status(), ["boom"]);
    expect(w.text()).toContain("Stop");
    commands.stopPreview!.mockImplementation(() => ok(null));
    // The status event arrives; emulate through the store.
    const { usePreviewStore } = await import("../stores/preview");
    usePreviewStore().status.p1 = status({ live: false, state: "failed", phase: null, errorMessage: "App crashed" });
    await flushPromises();
    expect(w.text()).toContain("App crashed");
    expect(w.text()).toContain("boom");
    expect(w.text()).toContain("Start again");
  });

  it("after an app restart shows the start screen with 'Last preview … · View log' (M3-D41)", async () => {
    const w = await setup(status({ live: false, state: "succeeded", phase: null, completedAt: "2026-11-02T15:00:00Z" }), ["old line"]);
    expect(w.text()).toContain("Start preview");
    expect(w.text()).toContain("Last preview");
    expect(w.find("[role=log]").exists()).toBe(false);
    await w.find("button.link").trigger("click");
    expect(w.text()).toContain("old line");
  });

  it("shows the backend's preflight message as returned", async () => {
    const w = await setup(null);
    commands.startPreview!.mockImplementation(() => Promise.resolve({ status: "error", error: { message: "node isn't installed. Run the Environment Doctor.", fieldErrors: [] } }));
    await w.findAll("button").find((b) => b.text() === "Start preview")!.trigger("click");
    await flushPromises();
    expect(w.find("[role=alert]").text()).toBe("node isn't installed. Run the Environment Doctor.");
  });

  it("Copy puts every kept line on the clipboard and Ctrl+F focuses the log's find field", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.assign(navigator, { clipboard: { writeText } });
    const w = await setup(status(), ["one", "two"]);
    await w.find("button[aria-label='Copy log']").trigger("click");
    expect(writeText).toHaveBeenCalledWith("one\ntwo");
    await w.find("input[type=search]").trigger("keydown", { key: "f", ctrlKey: true });
    expect(document.activeElement).toBe(w.find("input[type=search]").element);
  });
});
