import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import type { ExportReadiness } from "../../bindings";

const { commands, openUrl } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>>, openUrl: vi.fn(() => Promise.resolve()) }));
vi.mock("../../bindings", () => ({
  commands,
  events: { buildStatusEvent: { listen: vi.fn(async () => () => {}) }, buildLogEvent: { listen: vi.fn(async () => () => {}) } },
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl, revealItemInDir: vi.fn() }));

import { err, ok } from "../../test-helpers";
import { useNavStore } from "../../stores/nav";
import ExportDialog from "./ExportDialog.vue";

const ready = (over: Partial<ExportReadiness> = {}): ExportReadiness => ({
  docker: "ready", dockerVersion: "29.8.2", detail: null, imagePresent: true, licenseAcceptedAt: "2026-11-01T10:00:00Z", lastTargets: ["linux", "windows"], ...over,
});
const job = { jobId: "e1", projectId: "p1", version: "1.2.0", state: "running", phase: "environment", startedAt: null, completedAt: null, errorKind: null, errorMessage: null, releaseId: null, live: true, hasLog: true,
  export: { releaseId: "r1", version: "1.2.0", targets: ["linux", "windows"], results: [], firstRun: false } };

async function setup(readiness: ExportReadiness | null = ready(), existing: ("linux" | "windows")[] = []) {
  commands.exportReadiness!.mockImplementation(() => (readiness ? ok(readiness) : err("Probe failed")));
  const pinia = createPinia();
  setActivePinia(pinia);
  const w = mount(ExportDialog, { props: { projectId: "p1", releaseId: "r1", appName: "Pacman", version: "1.2.0", existing }, global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return { w, nav: useNavStore() };
}
const box = (label: string) =>
  [...document.querySelectorAll<HTMLInputElement>("input[type=checkbox]")].find((i) => i.closest("label")?.textContent?.trim().startsWith(label))!;
const button = (text: string) => [...document.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent?.trim() === text)!;
const describedBy = (el: HTMLElement) => (el.getAttribute("aria-describedby") ?? "").split(" ").map((id) => document.getElementById(id)?.textContent?.trim());

beforeEach(() => {
  document.body.innerHTML = "";
  openUrl.mockClear();
  Object.assign(commands, { exportReadiness: vi.fn(), startExport: vi.fn(() => ok(job)) });
});
afterEach(() => vi.useRealTimers());

describe("export dialog", () => {
  it("is titled with the app and version, focus starts on the first checkbox, targets say what they make", async () => {
    await setup();
    expect(document.querySelector("[role=dialog] h2")!.textContent).toBe("Export Pacman 1.2.0");
    expect(document.activeElement).toBe(box("Other Linux"));
    expect(describedBy(box("Other Linux"))).toEqual(["AppImage, .deb, .rpm for Ubuntu 22.04 and newer, Debian 12, Fedora, …"]);
    expect(describedBy(box("Windows 11"))).toEqual(["setup.exe installer (not tested by Studio, unsigned)"]);
  });

  it("ticks both on first use and what was ticked last time afterwards", async () => {
    await setup(ready({ lastTargets: ["linux", "windows"] }));
    expect([box("Other Linux").checked, box("Windows 11").checked]).toEqual([true, true]);
    document.body.innerHTML = "";
    await setup(ready({ lastTargets: ["windows"] }));
    expect([box("Other Linux").checked, box("Windows 11").checked]).toEqual([false, true]);
  });

  it("Export starts the chosen targets and closes", async () => {
    const { w } = await setup(ready({ lastTargets: ["linux"] }));
    button("Export").click();
    await flushPromises();
    expect(commands.startExport).toHaveBeenCalledWith("p1", "r1", ["linux"], false);
    expect(w.emitted("close")).toBeTruthy();
  });

  it("is disabled without a target", async () => {
    await setup(ready({ lastTargets: ["linux"] }));
    box("Other Linux").click();
    await flushPromises();
    expect(button("Export").disabled).toBe(true);
  });

  it("says a target with files is replaced when the export finishes, tied to its checkbox", async () => {
    await setup(ready(), ["windows"]);
    expect(document.body.textContent).toContain("Replaces the current Windows export when it finishes.");
    expect(describedBy(box("Windows 11"))).toContain("Replaces the current Windows export when it finishes.");
    expect(document.body.textContent).not.toContain("Replaces the current Other Linux");
  });

  it("shows startExport's refusal in the dialog and stays open", async () => {
    commands.startExport!.mockImplementation(() => err("An export is already running"));
    const { w } = await setup();
    button("Export").click();
    await flushPromises();
    expect(document.querySelector("[role=alert]")!.textContent).toContain("An export is already running");
    expect(w.emitted("close")).toBeUndefined();
  });
});

describe("license", () => {
  const LICENSE = "I accept Microsoft's license for the Windows build tools (Visual C++ runtime and Windows SDK)";
  it("shows an unticked required checkbox while Windows is ticked and not accepted; Export waits for it", async () => {
    await setup(ready({ licenseAcceptedAt: null }));
    const lic = box("I accept Microsoft's license");
    expect(lic.closest("label")!.textContent!.trim()).toBe(LICENSE);
    expect(lic.checked).toBe(false);
    expect(lic.required).toBe(true);
    expect(button("Export").disabled).toBe(true);
    lic.click();
    await flushPromises();
    expect(button("Export").disabled).toBe(false);
    button("Export").click();
    await flushPromises();
    expect(commands.startExport).toHaveBeenCalledWith("p1", "r1", ["linux", "windows"], true);
  });

  it("is absent without Windows and after acceptance", async () => {
    await setup(ready({ licenseAcceptedAt: null, lastTargets: ["linux"] }));
    expect(box("I accept Microsoft's license")).toBeUndefined();
    document.body.innerHTML = "";
    await setup(ready());
    expect(box("I accept Microsoft's license")).toBeUndefined();
    expect(button("Export").disabled).toBe(false);
  });

  it("links to the license and opens it externally", async () => {
    await setup(ready({ licenseAcceptedAt: null }));
    const a = [...document.querySelectorAll("a")].find((x) => x.textContent === "Read the license")!;
    a.click();
    expect(openUrl).toHaveBeenCalledWith("https://go.microsoft.com/fwlink/?LinkId=2086102");
  });
});

describe("first run and Docker", () => {
  it("explains the first export downloads about 3 GB", async () => {
    await setup(ready({ imagePresent: false }));
    expect(document.body.textContent).toContain("First export: downloads the build environment (about 3 GB) and takes several minutes.");
    document.body.innerHTML = "";
    await setup(ready({ imagePresent: true }));
    expect(document.body.textContent).not.toContain("First export");
  });

  it.each([
    ["notRunning", "Docker isn't running. Start it with: systemctl start docker"],
    ["noPermission", "Docker is installed, but your user can't use it. Add yourself to the docker group, then log out and back in."],
    ["missing", "Exports need Docker. Install the docker package."],
  ] as const)("%s: a warn alert with the fix, Open Doctor, Export disabled", async (docker, sentence) => {
    const { w, nav } = await setup(ready({ docker, detail: "raw reason" }));
    const alert = document.querySelector(".callout.warn[role=alert]")!;
    expect(alert.textContent!.replace(/\s+/g, " ")).toContain(sentence);
    expect(button("Export").disabled).toBe(true);
    button("Open Doctor").click();
    expect(nav.view).toBe("doctor");
    expect(w.emitted("close")).toBeTruthy();
  });

  it("shows the spinner only after 400 ms of checking, and Export waits for the answer", async () => {
    vi.useFakeTimers();
    commands.exportReadiness!.mockImplementation(() => new Promise(() => {}));
    const pinia = createPinia();
    setActivePinia(pinia);
    mount(ExportDialog, { props: { projectId: "p1", releaseId: "r1", appName: "Pacman", version: "1.2.0", existing: [] }, global: { plugins: [pinia] }, attachTo: document.body });
    await vi.advanceTimersByTimeAsync(300);
    expect(document.body.textContent).not.toContain("Checking Docker");
    await vi.advanceTimersByTimeAsync(200);
    expect(document.body.textContent).toContain("Checking Docker…");
    expect(button("Export").disabled).toBe(true);
  });

  it("a failed probe is reported and keeps Export off", async () => {
    await setup(null);
    expect(document.querySelector("[role=alert]")!.textContent).toContain("Probe failed");
    expect(button("Export").disabled).toBe(true);
  });
});
