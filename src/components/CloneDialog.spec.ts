import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands, dialogOpen } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>>, dialogOpen: vi.fn() }));
vi.mock("../bindings", () => ({ commands, events: { githubSignInCodeEvent: { listen: vi.fn(async () => () => {}) } } }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: dialogOpen }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

import { mockCommands, ok, project } from "../test-helpers";
import { useGithubStore } from "../stores/github";
import { useNavStore } from "../stores/nav";
import { useProjectsStore } from "../stores/projects";
import { useSettingsStore } from "../stores/settings";
import CloneDialog from "./CloneDialog.vue";

const repo = (nameWithOwner: string, updatedAt = "2026-01-01T00:00:00Z") => ({ nameWithOwner, private: false, description: null, updatedAt });
const fail = (error: unknown) => Promise.resolve({ status: "error" as const, error });
const signedIn = () => ok({ kind: "signedIn", login: "stefan", gitProtocol: "https" });

async function setup() {
  const pinia = createPinia();
  setActivePinia(pinia);
  useSettingsStore().settings = { defaultProjectsDir: "/home/u/Projects", toolOverrides: {} };
  const w = mount(CloneDialog, { global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return { w, pinia };
}
const btn = (w: ReturnType<typeof mount>, text: string) => w.findAll("button").find((b) => b.text().startsWith(text))!;

beforeEach(() => {
  document.body.innerHTML = "";
  Object.assign(commands, mockCommands());
  commands.githubAuthStatus!.mockImplementation(signedIn);
  commands.listProjects!.mockImplementation(() => ok([]));
  dialogOpen.mockReset();
});

describe("CloneDialog", () => {
  it("lists repos newest first and filters by what is typed", async () => {
    commands.githubListRepos!.mockImplementation(() => ok([repo("stefan/old", "2025-01-01T00:00:00Z"), repo("stefan/quicknotes", "2026-06-01T00:00:00Z")]));
    const { w } = await setup();
    const names = () => w.findAll(".repo").map((b) => b.text());
    expect(names()).toEqual(["stefan/quicknotes", "stefan/old"]);
    await w.get("#cl-spec").setValue("old");
    expect(names()).toEqual(["stefan/old"]);
    expect(w.get("#cl-spec").attributes("placeholder")).toContain("stefan/quicknotes or a GitHub URL");
    expect(w.text()).toContain("Will clone to: /home/u/Projects/old");
  });

  it("shows empty, loading and error list states", async () => {
    commands.githubListRepos!.mockImplementation(() => ok([]));
    expect((await setup()).w.text()).toContain("You have no repositories yet");

    document.body.innerHTML = "";
    commands.githubListRepos!.mockImplementation(() => fail({ kind: "unreachable", message: "x" }));
    const { w } = await setup();
    expect(w.text()).toContain("The connection to GitHub stopped responding.");
    useGithubStore().reposLoading = true;
    await flushPromises();
    expect(w.text()).toContain("Loading your repositories");
  });

  it("signed out: shows Sign in, then the code while signing in", async () => {
    commands.githubAuthStatus!.mockImplementation(() => ok({ kind: "signedOut" }));
    const { w } = await setup();
    expect(w.find(".repos").exists()).toBe(false);
    let finish!: () => void;
    commands.githubSignIn!.mockImplementation(() => new Promise((r) => (finish = () => r({ status: "ok", data: { kind: "signedIn", login: "stefan", gitProtocol: "https" } }))));
    await btn(w, "Sign in").trigger("click");
    await flushPromises();
    expect(w.text()).toContain("Enter this code on GitHub");
    finish();
    await flushPromises();
    expect(commands.githubListRepos).toHaveBeenCalled();
  });

  it("clones a typed spec, shows progress with Stop, then opens the project", async () => {
    vi.useFakeTimers();
    commands.githubListRepos!.mockImplementation(() => ok([]));
    let finish!: () => void;
    commands.cloneProject!.mockImplementation(() => new Promise((r) => (finish = () => r({ status: "ok", data: { kind: "imported", project: project() } }))));
    commands.cloneProjectStop!.mockImplementation(() => ok(true));
    const { w, pinia } = await setup();
    await w.get("#cl-spec").setValue("stefan/quicknotes");
    await w.get("form").trigger("submit");
    await vi.advanceTimersByTimeAsync(2100);
    expect(commands.cloneProject).toHaveBeenCalledWith("stefan/quicknotes", null);
    expect(w.text()).toContain("Cloning stefan/quicknotes · 2 s");
    await btn(w, "Stop").trigger("click");
    expect(commands.cloneProjectStop).toHaveBeenCalledWith("stefan/quicknotes");
    finish();
    await flushPromises();
    expect(w.emitted("close")).toBeTruthy();
    expect(useNavStore(pinia).projectId).toBe("p1");
    expect(useNavStore(pinia).view).toBe("project");
    vi.useRealTimers();
  });

  it("sends a picked folder and shows targetExists with Change folder", async () => {
    commands.githubListRepos!.mockImplementation(() => ok([]));
    dialogOpen.mockResolvedValue("/other");
    commands.cloneProject!.mockImplementation(() => fail({ kind: "gh", error: { kind: "targetExists", path: "/other/quicknotes" } }));
    const { w } = await setup();
    await w.get("#cl-spec").setValue("stefan/quicknotes");
    await btn(w, "Change…").trigger("click");
    await flushPromises();
    await w.get("form").trigger("submit");
    await flushPromises();
    expect(commands.cloneProject).toHaveBeenCalledWith("stefan/quicknotes", "/other");
    expect(w.get("[role=alert]").text()).toContain("/other/quicknotes already exists.");
    expect(btn(w, "Change folder")).toBeTruthy();
    expect(w.emitted("close")).toBeFalsy();
  });

  it("queues a relink confirmation for the Dashboard", async () => {
    commands.githubListRepos!.mockImplementation(() => ok([]));
    const existing = project();
    commands.cloneProject!.mockImplementation(() => ok({ kind: "needsRelink", existing, newPath: "/x" }));
    const { w, pinia } = await setup();
    await w.get("#cl-spec").setValue("stefan/quicknotes");
    await w.get("form").trigger("submit");
    await flushPromises();
    expect(useProjectsStore(pinia).pendingImport?.kind).toBe("needsRelink");
    expect(w.emitted("close")).toBeTruthy();
  });
});
