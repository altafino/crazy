import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import type { PreviewStatus } from "../bindings";

const { commands, ev } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>>, ev: { current: null as unknown } }));
vi.mock("../bindings", () => ({ commands, get events() { return (ev.current as { events: unknown }).events; } }));

import { mockCommands, mockEvents, ok, project } from "../test-helpers";
import ProjectCard from "../components/ProjectCard.vue";
import { useNavStore } from "../stores/nav";
import { usePreviewStore } from "../stores/preview";
import ProjectView from "./ProjectView.vue";

const status = (phase: PreviewStatus["phase"], live = true): PreviewStatus => ({
  jobId: "j1", projectId: "p1", state: "running", phase, port: null, startedAt: null, completedAt: null, errorMessage: null, live, hasLog: true,
});

beforeEach(() => {
  document.body.innerHTML = "";
  Object.assign(commands, mockCommands());
  ev.current = mockEvents();
});

describe("Preview indicators (M3-D48)", () => {
  it("the tab dot follows the phase and carries a text name", async () => {
    commands.listProjects!.mockImplementation(() => ok([project()]));
    commands.listSessions!.mockImplementation(() => ok([]));
    const pinia = createPinia();
    setActivePinia(pinia);
    useNavStore().openProject("p1");
    const w = mount(ProjectView, { global: { plugins: [pinia], stubs: { ChatTab: true } }, attachTo: document.body });
    await flushPromises();
    const tab = () => w.find("#tab-preview");
    expect(tab().find(".dot").exists()).toBe(false);
    const p = usePreviewStore();
    for (const [phase, cls, name] of [
      [{ phase: "compiling" }, "accent", "Preview, starting"],
      [{ phase: "appRunning" }, "ok", "Preview, running"],
      [{ phase: "buildFailed", firstError: "e" }, "danger", "Preview, build failed"],
    ] as const) {
      p.status.p1 = status(phase);
      await flushPromises();
      expect(tab().find(".dot").classes()).toContain(cls === "accent" ? "dot" : cls);
      expect(tab().attributes("aria-label")).toBe(name);
    }
    p.status.p1 = status(null, false);
    await flushPromises();
    expect(tab().find(".dot").exists()).toBe(false);
  });

  it("the card badge says Previewing or Preview: build failed, after Needs approval", async () => {
    const pinia = createPinia();
    setActivePinia(pinia);
    const p = usePreviewStore();
    const w = mount(ProjectCard, { props: { project: project(), waiting: true }, global: { plugins: [pinia] } });
    expect(w.text()).not.toContain("Preview");
    p.status.p1 = status({ phase: "appRunning" });
    await flushPromises();
    const badges = w.findAll(".badge").map((b) => b.text());
    expect(badges).toEqual(["Needs approval", "Previewing"]);
    p.status.p1 = status({ phase: "buildFailed", firstError: "e" });
    await flushPromises();
    expect(w.findAll(".badge").map((b) => b.text())).toEqual(["Needs approval", "Preview: build failed"]);
  });
});
