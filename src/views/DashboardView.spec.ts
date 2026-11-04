import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands, dialogOpen } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>>, dialogOpen: vi.fn() }));
vi.mock("../bindings", () => ({ commands }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: dialogOpen }));
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: vi.fn() }));

import { mockCommands, ok, project } from "../test-helpers";
import DashboardView from "./DashboardView.vue";

async function setup(list = [project()]) {
  commands.listProjects!.mockImplementation(() => ok(list));
  const pinia = createPinia();
  setActivePinia(pinia);
  const w = mount(DashboardView, { global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return w;
}

beforeEach(() => {
  document.body.innerHTML = "";
  Object.assign(commands, mockCommands());
  dialogOpen.mockReset();
});

describe("DashboardView", () => {
  it("shows the empty state with zero projects", async () => {
    const w = await setup([]);
    expect(w.text()).toContain("Create Your First App");
    expect(w.text()).toContain("Import a folder");
  });

  it("offers Relink for a missing workspace and relinks to the picked folder", async () => {
    commands.relinkProject!.mockImplementation(() => ok(project()));
    dialogOpen.mockResolvedValue("/new/place");
    const w = await setup([project({ state: { kind: "missingWorkspace" } })]);
    expect(w.text()).toContain("Folder missing");
    const relink = w.findAll("button").find((b) => b.text() === "Relink")!;
    await relink.trigger("click");
    await flushPromises();
    expect(commands.relinkProject).toHaveBeenCalledWith("p1", "/new/place");
  });

  it("notes that imported folders cannot build", async () => {
    const w = await setup([project({ supportsOperations: false, templateId: "existing" })]);
    expect(w.text()).toContain("build and install are not available");
  });

  it("offers Clone from GitHub in the header and the empty state, and opens the dialog", async () => {
    const clone = (w: Awaited<ReturnType<typeof setup>>) => w.findAll("button").find((b) => b.text() === "Clone from GitHub")!;
    let w = await setup([]);
    expect(clone(w)).toBeTruthy();
    document.body.innerHTML = "";
    w = await setup();
    await clone(w).trigger("click");
    await flushPromises();
    expect(w.get("[role=dialog]").text()).toContain("Repository");
  });

  describe("import flow", () => {
    const existing = project({ id: "old", name: "Notes", workspacePath: "/old/notes" });
    const importWith = async (outcome: unknown) => {
      commands.importProject!.mockImplementation(() => ok(outcome));
      dialogOpen.mockResolvedValue("/new/notes");
      const w = await setup([]);
      await w.findAll("button").find((b) => b.text() === "Import a folder")!.trigger("click");
      await flushPromises();
      return w;
    };
    const dialogBtn = (w: Awaited<ReturnType<typeof setup>>, label: string) =>
      w.findAll("[role=dialog] button").find((b) => b.text() === label)!;

    it("asks to relink and calls relinkProject on confirm", async () => {
      commands.relinkProject!.mockImplementation(() => ok(existing));
      const w = await importWith({ kind: "needsRelink", existing, newPath: "/new/notes" });
      const text = w.get("[role=dialog]").text();
      expect(text).toContain("This folder contains Notes, which the library lists at /old/notes (missing). Relink it here?");
      await dialogBtn(w, "Relink").trigger("click");
      await flushPromises();
      expect(commands.relinkProject).toHaveBeenCalledWith("old", "/new/notes");
      expect(w.find("[role=dialog]").exists()).toBe(false);
    });

    it("explains the copy and calls importProjectAsCopy on confirm", async () => {
      commands.importProjectAsCopy!.mockImplementation(() => ok(project({ id: "new" })));
      const w = await importWith({ kind: "needsCopyConfirm", existing, newPath: "/new/notes" });
      const text = w.get("[role=dialog]").text();
      expect(text).toContain("/old/notes");
      expect(text).toContain("new project ID");
      expect(text).toContain("project.json and tauri.conf.json");
      await dialogBtn(w, "Import as copy").trigger("click");
      await flushPromises();
      expect(commands.importProjectAsCopy).toHaveBeenCalledWith("/new/notes");
    });

    it("calls nothing on cancel", async () => {
      const w = await importWith({ kind: "needsCopyConfirm", existing, newPath: "/new/notes" });
      await dialogBtn(w, "Cancel").trigger("click");
      await flushPromises();
      expect(commands.importProjectAsCopy).not.toHaveBeenCalled();
      expect(commands.relinkProject).not.toHaveBeenCalled();
      expect(w.find("[role=dialog]").exists()).toBe(false);
    });

    it("closes on Escape without calling anything", async () => {
      const w = await importWith({ kind: "needsRelink", existing, newPath: "/new/notes" });
      await w.get("[role=dialog]").trigger("keydown", { key: "Escape" });
      expect(commands.relinkProject).not.toHaveBeenCalled();
      expect(w.find("[role=dialog]").exists()).toBe(false);
    });

    it("tells the user when the project is already in the library", async () => {
      const w = await importWith({ kind: "alreadyInLibrary", project: existing });
      expect(w.text()).toContain('"Notes" is already in your library.');
    });
  });
});
