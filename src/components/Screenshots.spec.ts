import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands, events } = vi.hoisted(() => ({
  commands: {} as Record<string, ReturnType<typeof vi.fn>>,
  events: { screenshotsChangedEvent: { listen: vi.fn(() => Promise.resolve(() => {})) } },
}));
vi.mock("../bindings", () => ({ commands, events }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(), revealItemInDir: vi.fn() }));

import { mockCommands, ok, project } from "../test-helpers";
import ProjectCard from "./ProjectCard.vue";

const shot = (n: number) => ({ path: `/d/screenshots/p1/${n}.png`, src: `data:image/png;base64,${n}` });

beforeEach(() => {
  document.body.innerHTML = "";
  setActivePinia(createPinia());
  Object.assign(commands, mockCommands(), {
    listInstalledApps: vi.fn(() => ok([])), getBuildStatus: vi.fn(() => ok(null)),
    projectScreenshots: vi.fn(() => ok({ latest: [shot(1), shot(2), shot(3)], installed: [] })),
  });
});

describe("screenshots on the project card", () => {
  it("shows the newest shots; one opens large and leaves the project closed", async () => {
    const w = mount(ProjectCard, { props: { project: project({ id: "p1", name: "Notes" }) }, attachTo: document.body });
    await flushPromises();
    const thumbs = w.findAll(".thumb img");
    expect(thumbs.map((t) => t.attributes("src"))).toEqual([1, 2, 3].map((n) => `data:image/png;base64,${n}`));
    await w.findAll(".thumb")[1]!.trigger("click");
    expect(w.find(".large").attributes("src")).toBe("data:image/png;base64,2");
    await w.find(".large").trigger("click");
    expect(w.emitted("enter")).toBeUndefined();
    const next = w.findAll(".viewer-foot button").find((b) => b.text() === "Next")!;
    await next.trigger("click");
    expect(w.find(".large").attributes("src")).toBe("data:image/png;base64,3");
  });

  it("shows nothing without shots", async () => {
    commands.projectScreenshots = vi.fn(() => ok({ latest: [], installed: [] }));
    const w = mount(ProjectCard, { props: { project: project({ id: "p2" }) } });
    await flushPromises();
    expect(w.find(".shots").exists()).toBe(false);
  });
});
