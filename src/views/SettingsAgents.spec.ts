import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../bindings", () => ({ commands }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { err, ok } from "../test-helpers";
import { useNavStore } from "../stores/nav";
import SettingsView from "./SettingsView.vue";

const profile = (over = {}) => ({
  id: "a1", name: "Claude", provider: "claude", executable: "/usr/bin/claude", env: [], model: null,
  inactivitySecs: 600, instructions: "", isDefault: true, enabled: true, ...over,
});
const proposal = {
  profile: profile({ id: "", env: [{ name: "CLAUDE_CONFIG_DIR", value: "/home/u/.claude-a" }] }),
  notes: [],
};
const report = (claude: object) => ({
  shell: { status: { state: "ok" } }, tools: [{ id: "claude", name: "Claude Code", ...claude }], graphics: {}, warnings: [], checkedAt: 0,
});
const installed = { state: "installed", version: "2.1.316", path: "/usr/bin/claude", source: "system" };

async function setup(opts: { profiles?: object[]; section?: "general" | "agents"; claude?: object } = {}) {
  commands.listAgentProfiles!.mockImplementation(() => ok(opts.profiles ?? []));
  commands.proposeAgentProfile!.mockImplementation(() => ok(proposal));
  commands.runDoctor!.mockImplementation(() => ok(report(opts.claude ?? installed)));
  commands.getSettings!.mockImplementation(() => ok({ defaultProjectsDir: "/home/u/Projects", toolOverrides: {} }));
  const pinia = createPinia();
  setActivePinia(pinia);
  useNavStore().settingsSection = opts.section ?? "agents";
  const w = mount(SettingsView, { global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return w;
}
const btn = (w: ReturnType<typeof mount>, label: string) => w.findAll("button").find((b) => b.text().includes(label))!;

beforeEach(() => {
  document.body.innerHTML = "";
  for (const k of ["listAgentProfiles", "saveAgentProfile", "deleteAgentProfile", "probeAgentProfile", "proposeAgentProfile", "runDoctor", "getSettings", "saveSettings"]) commands[k] = vi.fn();
});

describe("Settings sections", () => {
  it("opens on AI Agents when nav.settingsSection is agents", async () => {
    const w = await setup({ section: "agents" });
    expect(w.find("#set-tab-agents").attributes("aria-selected")).toBe("true");
    expect(w.text()).toContain("Executable path");
  });

  it("still renders General", async () => {
    const w = await setup({ section: "general" });
    expect(w.text()).toContain("Default projects folder");
    await w.find("#set-tab-agents").trigger("click");
    expect(w.text()).toContain("Executable path");
  });
});

describe("AI Agents", () => {
  it("shows the alias banner and pre-fills without saving", async () => {
    const w = await setup();
    expect(w.text()).toContain("We found your Claude setup from your shell alias 'claude' (CLAUDE_CONFIG_DIR=/home/u/.claude-a)");
    await btn(w, "Review and save").trigger("click");
    expect((w.find("#ag-exe").element as HTMLInputElement).value).toBe("/usr/bin/claude");
    expect((w.find('input[aria-label="Value 1"]').element as HTMLInputElement).value).toBe("/home/u/.claude-a");
    expect(commands.saveAgentProfile).not.toHaveBeenCalled();
    expect(w.text()).not.toContain("We found your Claude setup");
  });

  it("dismisses the banner", async () => {
    const w = await setup();
    await btn(w, "Dismiss").trigger("click");
    expect(w.text()).not.toContain("We found your Claude setup");
  });

  it("shows a secret env refusal inline on its row", async () => {
    commands.saveAgentProfile!.mockImplementation(() => err("Invalid profile", [{ field: "env.API_TOKEN", message: "Secrets can't be stored here" }]));
    const w = await setup({ profiles: [profile({ env: [{ name: "API_TOKEN", value: "x" }] })] });
    await w.find("form").trigger("submit");
    await flushPromises();
    expect(w.find(".field-error").text()).toContain("Secrets can't be stored here");
    expect(w.find('input[aria-label="Variable name 1"]').attributes("aria-invalid")).toBe("true");
    expect(w.text()).not.toContain("Saved");
  });

  it("shows Saved after a successful save", async () => {
    commands.saveAgentProfile!.mockImplementation(() => ok(profile()));
    const w = await setup({ profiles: [profile()] });
    await w.find("form").trigger("submit");
    await flushPromises();
    expect(w.text()).toContain("Saved");
  });

  it("Test profile shows Works with the version", async () => {
    commands.probeAgentProfile!.mockImplementation(() => ok({ ok: true, version: "2.1.316 (Claude Code)", message: "" }));
    const w = await setup({ profiles: [profile()] });
    await btn(w, "Test profile").trigger("click");
    await flushPromises();
    expect(w.text()).toContain("Works · v2.1.316");
    expect(w.text()).not.toContain("Not verified");
  });

  it("Test profile shows a spinner while pending and the error on failure", async () => {
    let resolve!: (v: unknown) => void;
    commands.probeAgentProfile!.mockImplementation(() => new Promise((r) => (resolve = r)));
    const w = await setup({ profiles: [profile()] });
    await btn(w, "Test profile").trigger("click");
    expect(w.text()).toContain("Testing…");
    resolve({ status: "ok", data: { ok: false, version: null, message: "No such file or directory" } });
    await flushPromises();
    expect(w.text()).toContain("No such file or directory");
    expect(w.text()).not.toContain("Works");
  });

  it("warns on a version outside the verified range", async () => {
    commands.probeAgentProfile!.mockImplementation(() => ok({ ok: true, version: "2.0.9", message: "" }));
    const w = await setup({ profiles: [profile()] });
    await btn(w, "Test profile").trigger("click");
    await flushPromises();
    expect(w.text()).toContain("Not verified with this version; approvals may fall back to strict mode");
  });

  it("explains a missing CLI with the Doctor hint", async () => {
    const w = await setup({ claude: { state: "missing", hint: "Install with npm i -g @anthropic-ai/claude-code" } });
    expect(w.text()).toContain("Claude Code isn't installed or isn't on your PATH");
    expect(w.text()).toContain("Install with npm i -g");
  });

  it("deletes only after confirmation", async () => {
    commands.deleteAgentProfile!.mockImplementation(() => ok(null));
    const w = await setup({ profiles: [profile()] });
    await btn(w, "Delete").trigger("click");
    expect(commands.deleteAgentProfile).not.toHaveBeenCalled();
    commands.listAgentProfiles!.mockImplementation(() => ok([]));
    const confirm = w.findAll("button").filter((b) => b.text() === "Delete").pop()!;
    await confirm.trigger("click");
    await flushPromises();
    expect(commands.deleteAgentProfile).toHaveBeenCalledWith("a1");
  });
});
