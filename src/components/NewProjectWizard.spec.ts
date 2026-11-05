import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../bindings", () => ({ commands }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { err, mockCommands, ok, project } from "../test-helpers";
import { useSettingsStore } from "../stores/settings";
import { useNavStore } from "../stores/nav";
import { useToastStore } from "../stores/toast";
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

  it("database box is unticked by default and sent as false", async () => {
    commands.createProject!.mockImplementation(() => ok(project()));
    const w = setup();
    const box = w.get<HTMLInputElement>("#np-db-hint").element.previousElementSibling!.querySelector("input")!;
    expect(box.checked).toBe(false);
    expect(w.get("#np-db-hint").text()).toBe("For apps that save things like scores, notes or settings");
    await w.get("#np-name").setValue("Notes");
    await settle();
    await w.get("form").trigger("submit");
    await flushPromises();
    expect(commands.createProject).toHaveBeenCalledWith(expect.objectContaining({ database: false }));
  });

  it("sends database: true when ticked", async () => {
    commands.createProject!.mockImplementation(() => ok(project()));
    const w = setup();
    await w.get<HTMLInputElement>("#np-db-hint").element.previousElementSibling!.querySelector("input")!.click();
    await w.get("#np-name").setValue("Notes");
    await settle();
    await w.get("form").trigger("submit");
    await flushPromises();
    expect(commands.createProject).toHaveBeenCalledWith(expect.objectContaining({ database: true }));
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

describe("NewProjectWizard GitHub checkbox", () => {
  const boxes = (w: ReturnType<typeof setup>) => w.findAll<HTMLInputElement>("input[type=checkbox]");
  const signedIn = () => ok({ kind: "signedIn", login: "stefan", gitProtocol: "https" });
  const fillAndCreate = async (w: ReturnType<typeof setup>) => {
    await w.get("#np-name").setValue("Notes");
    await settle();
    await w.get("form").trigger("submit");
    await flushPromises();
  };

  it("is off by default, disabled with 'Needs Git' when Git is off", async () => {
    const w = setup();
    await flushPromises();
    const [git, hub] = boxes(w);
    expect(hub!.element.checked).toBe(false);
    expect(w.text()).toContain("Also create a private GitHub repository");
    await git!.setValue(false);
    expect(hub!.attributes("disabled")).toBeDefined();
    expect(w.text()).toContain("Needs Git");
  });

  it("shows 'as <login>' when signed in and 'sign in' hint when signed out and ticked", async () => {
    commands.githubAuthStatus!.mockImplementation(signedIn);
    let w = setup();
    await flushPromises();
    expect(w.text()).toContain("as stefan");
    document.body.innerHTML = "";
    commands.githubAuthStatus!.mockImplementation(() => ok({ kind: "signedOut" }));
    w = setup();
    await flushPromises();
    expect(w.text()).not.toContain("You'll sign in");
    await boxes(w)[1]!.setValue(true);
    expect(w.text()).toContain("You'll sign in on the project's GitHub tab");
  });

  it("signed in: sends createGithub, creates the repo after the project", async () => {
    commands.githubAuthStatus!.mockImplementation(signedIn);
    commands.createProject!.mockImplementation(() => ok(project()));
    commands.githubCreateRepo!.mockImplementation(() => ok({}));
    const w = setup();
    await flushPromises();
    await boxes(w)[1]!.setValue(true);
    await fillAndCreate(w);
    expect(commands.createProject).toHaveBeenCalledWith(expect.objectContaining({ createGithub: { name: "notes", private: true, description: null } }));
    expect(commands.githubCreateRepo).toHaveBeenCalledWith("p1", "notes", true, null);
    expect(w.emitted("close")).toBeTruthy();
  });

  it("failure keeps the project, toasts and opens the GitHub tab", async () => {
    commands.githubAuthStatus!.mockImplementation(signedIn);
    commands.createProject!.mockImplementation(() => ok(project()));
    commands.githubCreateRepo!.mockImplementation(() => Promise.resolve({ status: "error", error: { kind: "nameTaken" } }));
    const w = setup();
    await flushPromises();
    await boxes(w)[1]!.setValue(true);
    await fillAndCreate(w);
    expect(w.emitted("created")).toBeTruthy();
    expect(useToastStore().current?.text).toBe("You already have a repository named notes.");
    expect(useNavStore().projectId).toBe("p1");
    expect(useNavStore().tab).toBe("github");
  });

  it("signed out: no repo call, opens the project on the GitHub tab", async () => {
    commands.createProject!.mockImplementation(() => ok(project()));
    const w = setup();
    await flushPromises();
    await boxes(w)[1]!.setValue(true);
    await fillAndCreate(w);
    expect(commands.githubCreateRepo).not.toHaveBeenCalled();
    expect(useNavStore().tab).toBe("github");
  });

  it("unticked sends no createGithub and stays on the dashboard", async () => {
    commands.createProject!.mockImplementation(() => ok(project()));
    const w = setup();
    await flushPromises();
    await fillAndCreate(w);
    expect(commands.createProject).toHaveBeenCalledWith(expect.objectContaining({ createGithub: null }));
    expect(useNavStore().projectId).toBeNull();
  });
});
