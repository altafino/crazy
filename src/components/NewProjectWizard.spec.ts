import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../bindings", () => ({ commands }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { err, mockCommands, ok, project } from "../test-helpers";
import { useSettingsStore } from "../stores/settings";
import NewProjectWizard from "./NewProjectWizard.vue";

function setup() {
  const pinia = createPinia();
  setActivePinia(pinia);
  useSettingsStore().settings = { defaultProjectsDir: "/home/u/Projects", toolOverrides: {} };
  return mount(NewProjectWizard, { global: { plugins: [pinia] }, attachTo: document.body });
}
const settle = async () => {
  await vi.advanceTimersByTimeAsync(350);
  await flushPromises();
};
const createBtn = (w: ReturnType<typeof setup>) => w.get("button[type=submit]");

beforeEach(() => {
  vi.useFakeTimers();
  document.body.innerHTML = "";
  Object.assign(commands, mockCommands());
  commands.validateNewProject!.mockImplementation(() => ok([]));
  commands.listProjects!.mockImplementation(() => ok([]));
});

describe("NewProjectWizard", () => {
  it("derives the slug from the name until the slug is edited", async () => {
    const w = setup();
    const slug = w.get<HTMLInputElement>("#np-slug");
    await w.get("#np-name").setValue("My Cool App");
    expect(slug.element.value).toBe("my-cool-app");
    expect(w.text()).toContain("Will create: /home/u/Projects/my-cool-app");

    await slug.setValue("custom");
    await w.get("#np-name").setValue("Another Name");
    expect(slug.element.value).toBe("custom");
  });

  it("shows a field error next to its field and disables Create", async () => {
    commands.validateNewProject!.mockImplementation(() => ok([{ field: "slug", message: "Slug already used" }]));
    const w = setup();
    await w.get("#np-name").setValue("Notes");
    await settle();

    const msg = w.get("#slug-err");
    expect(msg.text()).toBe("Slug already used");
    expect(w.get("#np-slug").attributes("aria-describedby")).toContain("slug-err");
    expect(w.find("#name-err").exists()).toBe(false);
    expect(createBtn(w).attributes("disabled")).toBeDefined();
  });

  it("creates, refreshes the list and closes", async () => {
    commands.createProject!.mockImplementation(() => ok(project()));
    const w = setup();
    await w.get("#np-name").setValue("Notes");
    await settle();
    expect(createBtn(w).attributes("disabled")).toBeUndefined();

    await w.get("form").trigger("submit");
    await flushPromises();

    expect(commands.createProject).toHaveBeenCalledWith(
      expect.objectContaining({ name: "Notes", parentDir: "/home/u/Projects", initGit: true, identifier: null }),
    );
    expect(commands.listProjects).toHaveBeenCalled();
    expect(w.emitted("close")).toBeTruthy();
    expect(w.emitted("created")![0]).toEqual([project()]);
  });

  it("does not create twice on a double submit", async () => {
    commands.createProject!.mockImplementation(() => ok(project()));
    const w = setup();
    await w.get("#np-name").setValue("Notes");
    await settle();
    await w.get("form").trigger("submit");
    await w.get("form").trigger("submit");
    await flushPromises();
    expect(commands.createProject).toHaveBeenCalledTimes(1);
  });

  it("shows fieldErrors from a failed create inline and the message in the alert", async () => {
    commands.createProject!.mockImplementation(() =>
      err("Could not create project", [{ field: "parentDir", message: "Folder is not writable" }]),
    );
    const w = setup();
    await w.get("#np-name").setValue("Notes");
    await settle();
    await w.get("form").trigger("submit");
    await flushPromises();

    expect(w.get("#parentDir-err").text()).toBe("Folder is not writable");
    expect(w.get("[role=alert]").text()).toContain("Could not create project");
    expect(w.emitted("close")).toBeFalsy();
  });
});
