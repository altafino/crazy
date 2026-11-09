import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../bindings", () => ({ commands }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { err, ok } from "../test-helpers";
import { useNavStore } from "../stores/nav";
import { useSettingsStore } from "../stores/settings";
import SettingsView from "./SettingsView.vue";
import { notifyFinishOn } from "../lib/notify";

async function setup() {
  Object.assign(commands, {
    exportCacheSize: vi.fn(() => ok({ bytes: 0 })),
    exportReadiness: vi.fn(() => ok({ docker: "ready", dockerVersion: null, detail: null, imagePresent: true, licenseAcceptedAt: null, lastTargets: [] })),
    getUiState: vi.fn(() => ok({ notifyFinish: true, lastSeenVersion: null })),
    setNotifyFinish: vi.fn(() => ok(null)),
    getSettings: vi.fn(() => ok({ defaultProjectsDir: "/home/u/Projects", toolOverrides: {} })),
    saveSettings: vi.fn((s) => ok(s)),
    runDoctor: vi.fn(() => ok({ shell: { status: { state: "ok" } }, tools: [], graphics: {}, warnings: [], checkedAt: 0 })),
  });
  localStorage.clear();
  const pinia = createPinia();
  setActivePinia(pinia);
  useNavStore().settingsSection = "general";
  const w = mount(SettingsView, { global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return { w, settings: useSettingsStore() };
}

beforeEach(() => {
  document.body.innerHTML = "";
});

describe("Settings › General saves by itself", () => {
  it("has no Save button and no 'Settings saved.' line", async () => {
    const { w } = await setup();
    expect(w.findAll("button").map((b) => b.text())).not.toContain("Save");
    expect(w.text()).not.toContain("Settings saved.");
  });

  it("the projects folder saves on blur through the settings store, and the wizard's copy follows", async () => {
    const { w, settings } = await setup();
    await w.find("#set-dir").setValue(" /data/apps ");
    await w.find("#set-dir").trigger("blur");
    await flushPromises();
    expect(commands.saveSettings).toHaveBeenCalledWith({ defaultProjectsDir: "/data/apps", toolOverrides: {} });
    expect(settings.settings?.defaultProjectsDir).toBe("/data/apps");
    expect(w.text()).toContain("Saved");
  });

  it("Enter saves too, and Esc puts the saved folder back", async () => {
    const { w } = await setup();
    await w.find("#set-dir").setValue("/a");
    await w.find("#set-dir").trigger("keydown", { key: "Escape" });
    expect((w.find("#set-dir").element as HTMLInputElement).value).toBe("/home/u/Projects");
    await w.find("#set-dir").setValue("/b");
    await w.find("#set-dir").trigger("keydown.enter");
    await flushPromises();
    expect(commands.saveSettings).toHaveBeenCalledTimes(1);
    expect(commands.saveSettings).toHaveBeenCalledWith({ defaultProjectsDir: "/b", toolOverrides: {} });
  });

  it("a backend field error shows under the folder field", async () => {
    const { w } = await setup();
    commands.saveSettings!.mockImplementation(() => err("Invalid settings", [{ field: "defaultProjectsDir", message: "Use an absolute path" }]));
    await w.find("#set-dir").setValue("rel/dir");
    await w.find("#set-dir").trigger("blur");
    await flushPromises();
    expect(w.find("#set-dir-err").text()).toBe("Use an absolute path");
    expect(w.find("#set-dir").attributes("aria-invalid")).toBe("true");
  });

  it("Notifications saves on change", async () => {
    const { w } = await setup();
    const box = w.find<HTMLInputElement>("input[type=checkbox]");
    expect(box.element.checked).toBe(true);
    await box.setValue(false);
    await flushPromises();
    expect(commands.setNotifyFinish).toHaveBeenCalledWith(false);
    expect(notifyFinishOn()).toBe(false);
  });
});
