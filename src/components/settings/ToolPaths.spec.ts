import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../../bindings", () => ({ commands }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { err, ok } from "../../test-helpers";
import { useDoctorStore } from "../../stores/doctor";
import { useNavStore } from "../../stores/nav";
import { useSettingsStore } from "../../stores/settings";
import ToolPaths from "./ToolPaths.vue";

const tool = (id: string, name: string, state: object) => ({ id, name, ...state });
const REPORT = {
  shell: { status: { state: "ok" } }, graphics: {}, warnings: [], checkedAt: 0,
  tools: [
    tool("git", "Git", { state: "installed", version: "2.47.1", path: "/usr/bin/git", source: "shellPath" }),
    tool("node", "Node", { state: "installed", version: "22.1.0", path: "/opt/node/bin/node", source: "override" }),
    tool("cargo", "Cargo", { state: "missing", hint: "Install Rust" }),
    tool("gh", "GitHub CLI", { state: "misconfigured", path: "/bad/gh", reason: "Not executable" }),
  ],
};

async function setup(o: { report?: boolean; overrides?: Record<string, string> } = {}) {
  commands.getSettings = vi.fn(() => ok({ defaultProjectsDir: "/p", toolOverrides: o.overrides ?? { node: "/opt/node/bin/node" } }));
  commands.saveSettings = vi.fn((s) => ok(s));
  commands.runDoctor = vi.fn(() => ok(REPORT));
  const pinia = createPinia();
  setActivePinia(pinia);
  if (o.report !== false) useDoctorStore().report = REPORT as never;
  const w = mount(ToolPaths, { global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return { w, nav: useNavStore(), settings: useSettingsStore() };
}
const cap = (w: ReturnType<typeof mount>, id: string) => w.find(`#tool-${id}-cap`);

beforeEach(() => {
  document.body.innerHTML = "";
});

describe("Settings › Tools rows (D4)", () => {
  it("installed through auto-detect says Found: path · version", async () => {
    const { w } = await setup();
    expect(cap(w, "git").text()).toBe("Found: /usr/bin/git · 2.47.1");
    expect(cap(w, "git").find("button").exists()).toBe(false);
    expect(w.find("#tool-git").attributes("aria-describedby")).toBe("tool-git-cap");
  });

  it("installed through your path says Using your path with a Clear link", async () => {
    const { w } = await setup();
    expect(cap(w, "node").text()).toContain("Using your path · 22.1.0");
    expect(cap(w, "node").find("button").text()).toBe("Clear");
    expect((w.find("#tool-node").element as HTMLInputElement).value).toBe("/opt/node/bin/node");
  });

  it("missing shows the Not found badge and How to install opens Doctor", async () => {
    const { w, nav } = await setup();
    expect(cap(w, "cargo").find(".badge.warn").text()).toBe("Not found");
    await cap(w, "cargo").find("button").trigger("click");
    expect(nav.view).toBe("doctor");
  });

  it("misconfigured shows the report's reason as a field error", async () => {
    const { w } = await setup();
    expect(cap(w, "gh").find(".field-error").text()).toBe("Not executable");
  });

  it("no report yet says Checking…", async () => {
    const { w } = await setup({ report: false });
    expect(cap(w, "git").text()).toBe("Checking…");
    expect(w.findAll("input")).toHaveLength(11); // the fallback list keeps every field reachable
  });

  it("Clear removes the override (back to auto-detect), saves and re-checks", async () => {
    const { w } = await setup();
    await cap(w, "node").find("button").trigger("click");
    await flushPromises();
    expect(commands.saveSettings).toHaveBeenCalledWith({ defaultProjectsDir: "/p", toolOverrides: {} });
    expect((w.find("#tool-node").element as HTMLInputElement).value).toBe("");
    expect(commands.runDoctor).toHaveBeenCalledWith(true);
  });

  it("a typed path saves on blur and a backend refusal shows under the row", async () => {
    const { w } = await setup({ overrides: {} });
    commands.saveSettings!.mockImplementation(() => err("Invalid settings", [{ field: "toolOverrides.git", message: "Use an absolute path" }]));
    await w.find("#tool-git").setValue("git");
    await w.find("#tool-git").trigger("blur");
    await flushPromises();
    expect(cap(w, "git").find(".field-error").text()).toBe("Use an absolute path");
    expect(w.find("#tool-git").attributes("aria-invalid")).toBe("true");
  });

  it("shows Checking… on the saved row while the forced Doctor run is going", async () => {
    const { w } = await setup({ overrides: {} });
    let finish!: (v: unknown) => void;
    commands.runDoctor!.mockImplementation(() => new Promise((r) => (finish = r)));
    await w.find("#tool-git").setValue("/usr/local/bin/git");
    await w.find("#tool-git").trigger("blur");
    await flushPromises();
    expect(cap(w, "git").text()).toBe("Checking…");
    expect(cap(w, "cargo").text()).toContain("Not found"); // the other rows keep their caption
    finish({ status: "ok", data: REPORT });
    await flushPromises();
    expect(cap(w, "git").text()).toContain("Found:");
  });
});

describe("Doctor re-run after a Settings save (R7)", () => {
  it("a folder save does not run the Doctor; a tool path save does", async () => {
    const { settings } = await setup({ overrides: {} });
    await settings.save({ defaultProjectsDir: "/other", toolOverrides: {} });
    expect(commands.runDoctor).not.toHaveBeenCalled();
    await settings.save({ defaultProjectsDir: "/other", toolOverrides: { git: "/usr/bin/git" } });
    expect(commands.runDoctor).toHaveBeenCalledTimes(1);
    expect(commands.runDoctor).toHaveBeenCalledWith(true);
    await settings.save({ defaultProjectsDir: "/other2", toolOverrides: { git: "/usr/bin/git" } });
    expect(commands.runDoctor).toHaveBeenCalledTimes(1);
  });
});
