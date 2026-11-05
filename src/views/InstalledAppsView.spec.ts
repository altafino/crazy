import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands, reveal } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>>, reveal: vi.fn() }));
vi.mock("../bindings", () => ({ commands }));
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: reveal }));

import type { InstalledApp_Serialize } from "../bindings";
import { err, ok } from "../test-helpers";
import { useNavStore } from "../stores/nav";
import { useToastStore } from "../stores/toast";
import InstalledAppsView from "./InstalledAppsView.vue";

const app = (over: Partial<InstalledApp_Serialize> = {}): InstalledApp_Serialize => ({
  projectId: "p1", projectName: "Notes", state: { kind: "installed", releaseId: "r1", version: "1.1.0" },
  installedAt: "2026-11-02T14:52:00Z", releasesKept: 3, unverified: false, folder: "/home/u/apps/notes", icon: null, ...over,
});
const btn = (w: ReturnType<typeof mount>, t: string) => w.findAll("button").find((b) => b.text() === t);

async function mountWith(apps: InstalledApp_Serialize[]) {
  setActivePinia(createPinia());
  commands.listInstalledApps!.mockImplementation(() => ok(apps));
  const w = mount(InstalledAppsView, { attachTo: document.body });
  await flushPromises();
  return w;
}

beforeEach(() => {
  document.body.innerHTML = "";
  for (const k of ["listInstalledApps", "launchInstalled", "uninstallApp", "repairLauncher"]) commands[k] = vi.fn();
  reveal.mockReset();
});

describe("InstalledAppsView", () => {
  it("shows the launcher's icon, or the first letter without one", async () => {
    const w = await mountWith([app({ icon: "data:image/png;base64,AAA" }), app({ projectId: "p2", projectName: "Zed" })]);
    const rows = w.findAll("li.row");
    expect(rows[0]!.get("img").attributes("src")).toBe("data:image/png;base64,AAA");
    expect(rows[1]!.find("img").exists()).toBe(false);
    expect(rows[1]!.get(".icon").text()).toBe("Z");
  });

  it("shows the empty state with Go to Projects", async () => {
    const w = await mountWith([]);
    expect(w.text()).toContain("Releases tab");
    await btn(w, "Go to Projects")!.trigger("click");
    expect(useNavStore().view).toBe("projects");
  });

  it("renders row, caption and footer; Launch and Open project work", async () => {
    const w = await mountWith([app()]);
    expect(w.text()).toContain("Active 1.1.0");
    expect(w.text()).toContain("3 releases kept");
    expect(w.text()).toContain("New apps can take a few seconds");
    commands.launchInstalled!.mockImplementation(() => ok(null));
    await btn(w, "Launch")!.trigger("click");
    expect(commands.launchInstalled).toHaveBeenCalledWith("p1");
    await btn(w, "Open project")!.trigger("click");
    expect(useNavStore().projectId).toBe("p1");
  });

  it("hides Open project for a removed project and shows the unverified badge", async () => {
    const w = await mountWith([app({ projectName: null, unverified: true })]);
    expect(btn(w, "Open project")).toBeUndefined();
    expect(w.text()).toContain("Unverified release");
  });

  it("launcher missing offers Repair instead of Launch", async () => {
    const w = await mountWith([app({ state: { kind: "launcherMissing", releaseId: "r1", version: "1.1.0" } })]);
    expect(w.text()).toContain("Launcher missing");
    expect(w.text()).toContain("removed outside Studio");
    expect(btn(w, "Launch")).toBeUndefined();
    commands.repairLauncher!.mockImplementation(() => ok({ kind: "installed", releaseId: "r1", version: "1.1.0" }));
    await btn(w, "Repair")!.trigger("click");
    expect(commands.repairLauncher).toHaveBeenCalledWith("p1");
  });

  it("unknown release has a caption and no Launch", async () => {
    const w = await mountWith([app({ state: { kind: "unknownRelease", version: null } })]);
    expect(btn(w, "Launch")).toBeUndefined();
    expect(w.text()).toContain("no kept release file");
  });

  it("menu reveals the folder", async () => {
    const w = await mountWith([app()]);
    await w.get("[aria-label='More actions for Notes']").trigger("click");
    await btn(w, "Show in Files")!.trigger("click");
    expect(reveal).toHaveBeenCalledWith("/home/u/apps/notes");
  });

  it("uninstall confirms, calls the backend, refreshes and toasts", async () => {
    const w = await mountWith([app()]);
    await w.get("[aria-label='More actions for Notes']").trigger("click");
    await btn(w, "Uninstall…")!.trigger("click");
    expect(document.body.textContent).toContain("Uninstall Notes?");
    expect(document.body.textContent).toContain("Git history are untouched");
    commands.uninstallApp!.mockImplementation(() => ok(null));
    commands.listInstalledApps!.mockImplementation(() => ok([]));
    const confirm = [...document.body.querySelectorAll("button")].find((b) => b.textContent === "Uninstall")!;
    confirm.click();
    await flushPromises();
    expect(commands.uninstallApp).toHaveBeenCalledWith("p1");
    expect(useToastStore().current?.text).toBe("Uninstalled Notes.");
    expect(w.text()).toContain("No installed apps yet");
  });

  it("shows backend errors inline", async () => {
    const w = await mountWith([app()]);
    commands.launchInstalled!.mockImplementation(() => err("gio failed"));
    await btn(w, "Launch")!.trigger("click");
    await flushPromises();
    expect(w.get("[role=alert]").text()).toContain("gio failed");
  });
});
