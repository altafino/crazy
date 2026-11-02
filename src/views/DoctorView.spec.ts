import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../bindings", () => ({ commands }));

import type { DoctorReport_Serialize } from "../bindings";
import { mockCommands, ok } from "../test-helpers";
import DoctorView from "./DoctorView.vue";

const report: DoctorReport_Serialize = {
  shell: {
    path: "/usr/bin",
    status: { state: "ok" },
    aliases: [{ tool: "claude", definition: "CLAUDE_CONFIG_DIR=~/.claude-a claude", envAssignments: ["CLAUDE_CONFIG_DIR"] }],
  },
  tools: [
    { id: "git", name: "Git", state: "installed", version: "2.45.0", path: "/usr/bin/git", source: "shellPath" },
    { id: "gh", name: "GitHub CLI", state: "missing", hint: "Install with pacman -S github-cli" },
    { id: "node", name: "Node.js", state: "misconfigured", path: "/opt/node", reason: "Version probe timed out" },
  ],
  graphics: { info: { nvidia: true, wayland: true, driverVersion: "570.1" }, note: "Explicit sync workaround not needed" },
  warnings: ["Alias claude sets CLAUDE_CONFIG_DIR"],
  checkedAt: 1,
};

beforeEach(() => {
  Object.assign(commands, mockCommands());
  commands.runDoctor!.mockImplementation(() => ok(report));
});

describe("DoctorView", () => {
  it("renders tool rows, alias warning, shell and graphics", async () => {
    setActivePinia(createPinia());
    const w = mount(DoctorView);
    await flushPromises();
    expect(commands.runDoctor).toHaveBeenCalledWith(false);

    const rows = w.findAll("tbody tr").map((r) => r.text());
    expect(rows[0]).toContain("Installed");
    expect(rows[0]).toContain("2.45.0");
    expect(rows[0]).toContain("shell PATH");
    expect(rows[1]).toContain("Missing");
    expect(rows[1]).toContain("Install with pacman -S github-cli");
    expect(rows[2]).toContain("Misconfigured");
    expect(rows[2]).toContain("Version probe timed out");

    expect(w.get("[aria-label=Warnings]").text()).toContain("Alias claude sets CLAUDE_CONFIG_DIR");
    expect(w.text()).toContain("CLAUDE_CONFIG_DIR=~/.claude-a claude");
    expect(w.text()).toContain("Driver 570.1");
  });

  it("Re-check forces a fresh run", async () => {
    setActivePinia(createPinia());
    const w = mount(DoctorView);
    await flushPromises();
    await w.findAll("button").find((b) => b.text() === "Re-check")!.trigger("click");
    expect(commands.runDoctor).toHaveBeenLastCalledWith(true);
  });
});
