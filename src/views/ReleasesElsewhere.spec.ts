import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import type { AgentEvent, BuildStatus } from "../bindings";

const { commands, ev } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>>, ev: { current: null as unknown } }));
vi.mock("../bindings", () => ({ commands, get events() { return (ev.current as { events: unknown }).events; } }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(), revealItemInDir: vi.fn() }));
vi.mock("@tauri-apps/plugin-notification", () => ({ isPermissionGranted: vi.fn(), requestPermission: vi.fn(), sendNotification: vi.fn() }));

import { mockCommands, mockEvents, ok, project, session } from "../test-helpers";
import ProjectCard from "../components/ProjectCard.vue";
import { useBuildsStore } from "../stores/builds";
import { useNavStore } from "../stores/nav";
import { useRunsStore } from "../stores/runs";
import ChatTab from "./ChatTab.vue";
import ProjectView from "./ProjectView.vue";

const profile = { id: "pr1", name: "Claude", provider: "claude", executable: "claude", env: [], model: null, inactivitySecs: 600, instructions: "", isDefault: true, enabled: true };
const evs: AgentEvent[] = [{ kind: "fileChange", seq: 2, path: "a.ts" }, { kind: "completed", seq: 9, outcome: "success", denials: [] }];
const building = (): BuildStatus => ({
  jobId: "b1", projectId: "p1", version: "1.1.0", state: "running", phase: "compiling", startedAt: null, completedAt: null,
  errorKind: null, errorMessage: null, releaseId: null, live: true, hasLog: true,
});

beforeEach(() => {
  document.body.innerHTML = "";
  localStorage.clear();
  Object.assign(commands, mockCommands(), {
    listInstalledApps: vi.fn(() => ok([])), getBuildStatus: vi.fn(() => ok(null)), getBuildLog: vi.fn(), listReleases: vi.fn(), getInstallState: vi.fn(),
  });
  ev.current = mockEvents();
});

async function chat(pinia = createPinia()) {
  commands.listSessions!.mockImplementation(() => ok([session()]));
  commands.getSessionEvents!.mockImplementation(() => ok(evs));
  commands.listAgentProfiles!.mockImplementation(() => ok([profile]));
  setActivePinia(pinia);
  await useRunsStore().loadSessions("p1");
  const w = mount(ChatTab, { props: { project: project() }, global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return { w, pinia };
}

describe("Chat (M4-DR13, M4-DR23)", () => {
  it("the successful Make-changes end row has Build release…, which opens Releases with the dialog", async () => {
    const { w } = await chat();
    const link = w.findAll(".end button").find((b) => b.text() === "Build release…")!;
    await link.trigger("click");
    const nav = useNavStore();
    expect(nav.tab).toBe("releases");
    expect(nav.buildDialogProject).toBe("p1");
  });

  it("while a build is live, Make changes is disabled and the strip says why", async () => {
    const { w, pinia } = await chat();
    useBuildsStore(pinia).status.p1 = building();
    await flushPromises();
    const radio = w.find("input[value='changes']");
    expect(radio.attributes("disabled")).toBeDefined();
    expect(w.find(".strip").text()).toContain("Building 1.1.0");
    expect(w.find(".strip").text()).toContain("Make changes is off");
    // Plan first still works.
    expect(w.find("input[value='plan']").attributes("disabled")).toBeUndefined();
  });

  it("Run stays off in Make-changes mode while building", async () => {
    localStorage.setItem("studio.mode.p1", "changes");
    const { w, pinia } = await chat();
    useBuildsStore(pinia).status.p1 = building();
    await w.find("textarea").setValue("do it");
    await flushPromises();
    const run = w.findAll("button").find((b) => b.text() === "Run")!;
    expect((w.find("input[value='changes']").element as HTMLInputElement).checked).toBe(true);
    expect(run.attributes("disabled")).toBeDefined();
  });
});

describe("ProjectCard (M4-DR20)", () => {
  it("shows Installed <v> from the Installed Apps list", async () => {
    commands.listInstalledApps!.mockImplementation(() => ok([{ projectId: "p1", projectName: "Notes", state: { kind: "installed", releaseId: "r1", version: "1.2.0" }, installedAt: "", releasesKept: 2, unverified: false, folder: "/x" }]));
    const pinia = createPinia();
    setActivePinia(pinia);
    const w = mount(ProjectCard, { props: { project: project() }, global: { plugins: [pinia] } });
    await flushPromises();
    expect(w.text()).toContain("Installed 1.2.0");
    expect(w.text()).not.toContain("Building…");
  });

  it("Building… wins while a build is live", async () => {
    commands.getBuildStatus!.mockImplementation(() => ok(building()));
    const pinia = createPinia();
    setActivePinia(pinia);
    const w = mount(ProjectCard, { props: { project: project() }, global: { plugins: [pinia] } });
    await flushPromises();
    expect(w.text()).toContain("Building…");
  });
});

describe("ProjectView", () => {
  it("has a Releases tab with its caption", async () => {
    commands.listProjects!.mockImplementation(() => ok([project()]));
    commands.listSessions!.mockImplementation(() => ok([]));
    commands.getBuildLog!.mockImplementation(() => ok(null));
    commands.listReleases!.mockImplementation(() => ok([]));
    commands.getInstallState!.mockImplementation(() => ok({ kind: "notInstalled" }));
    const pinia = createPinia();
    setActivePinia(pinia);
    useNavStore().openProject("p1");
    const w = mount(ProjectView, { global: { plugins: [pinia], stubs: { ChatTab: true } }, attachTo: document.body });
    await flushPromises();
    expect(w.findAll("[role='tab']").map((t) => t.text())).toEqual(["Chat", "Plan", "Changes", "Preview", "Source", "Releases", "GitHub", "Database"]);
    await w.find("#tab-releases").trigger("click");
    await flushPromises();
    expect(w.text()).toContain("Builds of this app and what GNOME launches");
    expect(w.find(".picker").exists()).toBe(false);
  });
});
