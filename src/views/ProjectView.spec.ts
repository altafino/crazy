import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands, ev } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>>, ev: { current: null as unknown } }));
vi.mock("../bindings", () => ({ commands, get events() { return (ev.current as { events: unknown }).events; } }));

import { mockCommands, mockEvents, ok, project, session } from "../test-helpers";
import { useNavStore } from "../stores/nav";
import ProjectView from "./ProjectView.vue";

async function setup(sessions = [session()]) {
  commands.listProjects!.mockImplementation(() => ok([project()]));
  commands.listSessions!.mockImplementation(() => ok(sessions));
  const pinia = createPinia();
  setActivePinia(pinia);
  useNavStore().openProject("p1");
  const w = mount(ProjectView, { global: { plugins: [pinia], stubs: { ChatTab: true, ChangesTab: true } }, attachTo: document.body });
  await flushPromises();
  return w;
}

beforeEach(() => {
  document.body.innerHTML = "";
  Object.assign(commands, mockCommands());
  ev.current = mockEvents();
});

describe("ProjectView", () => {
  it("shows the breadcrumb and returns to Projects from it and with Esc", async () => {
    const w = await setup();
    expect(w.find("nav[aria-label='Breadcrumb']").text()).toContain("Notes");
    await w.find(".crumbs button").trigger("click");
    expect(useNavStore().view).toBe("projects");
    useNavStore().openProject("p1");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(useNavStore().view).toBe("projects");
  });

  it("offers a run picker with the newest run selected", async () => {
    const w = await setup([session({ id: "s2", mode: "plan", jobState: "running" }), session()]);
    const select = w.find("select").element as HTMLSelectElement;
    expect(select.value).toBe("s2");
    expect(select.options[0]!.text).toMatch(/Plan first · Running$/);
  });

  it("marks the Chat tab when a run waits for approval", async () => {
    const w = await setup([session({ jobState: "waitingForApproval" })]);
    expect(w.find("#tab-chat").text()).toContain("Needs approval");
  });
});
