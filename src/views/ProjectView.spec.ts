import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands, ev } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>>, ev: { current: null as unknown } }));
vi.mock("../bindings", () => ({ commands, get events() { return (ev.current as { events: unknown }).events; } }));
const openDialog = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: openDialog }));

import { mockCommands, mockEvents, ok, project, session } from "../test-helpers";
import { useNavStore } from "../stores/nav";
import { useToastStore } from "../stores/toast";
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
  it("shows the app icon in the header and changes it from a picked image", async () => {
    commands.projectIcon!.mockImplementation(() => ok(null));
    const w = await setup();
    const btn = w.get("button[aria-label='Change app icon of Notes']");
    expect(btn.find("img").exists()).toBe(false); // letter placeholder until there is an icon
    openDialog.mockResolvedValueOnce("/pics/logo.png");
    commands.setProjectIcon!.mockImplementation(() => ok("data:image/png;base64,AAA"));
    await btn.trigger("click");
    await flushPromises();
    expect(commands.setProjectIcon).toHaveBeenCalledWith("p1", "/pics/logo.png");
    expect(btn.get("img").attributes("src")).toBe("data:image/png;base64,AAA");
    expect(useToastStore().current?.text).toBe("App icon changed. Preview and the next release use it.");
  });

  it("cancelling the picker changes nothing; a refused image is shown as a toast", async () => {
    const w = await setup();
    const btn = w.get("button[aria-label='Change app icon of Notes']");
    openDialog.mockResolvedValueOnce(null);
    await btn.trigger("click");
    await flushPromises();
    expect(commands.setProjectIcon).not.toHaveBeenCalled();
    openDialog.mockResolvedValueOnce("/pics/tiny.png");
    commands.setProjectIcon!.mockImplementation(() =>
      Promise.resolve({ status: "error", error: { message: "This image is 64 × 64 pixels. Pick one at least 256 × 256." } }),
    );
    await btn.trigger("click");
    await flushPromises();
    expect(useToastStore().current?.text).toBe("This image is 64 × 64 pixels. Pick one at least 256 × 256.");
  });

  it("orders the tabs Chat, Plan, Changes, Source, Preview, Releases, GitHub, Database", async () => {
    const w = await setup();
    const tabs = w.findAll("[role='tab']").map((t) => t.text());
    expect(tabs).toEqual(["Chat", "Plan", "Changes", "Source", "Preview", "Releases", "GitHub", "Database"]);
  });

  it("shows the plan progress on the Plan tab with an accessible name, and nothing without a plan", async () => {
    const w = await setup();
    expect(w.find("#tab-plan").attributes("aria-label")).toBeUndefined();
    expect(w.find("#tab-plan .count").exists()).toBe(false);
    commands.projectPlan!.mockImplementation(() => ok({ summary: "", savedAt: null, tasks: [{ text: "A", detail: "", done: true }, { text: "B", detail: "", done: false }] }));
    const w2 = await setup();
    expect(w2.find("#tab-plan .count").text()).toBe("1/2");
    expect(w2.find("#tab-plan").attributes("aria-label")).toBe("Plan, 1 of 2 tasks done");
  });

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
