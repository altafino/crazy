import { describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../bindings", () => ({ commands }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { ok } from "../test-helpers";
import SettingsView from "./SettingsView.vue";

describe("Settings > Approvals", () => {
  it("is its own tab, saves each switch at once and warns when risky actions stop asking", async () => {
    commands.getSettings = vi.fn(() => ok({ defaultProjectsDir: "/p", toolOverrides: {} }));
    commands.runDoctor = vi.fn(() => ok({ shell: { status: { state: "ok" } }, tools: [], graphics: {}, warnings: [], checkedAt: 0 }));
    commands.getPreviewSettings = vi.fn(() => ok({ maxLines: 8000, maxLogMb: 20, keepLogs: 7 }));
    commands.getAttachmentLimits = vi.fn(() => ok({}));
    commands.getDatabaseLimits = vi.fn(() => ok({ queryTimeLimitSecs: 2 }));
    commands.getApprovalSettings = vi.fn(() => ok({ autoApprove: false, askForRisky: true }));
    commands.setApprovalSettings = vi.fn((a) => ok(a));
    const pinia = createPinia();
    setActivePinia(pinia);
    const w = mount(SettingsView, { global: { plugins: [pinia] }, attachTo: document.body });
    await flushPromises();

    await w.find("#set-tab-approvals").trigger("click");
    await flushPromises();
    const [auto, risky] = w.findAll<HTMLInputElement>("#set-panel-approvals input[type=checkbox]");
    expect(auto!.element.checked).toBe(false);
    expect(risky!.element.checked).toBe(true);
    expect(w.text()).toContain("git push");

    await auto!.setValue(true);
    await flushPromises();
    expect(commands.setApprovalSettings).toHaveBeenLastCalledWith({ autoApprove: true, askForRisky: true });
    expect(w.text()).not.toContain("runs with your user's rights");

    await risky!.setValue(false);
    await flushPromises();
    expect(commands.setApprovalSettings).toHaveBeenLastCalledWith({ autoApprove: true, askForRisky: false });
    expect(w.text()).toContain("runs with your user's rights");
    expect(w.text()).toContain("Approval settings saved.");
  });
});
