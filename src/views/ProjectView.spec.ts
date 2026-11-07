import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands, ev } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>>, ev: { current: null as unknown } }));
vi.mock("../bindings", () => ({ commands, get events() { return (ev.current as { events: unknown }).events; } }));
const openDialog = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: openDialog }));
const opener = vi.hoisted(() => ({ openUrl: vi.fn(() => Promise.resolve()), revealItemInDir: vi.fn(() => Promise.resolve()) }));
vi.mock("@tauri-apps/plugin-opener", () => opener);

import { mockCommands, mockEvents, ok, project, session } from "../test-helpers";
import { useNavStore } from "../stores/nav";
import { useSyncStore } from "../stores/sync";
import { useToastStore } from "../stores/toast";
import ProjectView from "./ProjectView.vue";
import type { RepoStatus } from "../bindings";

type W = Awaited<ReturnType<typeof setup>>;
const menuItems = (w: W) => w.findAll("[role='menuitem']").map((b) => b.text());
async function openMenu(w: W) {
  await w.get("button[aria-label='More actions for Notes']").trigger("click");
  await flushPromises();
}
const item = (w: W, text: string) => w.findAll("[role='menuitem']").find((b) => b.text() === text)!;

async function setup(sessions = [session()]) {
  commands.listProjects!.mockImplementation(() => ok([project()]));
  commands.listSessions!.mockImplementation(() => ok(sessions));
  const pinia = createPinia();
  setActivePinia(pinia);
  useNavStore().openProject("p1");
  const w = mount(ProjectView, { global: { plugins: [pinia], stubs: { ChatTab: true, ChangesTab: true, teleport: true } }, attachTo: document.body });
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

describe("ProjectView ⋮ menu", () => {
  it("lists the project actions; Esc closes it without leaving the project", async () => {
    const w = await setup();
    const btn = w.get("button[aria-label='More actions for Notes']");
    expect(btn.attributes("aria-expanded")).toBe("false");
    await openMenu(w);
    expect(btn.attributes("aria-expanded")).toBe("true");
    expect(menuItems(w)).toEqual(["Show in Files", "Copy path", "Change app icon…", "Remove from Studio…"]);
    expect(document.activeElement?.textContent).toBe("Show in Files");

    await w.get("[role='menu']").trigger("keydown", { key: "ArrowDown" });
    expect(document.activeElement?.textContent).toBe("Copy path");
    await w.get("[role='menu']").trigger("keydown", { key: "Escape" });
    expect(w.find("[role='menu']").exists()).toBe(false);
    expect(useNavStore().view).toBe("project");
    expect(document.activeElement).toBe(btn.element);
  });

  it("shows Open on GitHub with a GitHub remote, and Launch and Uninstall for an installed app", async () => {
    commands.listInstalledApps!.mockImplementation(() =>
      ok([{ projectId: "p1", projectName: "Notes", state: { kind: "installed", releaseId: "r1", version: "1.1.0" },
        installedAt: "2026-11-02T14:52:00Z", releasesKept: 2, unverified: false, folder: "/x", icon: null }]),
    );
    const w = await setup();
    useSyncStore().status.p1 = { remote: { github: { owner: "me", name: "notes" } } } as unknown as RepoStatus;
    await openMenu(w);
    expect(menuItems(w)).toEqual([
      "Show in Files", "Copy path", "Change app icon…", "Open on GitHub", "Launch Notes", "Uninstall from GNOME…", "Remove from Studio…",
    ]);
    await item(w, "Open on GitHub").trigger("click");
    await flushPromises();
    expect(opener.openUrl).toHaveBeenCalledWith("https://github.com/me/notes");
    expect(w.find("[role='menu']").exists()).toBe(false);

    await openMenu(w);
    await item(w, "Launch Notes").trigger("click");
    await flushPromises();
    expect(commands.launchInstalled).toHaveBeenCalledWith("p1");

    await openMenu(w);
    await item(w, "Uninstall from GNOME…").trigger("click");
    await flushPromises();
    const dialog = w.get("[role='dialog']");
    expect(dialog.text()).toContain("Uninstall Notes?");
    await dialog.findAll("button").find((b) => b.text() === "Uninstall")!.trigger("click");
    await flushPromises();
    expect(commands.uninstallApp).toHaveBeenCalledWith("p1");
    expect(useToastStore().current?.text).toBe("Uninstalled Notes from GNOME.");
  });

  it("Remove from Studio keeps the folder unless Trash is ticked, then returns to Projects", async () => {
    const w = await setup();
    await openMenu(w);
    await item(w, "Remove from Studio…").trigger("click");
    await flushPromises();
    const dialog = w.get("[role='dialog']");
    expect(dialog.text()).toContain("Remove Notes from Studio?");
    expect(dialog.text()).toContain("The project folder stays where it is.");
    const trash = dialog.get("input[type='checkbox']");
    expect((trash.element as HTMLInputElement).checked).toBe(false);
    expect(document.activeElement).toBe(trash.element); // not the destructive button
    await trash.setValue(true);
    expect(dialog.text()).toContain("You can restore it from the Trash in Files.");

    await dialog.findAll("button").find((b) => b.text() === "Remove")!.trigger("click");
    await flushPromises();
    expect(commands.removeProject).toHaveBeenCalledWith("p1", true);
    expect(useNavStore().view).toBe("projects");
    expect(useToastStore().current?.text).toBe("Removed Notes from Studio. The folder is in the Trash.");
  });

  it("a refused removal stays in the dialog with the reason", async () => {
    commands.removeProject!.mockImplementation(() =>
      Promise.resolve({ status: "error", error: { message: "Notes is installed in GNOME. Uninstall it first, then remove the project.", fieldErrors: [] } }),
    );
    const w = await setup();
    await openMenu(w);
    await item(w, "Remove from Studio…").trigger("click");
    await flushPromises();
    const dialog = w.get("[role='dialog']");
    await dialog.findAll("button").find((b) => b.text() === "Remove")!.trigger("click");
    await flushPromises();
    expect(commands.removeProject).toHaveBeenCalledWith("p1", false);
    expect(dialog.get("[role='alert']").text()).toContain("Uninstall it first");
    expect(useNavStore().view).toBe("project");
  });
});
