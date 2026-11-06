import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../../bindings", () => ({ commands }));

import { err, ok } from "../../test-helpers";
import { useToastStore } from "../../stores/toast";
import ExportSettings from "./ExportSettings.vue";

const readiness = (licenseAcceptedAt: string | null) => ok({ docker: "ready", dockerVersion: "29", detail: null, imagePresent: true, licenseAcceptedAt, lastTargets: [] });

async function setup(o: { bytes?: number | null; sizeError?: string; accepted?: string | null } = {}) {
  commands.exportCacheSize!.mockImplementation(() => (o.sizeError ? err(o.sizeError) : ok({ bytes: o.bytes ?? 0 })));
  commands.exportReadiness!.mockImplementation(() => readiness(o.accepted ?? null));
  const pinia = createPinia();
  setActivePinia(pinia);
  const w = mount(ExportSettings, { global: { plugins: [pinia] }, attachTo: document.body });
  return { w, toast: useToastStore() };
}
const btn = (w: ReturnType<typeof mount>, text: string) => w.findAll("button").find((b) => b.text() === text);
const dialogBtn = (text: string) => [...document.querySelectorAll<HTMLButtonElement>("[role=dialog] button")].find((b) => b.textContent?.trim() === text);

beforeEach(() => {
  document.body.innerHTML = "";
  Object.assign(commands, { exportCacheSize: vi.fn(), exportReadiness: vi.fn(), clearExportCaches: vi.fn(() => ok(null)), withdrawExportLicense: vi.fn(() => ok(null)) });
});

describe("Settings > General > Exports card (design 12)", () => {
  it("says Calculating… while loading, then the size", async () => {
    const { w } = await setup({ bytes: 3.2 * 1024 ** 3 });
    expect(w.text()).toContain("Calculating…");
    expect(btn(w, "Clear export caches…")!.attributes("disabled")).toBeDefined();
    await flushPromises();
    expect(w.find("h2").text()).toBe("Exports");
    expect(w.text()).toContain("Export caches use 3.2 GB");
    expect(w.text()).not.toContain("Calculating…");
  });

  it("says No export caches at 0 and offers nothing to clear", async () => {
    const { w } = await setup({ bytes: 0 });
    await flushPromises();
    expect(w.text()).toContain("No export caches");
    expect(btn(w, "Clear export caches…")!.attributes("disabled")).toBeDefined();
  });

  it("a read error says Couldn't read Docker volumes with the reason under Details", async () => {
    const { w } = await setup({ sizeError: "permission denied on /var/run/docker.sock" });
    await flushPromises();
    expect(w.text()).toContain("Couldn't read Docker volumes");
    expect(w.find("details summary").text()).toBe("Details");
    expect(w.find("details pre").text()).toContain("permission denied");
  });

  it("Clear export caches… asks, names the size on the button, then clears and reloads", async () => {
    const { w, toast } = await setup({ bytes: 3.2 * 1024 ** 3 });
    await flushPromises();
    await btn(w, "Clear export caches…")!.trigger("click");
    expect(document.querySelector("[role=dialog] h2")!.textContent).toBe("Clear export caches?");
    expect(commands.clearExportCaches).not.toHaveBeenCalled();
    commands.exportCacheSize!.mockImplementation(() => ok({ bytes: 0 }));
    dialogBtn("Clear 3.2 GB")!.click();
    await flushPromises();
    expect(commands.clearExportCaches).toHaveBeenCalled();
    expect(toast.current!.text).toBe("Export caches cleared");
    expect(w.text()).toContain("No export caches");
  });

  it("Cancel clears nothing", async () => {
    const { w } = await setup({ bytes: 1024 ** 3 });
    await flushPromises();
    await btn(w, "Clear export caches…")!.trigger("click");
    dialogBtn("Cancel")!.click();
    await flushPromises();
    expect(commands.clearExportCaches).not.toHaveBeenCalled();
    expect(document.querySelector("[role=dialog]")).toBeNull();
  });

  it("a refusal while an export runs is a toast with the reason", async () => {
    commands.clearExportCaches!.mockImplementation(() => err("Wait until the export finishes"));
    const { w, toast } = await setup({ bytes: 1024 ** 3 });
    await flushPromises();
    await btn(w, "Clear export caches…")!.trigger("click");
    dialogBtn("Clear 1 GB")!.click();
    await flushPromises();
    expect(toast.current!.text).toBe("Wait until the export finishes");
    expect(w.text()).toContain("Export caches use 1 GB");
  });

  it("shows the license as not accepted, or accepted with a date and Withdraw", async () => {
    const a = await setup({ bytes: 0, accepted: null });
    await flushPromises();
    expect(a.w.text()).toContain("Microsoft license for Windows builds: not accepted");
    expect(btn(a.w, "Withdraw")).toBeUndefined();
    document.body.innerHTML = "";
    const b = await setup({ bytes: 0, accepted: "2026-11-01T10:00:00Z" });
    await flushPromises();
    expect(b.w.text()).toMatch(/Microsoft license for Windows builds: accepted .*2026/);
    commands.exportReadiness!.mockImplementation(() => readiness(null));
    await btn(b.w, "Withdraw")!.trigger("click");
    await flushPromises();
    expect(commands.withdrawExportLicense).toHaveBeenCalled();
    expect(b.w.text()).toContain("not accepted");
  });
});
