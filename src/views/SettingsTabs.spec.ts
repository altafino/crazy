import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../bindings", () => ({ commands }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { ok } from "../test-helpers";
import { SETTINGS_TABS, useNavStore } from "../stores/nav";
import SettingsView from "./SettingsView.vue";

async function setup(section: (typeof SETTINGS_TABS)[number]["id"]) {
  Object.assign(commands, {
    exportCacheSize: vi.fn(() => ok({ bytes: 0 })),
    exportReadiness: vi.fn(() => ok({ docker: "ready", dockerVersion: null, detail: null, imagePresent: true, licenseAcceptedAt: null, lastTargets: [] })),
    getSettings: vi.fn(() => ok({ defaultProjectsDir: "/p", toolOverrides: {} })),
    runDoctor: vi.fn(() => ok({ shell: { status: { state: "ok" } }, tools: [], graphics: {}, warnings: [], checkedAt: 0 })),
    getPreviewSettings: vi.fn(() => ok({ maxLines: 8000, maxLogMb: 20, keepLogs: 7 })),
    getAttachmentLimits: vi.fn(() => ok({ maxFiles: 10, maxImageMb: 20, maxPdfMb: 32, maxTextMb: 2, maxDocxMb: 20, maxDocxConvertedMb: 2, maxTotalMb: 64, pandocTimeoutSecs: 60 })),
    getDatabaseLimits: vi.fn(() => ok({ queryTimeLimitSecs: 2 })),
    getApprovalSettings: vi.fn(() => ok({ autoApprove: false, askForRisky: true })),
    listAgentProfiles: vi.fn(() => ok([])),
    proposeAgentProfile: vi.fn(() => ok(null)),
  });
  const pinia = createPinia();
  setActivePinia(pinia);
  useNavStore().settingsSection = section;
  const w = mount(SettingsView, { global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return w;
}

beforeEach(() => {
  document.body.innerHTML = "";
});

describe("Settings tabs (one SETTINGS_TABS list)", () => {
  it("lists the five tabs in order", () => {
    expect(SETTINGS_TABS.map((t) => t.label)).toEqual(["General", "AI Agents", "Approvals", "Limits", "Tools"]);
  });

  for (const t of SETTINGS_TABS) {
    it(`deep link to ${t.id} opens its tab and panel`, async () => {
      const w = await setup(t.id);
      expect(w.findAll("[role=tab]").map((b) => b.text())).toEqual(SETTINGS_TABS.map((x) => x.label));
      expect(w.find(`#set-tab-${t.id}`).attributes("aria-selected")).toBe("true");
      const panel = w.find("[role=tabpanel]");
      expect(panel.attributes("aria-labelledby")).toBe(`set-tab-${t.id}`);
    });
  }

  it("Limits shows its three cards and Tools shows the tool paths", async () => {
    const limits = await setup("limits");
    for (const h of ["Preview", "Attachments", "Database tab"]) expect(limits.text()).toContain(h);
    expect(limits.text()).not.toContain("Default projects folder");
    document.body.innerHTML = "";
    const tools = await setup("tools");
    expect(tools.text()).toContain("Tool paths");
    expect(tools.text()).not.toContain("Notifications");
  });

  it("arrow keys, Home and End move between tabs", async () => {
    const w = await setup("general");
    const key = async (k: string) => {
      await w.find("[role=tablist]").trigger("keydown", { key: k });
      await flushPromises();
    };
    await key("ArrowRight");
    expect(w.find("#set-tab-agents").attributes("aria-selected")).toBe("true");
    await key("End");
    expect(w.find("#set-tab-tools").attributes("aria-selected")).toBe("true");
    await key("ArrowRight");
    expect(w.find("#set-tab-general").attributes("aria-selected")).toBe("true");
    await key("ArrowLeft");
    expect(w.find("#set-tab-tools").attributes("aria-selected")).toBe("true");
    await key("Home");
    expect(w.find("#set-tab-general").attributes("aria-selected")).toBe("true");
  });
});
