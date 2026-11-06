import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import type { BuildStatus, ExportJob, InstallState, ReleaseExports, ReleaseView } from "../bindings";

const { commands, reveal, ev } = vi.hoisted(() => ({
  commands: {} as Record<string, ReturnType<typeof vi.fn>>, reveal: vi.fn(),
  ev: { status: null as null | ((e: { payload: { status: unknown } }) => void) },
}));
vi.mock("../bindings", () => ({
  commands,
  events: { buildStatusEvent: { listen: vi.fn(async (fn) => ((ev.status = fn), () => {})) }, buildLogEvent: { listen: vi.fn(async () => () => {}) } },
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: reveal }));

import { ok, err, project, session } from "../test-helpers";
import { useNavStore } from "../stores/nav";
import { useProjectsStore } from "../stores/projects";
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

async function setup(o: { releases?: ReleaseView[]; install?: InstallState; build?: BuildStatus | null; lines?: string[]; sessions?: ReturnType<typeof session>[]; exports?: ReleaseExports[] } = {}) {
  if (o.exports) commands.listReleaseExports!.mockImplementation(() => ok(o.exports));
  commands.getBuildStatus!.mockImplementation(() => ok(o.build ?? null));
  commands.getBuildLog!.mockImplementation(() => ok(o.build ? { jobId: o.build.jobId, lines: o.lines ?? [], nextSeq: (o.lines ?? []).length, logTruncated: false } : null));
  commands.listReleases!.mockImplementation(() => ok(o.releases ?? []));
  commands.getInstallState!.mockImplementation(() => ok(o.install ?? { kind: "notInstalled" }));
  const pinia = createPinia();
  setActivePinia(pinia);
  useProjectsStore().projects = [project()];
  if (o.sessions) commands.listSessions!.mockImplementation(() => ok(o.sessions));
  if (o.sessions) await useRunsStore().loadSessions("p1");
  const w = mount(ReleasesTab, { props: { project: project() }, global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return { w, nav: useNavStore(), toast: useToastStore() };
}
const btn = (w: { findAll: (s: string) => { text(): string; trigger(e: string): Promise<void>; attributes(n: string): string | undefined }[] }, text: string) =>
  w.findAll("button").find((b) => b.text() === text);
const dialogBtn = (text: string) => [...document.querySelectorAll<HTMLButtonElement>("dialog button, [role='dialog'] button")].find((b) => b.textContent?.trim() === text);

const READY = { docker: "ready", dockerVersion: "29.8.2", detail: null, imagePresent: true, licenseAcceptedAt: "2026-11-01T10:00:00Z", lastTargets: ["linux", "windows"] };
beforeEach(() => {
  document.body.innerHTML = "";
  reveal.mockReset();
  Object.assign(commands, {
    getBuildStatus: vi.fn(), getBuildLog: vi.fn(), listReleases: vi.fn(), getInstallState: vi.fn(), listInstalledApps: vi.fn(() => ok([])),
    startBuild: vi.fn(), stopBuild: vi.fn(() => ok(null)), activateRelease: vi.fn(), deleteRelease: vi.fn(() => ok(null)), uninstallApp: vi.fn(() => ok(null)),
    launchInstalled: vi.fn(() => ok(null)), repairLauncher: vi.fn(), nextBuildVersion: vi.fn(() => ok("1.0.0")),
    sourceStatus: vi.fn(() => ok({ entries: [] })), listSessions: vi.fn(() => ok([])),
    listReleaseExports: vi.fn(() => ok([])), exportReadiness: vi.fn(() => ok(READY)), startExport: vi.fn(),
  });
  ev.status = null;
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
    expect(document.body.textContent).toMatch(/This removes Notes( \S+)? from GNOME/);
    dialogBtn("Uninstall")!.click();
    await flushPromises();
    expect(commands.uninstallApp).toHaveBeenCalledWith("p1");
  });

  it("is hidden when nothing is installed", async () => {
    const { w } = await setup({ releases: [rel()] });
    expect(btn(w, "Uninstall from GNOME…")).toBeUndefined();
  });
});

describe("database safety (DR-7, DR-9, R10)", () => {
  const rolled = () => ({ state: installed("r2", "1.1.0"), previousReleaseId: "r1", previousVersion: "1.0.0" });
  const old = rel({ id: "r1", version: "1.0.0", createdAt: "2026-11-01T10:00:00Z", schemaVersion: 3 });
  const cur = rel({ id: "r2", version: "1.1.0", createdAt: "2026-11-02T10:00:00Z", schemaVersion: 4, lastActivatedAt: "2026-11-02T11:00:00Z" });
  const check = (over: object = {}) => ({ dataSchemaVersion: 4, targetSchemaVersion: 3, olderThanData: false, backup: null, ...over });

  beforeEach(() => {
    Object.assign(commands, { appdbRollbackCheck: vi.fn(() => ok(check())), appdbRestoreBackup: vi.fn(() => ok(null)) });
    commands.activateRelease!.mockImplementation(() => ok(rolled()));
  });

  it("captions a release whose schema differs from the active one's", async () => {
    const { w } = await setup({ releases: [old, cur], install: installed("r2", "1.1.0") });
    expect(w.find("[data-release='1.0.0']").text()).toContain("Database changes: schema 4 → 3");
    expect(w.find("[data-release='1.1.0']").text()).not.toContain("Database changes");
  });

  it("no caption when the schemas match or are unknown", async () => {
    const { w } = await setup({ releases: [{ ...old, schemaVersion: 4 }, cur, rel({ id: "r3", version: "0.9.0", createdAt: "2026-10-01T10:00:00Z" })], install: installed("r2", "1.1.0") });
    expect(w.text()).not.toContain("Database changes");
  });

  it("Activate with a schema change confirms and names the backup", async () => {
    const newer = rel({ id: "r3", version: "1.2.0", createdAt: "2026-11-03T10:00:00Z", schemaVersion: 5 });
    const { w } = await setup({ releases: [cur, newer], install: installed("r2", "1.1.0") });
    await btn(w, "Install 1.2.0")!.trigger("click");
    await flushPromises();
    expect(commands.activateRelease).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain("Database changes: schema 4 → 5.");
    expect(document.body.textContent).toContain("Studio backs up the app's data first (before 1.2.0).");
    dialogBtn("Install")!.click();
    await flushPromises();
    expect(commands.activateRelease).toHaveBeenCalledWith("p1", "r3");
  });

  it("without a schema change the switch needs no confirm", async () => {
    const newer = rel({ id: "r3", version: "1.2.0", createdAt: "2026-11-03T10:00:00Z", schemaVersion: 4 });
    const { w } = await setup({ releases: [cur, newer], install: installed("r2", "1.1.0") });
    await btn(w, "Install 1.2.0")!.trigger("click");
    await flushPromises();
    expect(commands.activateRelease).toHaveBeenCalledWith("p1", "r3");
  });

  it("rolling back to a release older than the data warns and can restore the backup", async () => {
    commands.appdbRollbackCheck!.mockImplementation(() => ok(check({ olderThanData: true, backup: { id: "bk1", label: "before 1.1.0" } })));
    const { w } = await setup({ releases: [old, cur], install: installed("r2", "1.1.0") });
    await w.find("[data-release='1.0.0']").findAll("button").find((b) => b.text() === "Roll back")!.trigger("click");
    await flushPromises();
    expect(commands.appdbRollbackCheck).toHaveBeenCalledWith("p1", "r1");
    expect(commands.activateRelease).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain("Version 1.0.0 is older than your data (schema 4 vs 3). It may not read it correctly.");
    expect(dialogBtn("Keep current data")).toBeTruthy();
    dialogBtn("Restore data from before 1.1.0")!.click();
    await flushPromises();
    expect(commands.activateRelease).toHaveBeenCalledWith("p1", "r1");
    expect(commands.appdbRestoreBackup).toHaveBeenCalledWith("p1", "bk1");
  });

  it("Keep current data switches without restoring; Cancel switches nothing", async () => {
    commands.appdbRollbackCheck!.mockImplementation(() => ok(check({ olderThanData: true, backup: { id: "bk1", label: "before 1.1.0" } })));
    const { w } = await setup({ releases: [old, cur], install: installed("r2", "1.1.0") });
    const roll = () => w.find("[data-release='1.0.0']").findAll("button").find((b) => b.text() === "Roll back")!;
    await roll().trigger("click");
    await flushPromises();
    dialogBtn("Cancel")!.click();
    await flushPromises();
    expect(commands.activateRelease).not.toHaveBeenCalled();
    await roll().trigger("click");
    await flushPromises();
    dialogBtn("Keep current data")!.click();
    await flushPromises();
    expect(commands.activateRelease).toHaveBeenCalledWith("p1", "r1");
    expect(commands.appdbRestoreBackup).not.toHaveBeenCalled();
  });

  it("a failed backup stops the switch: first line as the message, the reason under Details", async () => {
    commands.activateRelease!.mockImplementation(() => err("Couldn't back up Notes's data, so nothing was changed\ndisk full (os error 28)"));
    const { w } = await setup({ releases: [rel()] });
    await btn(w, "Install 1.0.0")!.trigger("click");
    await flushPromises();
    const c = w.find(".callout.danger");
    expect(c.find(".msg1").text()).toBe("Couldn't back up Notes's data, so nothing was changed");
    expect(c.find("details summary").text()).toBe("Details");
    expect(c.find("details pre").text()).toBe("disk full (os error 28)");
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

// --- F22: export for other systems ---

const now = () => new Date().toISOString();
const file = (name: string, kind: "appImage" | "deb" | "rpm" | "nsis", sizeBytes: number) => ({ name, kind, path: `/data/r1/exports/${name}`, sizeBytes, sha256: `sha-${name}` });
const linuxView = (over = {}) => ({
  target: "linux" as const, builtAt: now(), dir: "/data/r1/exports/linux", failed: null,
  files: [file("Notes_1.0.0_amd64.AppImage", "appImage", 101_000_000), file("notes_1.0.0_amd64.deb", "deb", 9_800_000), file("notes-1.0.0-1.x86_64.rpm", "rpm", 9_900_000)], ...over,
});
const winView = (over = {}) => ({ target: "windows" as const, builtAt: now(), dir: "/data/r1/exports/windows", failed: null, files: [file("Notes_1.0.0_x64-setup.exe", "nsis", 4_100_000)], ...over });
const exps = (...targets: Record<string, unknown>[]): ReleaseExports[] => [{ releaseId: "r1", dir: "/data/r1/exports", targets: targets as never }];
const job = (over: Partial<ExportJob> = {}): ExportJob => ({ releaseId: "r1", version: "1.0.0", targets: ["linux", "windows"], results: [], firstRun: false, ...over });
const exportStatus = (over: Partial<BuildStatus> = {}, j: Partial<ExportJob> = {}): BuildStatus => bst({ version: "1.0.0", phase: "exportLinux", export: job(j), ...over });

describe("export entry points (design 1)", () => {
  const r1 = rel({ lastActivatedAt: "2026-11-01T11:00:00Z" });
  const r2 = rel({ id: "r2", version: "1.1.0", createdAt: "2026-11-02T10:00:00Z" });
  const r3 = rel({ id: "r3", version: "0.9.0", createdAt: "2026-10-30T10:00:00Z", smoke: "failed", smokeDetail: "crash" });

  it("a visible Export… sits on the active row only", async () => {
    const { w } = await setup({ releases: [r1, r2], install: installed() });
    expect(w.find("tr[data-release='1.0.0']").findAll("button").some((b) => b.text() === "Export…")).toBe(true);
    expect(w.find("tr[data-release='1.1.0']").findAll("button").some((b) => b.text() === "Export…")).toBe(false);
    expect(w.findAll("button").filter((b) => b.text() === "Export…")).toHaveLength(1);
  });

  it("every kept row's ⋯ menu offers Export for other systems…", async () => {
    const { w } = await setup({ releases: [r1, r2, r3], install: installed() });
    const menu = (v: string) => w.find(`tr[data-release='${v}']`).findAll(".pop button").map((b) => b.text());
    expect(menu("1.0.0")).toContain("Export for other systems…");
    expect(menu("1.1.0")).toContain("Export for other systems…");
    expect(menu("0.9.0")).toEqual([]); // a release whose test launch failed has no ⋯ menu
  });

  it("Export… and the menu entry open the dialog for that release", async () => {
    const { w } = await setup({ releases: [r1, r2], install: installed() });
    await btn(w.find("tr[data-release='1.0.0']"), "Export…")!.trigger("click");
    await flushPromises();
    expect(document.querySelector("[role=dialog] h2")!.textContent).toBe("Export Notes 1.0.0");
    dialogBtn("Cancel")!.click();
    await flushPromises();
    await btn(w.find("tr[data-release='1.1.0']"), "Export for other systems…")!.trigger("click");
    await flushPromises();
    expect(document.querySelector("[role=dialog] h2")!.textContent).toBe("Export Notes 1.1.0");
  });

  it("the whole journey: Export… then Export shows Exporting in the status row", async () => {
    commands.startExport!.mockImplementation(() => ok(exportStatus({ phase: "installing" })));
    const { w } = await setup({ releases: [r1], install: installed() });
    await btn(w, "Export…")!.trigger("click");
    await flushPromises();
    dialogBtn("Export")!.click();
    await flushPromises();
    expect(commands.startExport).toHaveBeenCalledWith("p1", "r1", ["linux", "windows"], false);
    expect(w.text()).toContain("Exporting Notes 1.0.0 for Other Linux and Windows");
  });

  it("Export is off while a build runs or Claude is changing the project", async () => {
    const a = await setup({ releases: [r1], install: installed(), sessions: [session({ mode: "changes", jobState: "running" })] });
    expect(btn(a.w, "Export…")!.attributes("disabled")).toBeDefined();
    document.body.innerHTML = "";
    const b = await setup({ releases: [r1], install: installed(), build: bst() });
    expect(b.w.text()).toContain("Building 1.1.0");
    expect(btn(b.w, "Export…")!.attributes("disabled")).toBeDefined();
  });
});

describe("exports area (design 2, 9, 11, 13, 14)", () => {
  const r1 = rel();
  const open = async (o: Parameters<typeof setup>[0] = {}) => {
    const s = await setup({ releases: [r1], install: installed(), ...o });
    await s.w.find("button[aria-controls^='exports-']").trigger("click");
    return s;
  };

  it("has no toggle without exports", async () => {
    const { w } = await setup({ releases: [r1], install: installed() });
    expect(w.find("button[aria-controls^='exports-']").exists()).toBe(false);
  });

  it("a folded caption toggle 'Exports · 4 files' controls the area and keeps focus", async () => {
    const { w } = await setup({ releases: [r1], install: installed(), exports: exps(linuxView(), winView()) });
    const t = w.find("button[aria-controls^='exports-']");
    expect(t.text()).toBe("Exports · 4 files");
    expect(t.attributes("aria-expanded")).toBe("false");
    expect(w.find("tr.exp").exists()).toBe(false);
    (t.element as HTMLElement).focus();
    await t.trigger("click");
    expect(t.attributes("aria-expanded")).toBe("true");
    expect(document.activeElement).toBe(t.element);
    expect(document.getElementById(t.attributes("aria-controls")!)).toBeTruthy();
    await t.trigger("click");
    expect(w.find("tr.exp").exists()).toBe(false);
  });

  it("opens as a flat spanning row with one group per target, each a list under its heading", async () => {
    const { w } = await open({ exports: exps(linuxView(), winView()) });
    const row = w.find("tr.exp");
    expect(row.find("td").attributes("colspan")).toBe("6");
    const heads = row.findAll("h3").map((h) => h.text());
    expect(heads[0]).toMatch(/^Other Linux · built on the Ubuntu 22\.04 base · Today \d.* Unverified$/);
    expect(heads[1]).toMatch(/^Windows 11 · Today \d.* Unverified$/);
    expect(row.findAll("h3 .badge.warn")).toHaveLength(2);
    expect(row.text()).toContain("Not started by Studio.");
    expect(row.text()).toContain("Studio can't start Windows apps; try it on a Windows 11 PC.");
    expect(row.findAll("section ul")).toHaveLength(2);
    for (const sec of row.findAll("section")) expect(document.getElementById(sec.attributes("aria-labelledby")!)).toBeTruthy();
  });

  it("file lines: name with a full-name tooltip, purpose and size", async () => {
    const { w } = await open({ exports: exps(linuxView(), winView()) });
    const lines = w.findAll("tr.exp li").map((l) => l.text());
    expect(lines[0]).toContain("Notes_1.0.0_amd64.AppImage");
    expect(lines[0]).toContain("Runs on most distros (glibc 2.35+)");
    expect(lines[0]).toContain("96 MB");
    expect(lines[1]).toContain("Ubuntu, Debian, Mint");
    expect(lines[2]).toContain("Fedora, openSUSE");
    expect(lines[3]).toContain('Installer. Unsigned: Windows shows "More info → Run anyway".');
    expect(w.find("tr.exp li .fname").attributes("title")).toBe("Notes_1.0.0_amd64.AppImage");
    expect(w.find("tr.exp li .fsize").classes()).toContain("tnum");
  });

  it("Copy SHA-256 has a name per file, copies the checksum, says Copied and announces it", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const { w } = await open({ exports: exps(linuxView(), winView()) });
    const copies = w.findAll("tr.exp .copy");
    expect(copies.map((b) => b.attributes("aria-label"))).toEqual([
      "Copy SHA-256 of Notes_1.0.0_amd64.AppImage", "Copy SHA-256 of notes_1.0.0_amd64.deb", "Copy SHA-256 of notes-1.0.0-1.x86_64.rpm", "Copy SHA-256 of Notes_1.0.0_x64-setup.exe",
    ]);
    await copies[1]!.trigger("click");
    await flushPromises();
    expect(writeText).toHaveBeenCalledWith("sha-notes_1.0.0_amd64.deb");
    expect(copies[1]!.text()).toBe("Copied");
    expect(copies[0]!.text()).toBe("Copy SHA-256");
    expect(w.find("[aria-live='polite'].sr-only").text()).toBe("SHA-256 of notes_1.0.0_amd64.deb copied");
  });

  it("a failed target with no files shows '<Target>: export failed · <time>' and no group", async () => {
    const { w } = await open({ exports: exps(linuxView(), winView({ files: [], builtAt: null, failed: { at: now(), message: "link failed" } })) });
    const row = w.find("tr.exp");
    expect(row.text()).toMatch(/Windows: export failed · Today \d/);
    expect(row.findAll("h3")).toHaveLength(1);
  });

  it("a failed retry keeps the old files (one group with files, one failed line)", async () => {
    const { w } = await open({ exports: exps(winView({ failed: { at: now(), message: "x" } })) });
    expect(w.find("tr.exp").text()).toContain("Notes_1.0.0_x64-setup.exe");
    expect(w.find("tr.exp").text()).toMatch(/Windows: export failed/);
    expect(w.find("button[aria-controls^='exports-']").text()).toBe("Exports · 1 file");
  });

  it("an area with only a failure says so on the toggle", async () => {
    const { w } = await setup({ releases: [r1], install: installed(), exports: exps(winView({ files: [], builtAt: null, failed: { at: now(), message: "x" } })) });
    expect(w.find("button[aria-controls^='exports-']").text()).toBe("Exports · export failed");
  });

  it("Show in Files reveals the exports folder; Export again… opens the dialog noting the replace", async () => {
    const { w } = await open({ exports: exps(linuxView(), winView()) });
    await btn(w.find("tr.exp"), "Show in Files")!.trigger("click");
    expect(reveal).toHaveBeenCalledWith("/data/r1/exports");
    await btn(w.find("tr.exp"), "Export again…")!.trigger("click");
    await flushPromises();
    expect(document.querySelector("[role=dialog] h2")!.textContent).toBe("Export Notes 1.0.0");
    expect(document.body.textContent).toContain("Replaces the current Other Linux export when it finishes.");
    expect(document.body.textContent).toContain("Replaces the current Windows export when it finishes.");
  });
});

describe("export progress, stop and failure (design 4, 6, 7)", () => {
  const r1 = rel();
  const stepper = (w: ReturnType<typeof mount>) => w.findAll(".step").map((s) => `${s.find("span").text()}:${s.classes()[1]}`);
  const partial = (over: Partial<BuildStatus> = {}) =>
    exportStatus(
      { live: false, state: "failed", phase: null, errorKind: "buildFailed", errorMessage: "error: linking failed", ...over },
      { results: [{ target: "linux", ok: true, message: null }, { target: "windows", ok: false, message: "error: linking with `lld-link` failed" }] },
    );

  it("the status row names only the chosen targets, with elapsed time, the stepper, the log and Stop", async () => {
    const { w } = await setup({ releases: [r1], install: installed(), build: exportStatus({}, { targets: ["linux"] }), lines: ["Bundling AppImage"] });
    expect(w.find(".sentence").text()).toMatch(/^Exporting Notes 1\.0\.0 for Other Linux · \d+s$/);
    expect(w.find("ol").attributes("aria-label")).toBe("Export progress");
    expect(stepper(w)).toEqual(["Dependencies:done", "Linux:active"]);
    expect(w.text()).toContain("Bundling AppImage");
    expect(w.text()).not.toContain("opens for 5 seconds");
    await btn(w, "Stop")!.trigger("click");
    expect(commands.stopBuild).toHaveBeenCalledWith("p1");
  });

  it("both targets, and Environment only on the first export", async () => {
    const a = await setup({ releases: [r1], install: installed(), build: exportStatus({ phase: "exportWindows" }) });
    expect(a.w.find(".sentence").text()).toContain("for Other Linux and Windows");
    expect(stepper(a.w)).toEqual(["Dependencies:done", "Linux:done", "Windows:active"]);
    expect(a.w.text()).not.toContain("First export");
    document.body.innerHTML = "";
    const b = await setup({ releases: [r1], install: installed(), build: exportStatus({ phase: "environment" }, { firstRun: true }) });
    expect(stepper(b.w)).toEqual(["Environment:active", "Dependencies:pending", "Linux:pending", "Windows:pending"]);
    expect(b.w.text()).toContain("First export: downloading the build environment (about 3 GB). Later exports skip this step.");
  });

  it("progress is announced politely and Build new release… is disabled with a reason", async () => {
    const { w } = await setup({ releases: [r1], install: installed(), build: exportStatus() });
    expect(w.find(".sr-only[aria-live='polite']").text()).toBe("Exporting Notes 1.0.0 for Other Linux and Windows. Linux");
    const build = btn(w, "Build new release…")!;
    expect(build.attributes("disabled")).toBeDefined();
    expect(document.getElementById(build.attributes("aria-describedby")!)!.textContent).toBe("You can build when the export ends.");
  });

  it("a stopped export says so", async () => {
    const { w } = await setup({ releases: [r1], install: installed(), build: exportStatus({ live: false, state: "cancelled", phase: null, errorKind: "cancelled" }) });
    expect(w.text()).toContain("The last export was stopped.");
    expect(w.text()).not.toContain("The last build was stopped.");
  });

  it("a failed target gets a danger callout with its first error line, Details and Ask Claude to fix naming the target", async () => {
    const lines = ["Compiling app", "error: linking with `lld-link` failed", "  = note: LINK : fatal error LNK1181", "  = note: more"];
    const { w, nav } = await setup({ releases: [r1], install: installed(), build: partial(), lines });
    const c = w.findAll(".callout.danger");
    expect(c).toHaveLength(1);
    expect(c[0]!.attributes("role")).toBe("alert");
    expect(c[0]!.text()).toContain("Windows export failed");
    expect(c[0]!.text()).toContain("error: linking with `lld-link` failed");
    expect(c[0]!.find("details summary").text()).toBe("Details");
    await btn(w, "Ask Claude to fix")!.trigger("click");
    const text = nav.composerHandoff!.text;
    expect(text).toContain("Windows export of Notes 1.0.0 failed");
    expect(text).toContain("error: linking with `lld-link` failed");
    expect(text).toContain("LNK1181");
    expect(text).toContain("check the fix by exporting again");
    expect(nav.tab).toBe("chat");
  });

  it("each failed target has its own callout", async () => {
    const both = partial();
    both.export!.results = [{ target: "linux", ok: false, message: "a" }, { target: "windows", ok: false, message: "b" }];
    const { w } = await setup({ releases: [r1], install: installed(), build: both });
    const texts = w.findAll(".callout.danger").map((c) => c.text());
    expect(texts).toHaveLength(2);
    expect(texts[0]).toContain("Other Linux export failed");
    expect(texts[1]).toContain("Windows export failed");
  });

  it("a setup error (preflight) offers Open Doctor, not Ask Claude to fix", async () => {
    const pre = exportStatus({ live: false, state: "failed", phase: null, errorKind: "preflight", errorMessage: "Docker isn't running" });
    const { w, nav } = await setup({ releases: [r1], install: installed(), build: pre });
    expect(w.find(".callout.danger").text()).toContain("Export failed");
    expect(w.find(".callout.danger").text()).toContain("Docker isn't running");
    expect(btn(w, "Ask Claude to fix")).toBeUndefined();
    await btn(w, "Open Doctor")!.trigger("click");
    expect(nav.view).toBe("doctor");
  });

  it("an export failure does not use the build callout", async () => {
    const { w } = await setup({ releases: [r1], install: installed(), build: partial() });
    expect(w.text()).not.toContain("GNOME still starts");
  });

  it("when the export ends: files load, the area opens by itself and a toast offers Show in Files", async () => {
    const { w, toast } = await setup({ releases: [r1], install: installed(), build: exportStatus() });
    expect(w.find("tr.exp").exists()).toBe(false);
    commands.listReleaseExports!.mockImplementation(() => ok(exps(linuxView(), winView())));
    const done = exportStatus({ live: false, state: "succeeded", phase: null }, { results: [{ target: "linux", ok: true, message: null }, { target: "windows", ok: true, message: null }] });
    ev.status!({ payload: { status: done } });
    await flushPromises();
    expect(toast.current!.text).toBe("Notes 1.0.0 exported for Other Linux and Windows");
    expect(toast.current!.action!.label).toBe("Show in Files");
    toast.current!.action!.run();
    expect(reveal).toHaveBeenCalledWith("/data/r1/exports");
    expect(w.find("button[aria-controls^='exports-']").attributes("aria-expanded")).toBe("true");
    expect(w.find("tr.exp").text()).toContain("Notes_1.0.0_x64-setup.exe");
    expect(w.find(".sentence").text()).toContain("GNOME starts Notes 1.0.0"); // back to the idle row
  });

  it("a partly failed export toasts only the targets that worked; a stopped one stays quiet", async () => {
    const a = await setup({ releases: [r1], install: installed(), build: exportStatus() });
    ev.status!({ payload: { status: partial() } });
    await flushPromises();
    expect(a.toast.current!.text).toBe("Notes 1.0.0 exported for Other Linux");
    a.toast.dismiss();
    document.body.innerHTML = "";
    const b = await setup({ releases: [r1], install: installed(), build: exportStatus() });
    ev.status!({ payload: { status: exportStatus({ live: false, state: "cancelled", phase: null }) } });
    await flushPromises();
    expect(b.toast.current).toBeNull();
  });
});
