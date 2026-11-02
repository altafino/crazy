import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../bindings", () => ({ commands }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { ok } from "../test-helpers";
import { useNavStore } from "../stores/nav";
import SettingsView from "./SettingsView.vue";

async function setup() {
  commands.getSettings = vi.fn(() => ok({ defaultProjectsDir: "/home/u/Projects", toolOverrides: {} }));
  commands.runDoctor = vi.fn(() => ok({ shell: { status: { state: "ok" } }, tools: [], graphics: {}, warnings: [], checkedAt: 0 }));
  commands.getPreviewSettings = vi.fn(() => ok({ maxLines: 8000, maxLogMb: 20, keepLogs: 7 }));
  commands.savePreviewSettings = vi.fn((l) => ok(l));
  const pinia = createPinia();
  setActivePinia(pinia);
  useNavStore().settingsSection = "general";
  const w = mount(SettingsView, { global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return w;
}
const btn = (w: ReturnType<typeof mount>, label: string) => w.findAll("button").find((b) => b.text() === label)!;

beforeEach(() => {
  document.body.innerHTML = "";
});

describe("Settings › General › Preview (M3-D49)", () => {
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
    await w.find("#pv-keepLogs").setValue(51);
    await btn(w, "Save preview settings").trigger("click");
    expect(commands.savePreviewSettings).not.toHaveBeenCalled();
    expect(w.findAll(".field-error")).toHaveLength(2);
    expect(w.find("#pv-maxLines").attributes("aria-invalid")).toBe("true");
  });

  it("saves valid limits and Reset to defaults applies 5000 / 10 / 5", async () => {
    const w = await setup();
    await w.find("#pv-maxLines").setValue(2000);
    await btn(w, "Save preview settings").trigger("click");
    await flushPromises();
    expect(commands.savePreviewSettings).toHaveBeenLastCalledWith({ maxLines: 2000, maxLogMb: 20, keepLogs: 7 });
    expect(w.text()).toContain("Preview settings saved.");
    await btn(w, "Reset to defaults").trigger("click");
    await flushPromises();
    expect(commands.savePreviewSettings).toHaveBeenLastCalledWith({ maxLines: 5000, maxLogMb: 10, keepLogs: 5 });
    expect((w.find("#pv-maxLines").element as HTMLInputElement).value).toBe("5000");
  });
});
