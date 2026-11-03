import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import type { BuildStatus, InstallState, ReleaseView } from "../bindings";

const { commands, reveal } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>>, reveal: vi.fn() }));
vi.mock("../bindings", () => ({
  commands,
  events: { buildStatusEvent: { listen: vi.fn(async () => () => {}) }, buildLogEvent: { listen: vi.fn(async () => () => {}) } },
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: reveal }));

import { ok, err, project, session } from "../test-helpers";
import { useNavStore } from "../stores/nav";
import { useRunsStore } from "../stores/runs";
import { useToastStore } from "../stores/toast";
import ReleasesTab from "./ReleasesTab.vue";

const rel = (over: Partial<ReleaseView> = {}): ReleaseView => ({
  id: "r1", version: "1.0.0", createdAt: "2026-11-01T10:00:00Z", sizeBytes: 90_000_000, sourceCommit: "abcdef1234567", dirtySource: false,
  smoke: "passed", smokeDetail: null, notes: null, lastActivatedAt: null, filesRemoved: false, appimagePath: "/data/r1/Notes.AppImage", ...over,
});
const bst = (over: Partial<BuildStatus> = {}): BuildStatus => ({
  jobId: "b1", projectId: "p1", version: "1.1.0", state: "running", phase: "compiling", startedAt: new Date().toISOString(),
  completedAt: null, errorKind: null, errorMessage: null, releaseId: null, live: true, hasLog: true, ...over,
});
const installed = (id = "r1", version = "1.0.0"): InstallState => ({ kind: "installed", releaseId: id, version });

async function setup(o: { releases?: ReleaseView[]; install?: InstallState; build?: BuildStatus | null; lines?: string[]; sessions?: ReturnType<typeof session>[] } = {}) {
  commands.getBuildStatus!.mockImplementation(() => ok(o.build ?? null));
  commands.getBuildLog!.mockImplementation(() => ok(o.build ? { jobId: o.build.jobId, lines: o.lines ?? [], nextSeq: (o.lines ?? []).length, logTruncated: false } : null));
  commands.listReleases!.mockImplementation(() => ok(o.releases ?? []));
  commands.getInstallState!.mockImplementation(() => ok(o.install ?? { kind: "notInstalled" }));
  const pinia = createPinia();
  setActivePinia(pinia);
  if (o.sessions) commands.listSessions!.mockImplementation(() => ok(o.sessions));
  if (o.sessions) await useRunsStore().loadSessions("p1");
  const w = mount(ReleasesTab, { props: { project: project() }, global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return { w, nav: useNavStore(), toast: useToastStore() };
}
const btn = (w: { findAll: (s: string) => { text(): string; trigger(e: string): Promise<void>; attributes(n: string): string | undefined }[] }, text: string) =>
  w.findAll("button").find((b) => b.text() === text);
const dialogBtn = (text: string) => [...document.querySelectorAll<HTMLButtonElement>("dialog button, [role='dialog'] button")].find((b) => b.textContent?.trim() === text);

beforeEach(() => {
  document.body.innerHTML = "";
  reveal.mockReset();
  Object.assign(commands, {
    getBuildStatus: vi.fn(), getBuildLog: vi.fn(), listReleases: vi.fn(), getInstallState: vi.fn(), listInstalledApps: vi.fn(() => ok([])),
    startBuild: vi.fn(), stopBuild: vi.fn(() => ok(null)), activateRelease: vi.fn(), deleteRelease: vi.fn(() => ok(null)), uninstallApp: vi.fn(() => ok(null)),
    launchInstalled: vi.fn(() => ok(null)), repairLauncher: vi.fn(), nextBuildVersion: vi.fn(() => ok("1.0.0")),
    sourceStatus: vi.fn(() => ok({ entries: [] })), listSessions: vi.fn(() => ok([])),
  });
});

describe("status row", () => {
  it("never built: Build first release… opens the dialog pre-filled with the next version", async () => {
    const { w } = await setup();
    expect(w.text()).toContain("has no releases yet");
    await btn(w, "Build first release…")!.trigger("click");
    await flushPromises();
    expect(document.body.textContent).toContain("This is the first release.");
    expect(dialogBtn("Build 1.0.0")).toBeTruthy();
  });

  it("built, not installed: primary is Install <newest passed>", async () => {
    const { w, toast } = await setup({ releases: [rel()] });
    commands.activateRelease!.mockImplementation(() => ok({ state: installed(), previousReleaseId: null, previousVersion: null }));
    expect(w.text()).toContain("built but not installed");
    await btn(w, "Install 1.0.0")!.trigger("click");
    await flushPromises();
    expect(commands.activateRelease).toHaveBeenCalledWith("p1", "r1");
    expect(toast.current!.text).toContain("GNOME now starts Notes 1.0.0");
    expect(toast.current!.text).toContain("Activities");
  });

  it("installed: Launch and Build new release…", async () => {
    const { w } = await setup({ releases: [rel()], install: installed() });
    expect(w.text()).toContain("GNOME starts Notes 1.0.0");
    await btn(w, "Launch")!.trigger("click");
    expect(commands.launchInstalled).toHaveBeenCalledWith("p1");
    expect(btn(w, "Build new release…")).toBeTruthy();
    expect(btn(w, "Install 1.0.0")).toBeUndefined();
  });

  it("a newer tested build makes Install the primary action; an untested one does not", async () => {
    const newer = rel({ id: "r2", version: "1.1.0", createdAt: "2026-11-02T10:00:00Z" });
    const a = await setup({ releases: [rel(), newer], install: installed() });
    expect(a.w.text()).toContain("A newer tested build exists: 1.1.0");
    expect(a.w.find(".btn.primary").text()).toBe("Install 1.1.0");
    document.body.innerHTML = "";
    const b = await setup({ releases: [rel(), { ...newer, smoke: "unverified" }], install: installed() });
    expect(b.w.find(".btn.primary").exists()).toBe(false);
  });

  it("launcher missing: Repair", async () => {
    const { w } = await setup({ releases: [rel()], install: { kind: "launcherMissing", releaseId: "r1", version: "1.0.0" } });
    expect(w.text()).toContain("Launcher missing");
    commands.repairLauncher!.mockImplementation(() => ok(installed()));
    await btn(w, "Repair")!.trigger("click");
    expect(commands.repairLauncher).toHaveBeenCalledWith("p1");
  });

  it("Build is disabled with a reason while a Make-changes run is live (M4-DR23)", async () => {
    const { w } = await setup({ sessions: [session({ mode: "changes", jobState: "running" })] });
    expect(btn(w, "Build first release…")!.attributes("disabled")).toBeDefined();
    expect(w.text()).toContain("Claude is changing this project");
  });
});

describe("build dialog", () => {
  it("shows Highest so far, the uncommitted-changes note, notes, and starts the build", async () => {
    commands.sourceStatus!.mockImplementation(() => ok({ entries: [{ path: "a" }] }));
    commands.nextBuildVersion!.mockImplementation(() => ok("1.1.0"));
    const { w } = await setup({ releases: [rel()], install: installed() });
    await btn(w, "Build new release…")!.trigger("click");
    await flushPromises();
    expect(document.body.textContent).toContain("Highest so far: 1.0.0");
    expect(document.body.textContent).toContain("Uncommitted changes in your project will be included");
    document.querySelector<HTMLTextAreaElement>("dialog textarea, [role='dialog'] textarea")!.value = "";
    commands.startBuild!.mockImplementation(() => ok(bst()));
    dialogBtn("Build 1.1.0")!.click();
    await flushPromises();
    expect(commands.startBuild).toHaveBeenCalledWith("p1", "1.1.0", null);
    expect(w.text()).toContain("Building 1.1.0");
  });

  it("shows the backend's refusal and field errors", async () => {
    const { w } = await setup({ releases: [rel()], install: installed() });
    await btn(w, "Build new release…")!.trigger("click");
    await flushPromises();
    commands.startBuild!.mockImplementation(() => err("Invalid", [{ field: "version", message: "1.0.0 already exists" }]));
    dialogBtn("Build 1.0.0")!.click();
    await flushPromises();
    expect(document.body.textContent).toContain("1.0.0 already exists");
  });

  it("opens from the Chat handoff (M4-DR13)", async () => {
    const pinia = createPinia();
    setActivePinia(pinia);
    useNavStore().openBuildDialog("p1");
    commands.getBuildStatus!.mockImplementation(() => ok(null));
    commands.getBuildLog!.mockImplementation(() => ok(null));
    commands.listReleases!.mockImplementation(() => ok([]));
    commands.getInstallState!.mockImplementation(() => ok({ kind: "notInstalled" }));
    mount(ReleasesTab, { props: { project: project() }, global: { plugins: [pinia] }, attachTo: document.body });
    await flushPromises();
    expect(dialogBtn("Build 1.0.0")).toBeTruthy();
    expect(useNavStore().buildDialogProject).toBeNull();
  });
});

describe("building and failures", () => {
  it("shows the stepper, elapsed time, the 5 second warning, Stop and the log", async () => {
    const { w } = await setup({ build: bst({ phase: "storing" }), lines: ["Compiling app"] });
    expect(w.text()).toContain("Building 1.1.0");
    expect(w.text()).toContain("opens for 5 seconds");
    const steps = w.findAll(".step");
    expect(steps.map((s) => s.classes()[1])).toEqual(["done", "done", "active", "pending"]); // storing = Package
    expect(w.text()).toContain("Compiling app");
    await btn(w, "Stop")!.trigger("click");
    expect(commands.stopBuild).toHaveBeenCalledWith("p1");
  });

  it("build failed: first error, Ask Claude to fix, and which version GNOME still starts", async () => {
    const failed = bst({ live: false, state: "failed", errorKind: "buildFailed", errorMessage: "error[E0425]: cannot find value `x`\nmore" });
    const { w, nav } = await setup({ build: failed, releases: [rel()], install: installed(), lines: ["error[E0425]: cannot find value `x`", " --> src/main.rs:5:3"] });
    const c = w.find(".callout.danger");
    expect(c.text()).toContain("error[E0425]: cannot find value `x`");
    expect(c.text()).not.toContain("more");
    expect(c.text()).toContain("GNOME still starts 1.0.0");
    await btn(w, "Ask Claude to fix")!.trigger("click");
    expect(nav.composerHandoff!.projectId).toBe("p1");
    expect(nav.composerHandoff!.text).toContain("release build");
    expect(nav.composerHandoff!.text).toContain("src/main.rs:5:3");
    expect(nav.tab).toBe("chat");
  });

  it("preflight failure offers Open Doctor instead", async () => {
    const { w, nav } = await setup({ build: bst({ live: false, state: "failed", errorKind: "preflight", errorMessage: "pnpm not found" }) });
    expect(btn(w, "Ask Claude to fix")).toBeUndefined();
    await btn(w, "Open Doctor")!.trigger("click");
    expect(nav.view).toBe("doctor");
  });

  it("the log collapses to Build log after the build ends", async () => {
    const { w } = await setup({ build: bst({ live: false, state: "succeeded" }), lines: ["done"] });
    expect(w.find("[role='log']").exists()).toBe(false);
    await btn(w, "Build log")!.trigger("click");
    expect(w.find("[role='log']").text()).toContain("done");
  });
});

describe("release table", () => {
  const r1 = rel({ lastActivatedAt: "2026-11-01T11:00:00Z", notes: "First cut" });
  const r2 = rel({ id: "r2", version: "1.1.0", createdAt: "2026-11-02T10:00:00Z", dirtySource: true, smoke: "unverified" });
  const r3 = rel({ id: "r3", version: "1.2.0", createdAt: "2026-11-03T10:00:00Z", smoke: "failed", smokeDetail: "Segmentation fault\nstack" });
  const r4 = rel({ id: "r4", version: "0.9.0", createdAt: "2026-10-30T10:00:00Z", filesRemoved: true });
  const row = (w: { find: (s: string) => { text(): string; findAll(s: string): { text(): string }[] } }, v: string) => w.find(`tr[data-release='${v}']`);

  it("renders version, notes, source, test badge and status", async () => {
    const { w } = await setup({ releases: [r1, r2, r3, r4], install: installed("r2", "1.1.0") });
    const a = row(w, "1.0.0");
    expect(a.text()).toContain("First cut");
    expect(a.text()).toContain("abcdef1");
    expect(a.text()).toContain("Passed");
    expect(a.text()).toContain("Previous"); // latest lastActivatedAt among non-active
    const b = row(w, "1.1.0");
    expect(b.text()).toContain("Active");
    expect(b.text()).toContain("uncommitted edits");
    expect(b.text()).toContain("Unverified");
    expect(w.find("tr[data-release='1.0.0'] .badge.ok").exists()).toBe(true);
    expect(w.find("tr[data-release='1.1.0'] .badge.warn").exists()).toBe(true);
  });

  it("one action per row: Roll back for older, Install anyway for unverified", async () => {
    const { w } = await setup({ releases: [r1, r2], install: installed("r2", "1.1.0") });
    expect(row(w, "1.0.0").text()).toContain("Roll back");
    const unverified = rel({ id: "r5", version: "1.3.0", createdAt: "2026-11-04T10:00:00Z", smoke: "unverified" });
    document.body.innerHTML = "";
    const b = await setup({ releases: [r1, unverified], install: installed() });
    expect(row(b.w, "1.3.0").text()).toContain("Install anyway");
  });

  it("test launch failed: only Details and Delete, with the crash line", async () => {
    const { w } = await setup({ releases: [r1, r3], install: installed() });
    const r = w.find("tr[data-release='1.2.0']");
    expect(r.findAll("button").map((b) => b.text())).toEqual(["Details", "Delete"]);
    expect(r.text()).toContain("Segmentation fault");
    expect(r.text()).not.toContain("stack");
    await r.findAll("button")[0]!.trigger("click");
    expect(r.text()).toContain("stack");
  });

  it("removed builds hide behind Show N removed builds", async () => {
    const { w } = await setup({ releases: [r1, r4] });
    expect(w.find("tr[data-release='0.9.0']").exists()).toBe(false);
    await btn(w, "Show 1 removed build")!.trigger("click");
    expect(w.find("tr[data-release='0.9.0']").text()).toContain("Files removed");
  });

  it("Roll back acts at once with an Undo toast", async () => {
    const { w, toast } = await setup({ releases: [r1, r2], install: installed("r2", "1.1.0") });
    commands.activateRelease!.mockImplementation(() => ok({ state: installed("r1", "1.0.0"), previousReleaseId: "r2", previousVersion: "1.1.0" }));
    await btn(w, "Roll back")!.trigger("click");
    await flushPromises();
    expect(commands.activateRelease).toHaveBeenCalledWith("p1", "r1");
    expect(toast.current!.text).toBe("GNOME now starts Notes 1.0.0");
    expect(toast.current!.action!.label).toBe("Undo");
  });

  it("Delete asks first, naming the build; Show in Files reveals the AppImage", async () => {
    const { w } = await setup({ releases: [r1, r2], install: installed("r2", "1.1.0") });
    const menu = w.find("tr[data-release='1.0.0'] .pop");
    const item = (t: string) => menu.findAll("button").find((b) => b.text() === t)!;
    await item("Show in Files").trigger("click");
    expect(reveal).toHaveBeenCalledWith("/data/r1/Notes.AppImage");
    await item("Delete").trigger("click");
    expect(document.body.textContent).toContain("Delete Notes 1.0.0?");
    expect(commands.deleteRelease).not.toHaveBeenCalled();
    dialogBtn("Delete")!.click();
    await flushPromises();
    expect(commands.deleteRelease).toHaveBeenCalledWith("p1", "r1");
  });

  it("the active build has no Delete", async () => {
    const { w } = await setup({ releases: [r1, r2], install: installed("r2", "1.1.0") });
    const labels = w.find("tr[data-release='1.1.0'] .pop").findAll("button").map((b) => b.text());
    expect(labels).not.toContain("Delete");
  });
});

describe("uninstall", () => {
  it("Uninstall from GNOME… opens the dialog and confirms", async () => {
    const { w } = await setup({ releases: [rel()], install: installed() });
    await btn(w, "Uninstall from GNOME…")!.trigger("click");
    expect(document.body.textContent).toContain("Uninstall Notes?");
    dialogBtn("Uninstall")!.click();
    await flushPromises();
    expect(commands.uninstallApp).toHaveBeenCalledWith("p1");
  });

  it("is hidden when nothing is installed", async () => {
    const { w } = await setup({ releases: [rel()] });
    expect(btn(w, "Uninstall from GNOME…")).toBeUndefined();
  });
});

describe("imported folders", () => {
  it("explains that releases are unavailable", async () => {
    const pinia = createPinia();
    setActivePinia(pinia);
    const w = mount(ReleasesTab, { props: { project: project({ supportsOperations: false, templateId: "existing" }) }, global: { plugins: [pinia] } });
    await flushPromises();
    expect(w.text()).toContain("Releases aren't available");
  });
});
