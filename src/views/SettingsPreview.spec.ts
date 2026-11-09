import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../bindings", () => ({ commands }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { ok } from "../test-helpers";
import { useNavStore } from "../stores/nav";
import { useToastStore } from "../stores/toast";
import SettingsView from "./SettingsView.vue";

const ATTACH = { maxFiles: 10, maxImageMb: 20, maxPdfMb: 32, maxTextMb: 2, maxDocxMb: 20, maxDocxConvertedMb: 2, maxTotalMb: 64, pandocTimeoutSecs: 60 };

async function setup() {
  commands.exportCacheSize = vi.fn(() => ok({ bytes: 0 }));
  commands.exportReadiness = vi.fn(() => ok({ docker: "ready", dockerVersion: null, detail: null, imagePresent: true, licenseAcceptedAt: null, lastTargets: [] }));
  commands.getSettings = vi.fn(() => ok({ defaultProjectsDir: "/home/u/Projects", toolOverrides: {} }));
  commands.runDoctor = vi.fn(() => ok({ shell: { status: { state: "ok" } }, tools: [], graphics: {}, warnings: [], checkedAt: 0 }));
  commands.getPreviewSettings = vi.fn(() => ok({ maxLines: 8000, maxLogMb: 20, keepLogs: 7 }));
  commands.savePreviewSettings = vi.fn((l) => ok(l));
  commands.getAttachmentLimits = vi.fn(() => ok(ATTACH));
  commands.setAttachmentLimits = vi.fn((l) => ok(l));
  commands.getDatabaseLimits = vi.fn(() => ok({ queryTimeLimitSecs: 2 }));
  commands.setDatabaseLimits = vi.fn((l) => ok(l));
  const pinia = createPinia();
  setActivePinia(pinia);
  useNavStore().settingsSection = "limits";
  const w = mount(SettingsView, { global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return w;
}
const btn = (w: ReturnType<typeof mount>, label: string) => w.findAll("button").find((b) => b.text() === label)!;

beforeEach(() => {
  document.body.innerHTML = "";
});

describe("Settings › Limits › Preview (M3-D49)", () => {
  it("loads the three limits with their ranges", async () => {
    const w = await setup();
    expect((w.find("#pv-maxLines").element as HTMLInputElement).value).toBe("8000");
    expect(w.text()).toContain("1,000 to 50,000");
    expect(w.text()).toContain("1 to 100");
    expect(w.text()).toContain("1 to 50");
  });

  it("shows field errors and does not save out-of-range values", async () => {
    const w = await setup();
    await w.find("#pv-maxLines").setValue(500);
    await w.find("#pv-maxLines").trigger("blur");
    await w.find("#pv-keepLogs").setValue(51);
    await w.find("#pv-keepLogs").trigger("blur");
    expect(commands.savePreviewSettings).not.toHaveBeenCalled();
    expect(w.findAll(".field-error")).toHaveLength(2);
    expect(w.find("#pv-maxLines").attributes("aria-invalid")).toBe("true");
  });

  it("saves valid limits and Reset to defaults applies 5000 / 10 / 5", async () => {
    const w = await setup();
    await w.find("#pv-maxLines").setValue(2000);
    await w.find("#pv-maxLines").trigger("blur");
    await flushPromises();
    expect(commands.savePreviewSettings).toHaveBeenLastCalledWith({ maxLines: 2000, maxLogMb: 20, keepLogs: 7 });
    expect(w.text()).toContain("Saved");
    await btn(w, "Reset to defaults").trigger("click");
    await flushPromises();
    expect(commands.savePreviewSettings).toHaveBeenLastCalledWith({ maxLines: 5000, maxLogMb: 10, keepLogs: 5 });
    expect((w.find("#pv-maxLines").element as HTMLInputElement).value).toBe("5000");
  });
});

describe("Settings › Limits › Database tab (R9)", () => {
  it("loads the 2 s default with the '0 = no limit' hint", async () => {
    const w = await setup();
    expect(w.text()).toContain("Database tab");
    expect((w.find("#db-limit").element as HTMLInputElement).value).toBe("2");
    expect(w.find("#db-limit-h").text()).toBe("0 to 60; 0 = no limit");
  });

  it("refuses values outside 0-60 inline and saves valid ones, including 0", async () => {
    const w = await setup();
    await w.find("#db-limit").setValue(61);
    await w.find("#db-limit").trigger("blur");
    expect(commands.setDatabaseLimits).not.toHaveBeenCalled();
    expect(w.find("#db-limit").attributes("aria-invalid")).toBe("true");
    await w.find("#db-limit").setValue(0);
    await w.find("#db-limit").trigger("keydown.enter");
    await flushPromises();
    expect(commands.setDatabaseLimits).toHaveBeenLastCalledWith({ queryTimeLimitSecs: 0 });
    expect(w.text()).toContain("Saved");
    await btn(w, "Reset to default").trigger("click");
    await flushPromises();
    expect(commands.setDatabaseLimits).toHaveBeenLastCalledWith({ queryTimeLimitSecs: 2 });
    expect((w.find("#db-limit").element as HTMLInputElement).value).toBe("2");
  });
});

describe("Settings › Limits › Attachments", () => {
  it("loads the limits with their ranges and refuses invalid values inline", async () => {
    const w = await setup();
    expect((w.find("#at-maxImageMb").element as HTMLInputElement).value).toBe("20");
    expect(w.text()).toContain("10 to 600");
    expect(w.text()).toContain("1 to 1,024");
    await w.find("#at-maxFiles").setValue(51);
    await w.find("#at-maxFiles").trigger("blur");
    expect(commands.setAttachmentLimits).not.toHaveBeenCalled();
    expect(w.find("#at-maxFiles").attributes("aria-invalid")).toBe("true");
    expect(w.findAll(".field-error")).toHaveLength(1);
  });

  it("saves a change and Reset to defaults applies the defaults", async () => {
    const w = await setup();
    await w.find("#at-maxFiles").setValue(25);
    await w.find("#at-maxFiles").trigger("blur");
    await flushPromises();
    expect(commands.setAttachmentLimits).toHaveBeenCalledWith({ ...ATTACH, maxFiles: 25 });
    expect(w.text()).toContain("Saved");
    await w.findAll("button").filter((b) => b.text() === "Reset to defaults").slice(-1)[0]!.trigger("click");
    expect(commands.setAttachmentLimits).toHaveBeenLastCalledWith(ATTACH);
  });
});

describe("Settings › Limits › Reset to defaults with Undo (D5)", () => {
  it("Preview: Reset saves the defaults and the toast's Undo saves the previous values", async () => {
    const w = await setup();
    await btn(w, "Reset to defaults").trigger("click");
    await flushPromises();
    expect(commands.savePreviewSettings).toHaveBeenLastCalledWith({ maxLines: 5000, maxLogMb: 10, keepLogs: 5 });
    const toast = useToastStore().current!;
    expect(toast.text).toBe("Preview limits reset to defaults");
    expect(toast.action!.label).toBe("Undo");
    toast.action!.run();
    await flushPromises();
    expect(commands.savePreviewSettings).toHaveBeenLastCalledWith({ maxLines: 8000, maxLogMb: 20, keepLogs: 7 });
    expect((w.find("#pv-maxLines").element as HTMLInputElement).value).toBe("8000");
  });

  it("Database: Reset to default, then Undo restores the earlier limit", async () => {
    const w = await setup();
    await w.find("#db-limit").setValue(30);
    await w.find("#db-limit").trigger("blur");
    await flushPromises();
    await btn(w, "Reset to default").trigger("click");
    await flushPromises();
    expect(commands.setDatabaseLimits).toHaveBeenLastCalledWith({ queryTimeLimitSecs: 2 });
    expect(useToastStore().current!.text).toBe("Database limits reset to defaults");
    useToastStore().current!.action!.run();
    await flushPromises();
    expect(commands.setDatabaseLimits).toHaveBeenLastCalledWith({ queryTimeLimitSecs: 30 });
    expect((w.find("#db-limit").element as HTMLInputElement).value).toBe("30");
  });

  it("Attachments: Undo restores the previous values", async () => {
    const w = await setup();
    await w.find("#at-maxFiles").setValue(25);
    await w.find("#at-maxFiles").trigger("blur");
    await flushPromises();
    await w.findAll("button").filter((b) => b.text() === "Reset to defaults")[1]!.trigger("click");
    await flushPromises();
    expect(commands.setAttachmentLimits).toHaveBeenLastCalledWith(ATTACH);
    expect(useToastStore().current!.text).toBe("Attachment limits reset to defaults");
    useToastStore().current!.action!.run();
    await flushPromises();
    expect(commands.setAttachmentLimits).toHaveBeenLastCalledWith({ ...ATTACH, maxFiles: 25 });
  });

  it("does not offer Undo when the reset itself failed", async () => {
    const w = await setup();
    commands.savePreviewSettings = vi.fn(() => Promise.resolve({ status: "error" as const, error: { message: "disk full", fieldErrors: [] } }));
    await btn(w, "Reset to defaults").trigger("click");
    await flushPromises();
    expect(useToastStore().current).toBeNull();
    expect(w.text()).toContain("Couldn't save Preview settings");
  });
});
