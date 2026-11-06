import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../bindings", () => ({ commands }));

import type { DoctorReport_Serialize } from "../bindings";
import type { ExportReadiness } from "../bindings";
import { err, mockCommands, ok } from "../test-helpers";
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

describe("DoctorView optional tools (F22, design 10)", () => {
  const docker = (over: Partial<ExportReadiness> = {}) =>
    commands.exportReadiness!.mockImplementation(() => ok({ docker: "ready", dockerVersion: "29.8.2", detail: null, imagePresent: true, licenseAcceptedAt: null, lastTargets: [], ...over }));
  const optional = async () => {
    setActivePinia(createPinia());
    const w = mount(DoctorView);
    await flushPromises();
    const sec = w.findAll("h2").find((h) => h.text() === "Optional tools")!;
    expect(sec).toBeTruthy();
    return { w, row: w.findAll("table")[1]!.find("tbody tr") };
  };

  it("lists Docker under an Optional tools heading in the same table markup, with the caption", async () => {
    docker();
    const { w, row } = await optional();
    expect(w.findAll("table")).toHaveLength(2);
    expect(w.findAll("table")[1]!.findAll("thead th").map((h) => h.text())).toEqual(w.findAll("table")[0]!.findAll("thead th").map((h) => h.text()));
    expect(row.find("th").text()).toBe("Docker");
    expect(row.find(".badge.ok").text()).toBe("Installed");
    expect(row.text()).toContain("29.8.2");
    expect(w.text()).toContain("Needed only for exporting apps for other systems");
  });

  it.each([
    ["missing", "Not installed", "Exports need Docker. Install the docker package."],
    ["notRunning", "Not running", "Docker isn't running. Start it with: systemctl start docker"],
    ["noPermission", "No permission", "Docker is installed, but your user can't use it. Add yourself to the docker group, then log out and back in."],
  ] as const)("%s: a warn badge (never danger) and the fix in Details", async (state, badge, fix) => {
    docker({ docker: state, dockerVersion: null, detail: "raw reason" });
    const { row } = await optional();
    expect(row.find(".badge").text()).toBe(badge);
    expect(row.find(".badge").classes()).toContain("warn");
    expect(row.find(".badge").classes()).not.toContain("danger");
    expect(row.findAll("td")[4]!.text().replace(/\s+/g, " ")).toContain(fix);
    expect(row.text()).toContain("raw reason");
  });

  it("a failed probe is a warn badge with the reason, and Re-check probes again", async () => {
    commands.exportReadiness!.mockImplementation(() => err("probe failed"));
    const { w, row } = await optional();
    expect(row.find(".badge.warn").text()).toBe("Unknown");
    expect(row.text()).toContain("probe failed");
    docker();
    await w.findAll("button").find((b) => b.text() === "Re-check")!.trigger("click");
    await flushPromises();
    expect(commands.exportReadiness).toHaveBeenCalledTimes(2);
    expect(w.findAll("table")[1]!.find(".badge.ok").text()).toBe("Installed");
  });
});
