import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
const openUrl = vi.hoisted(() => vi.fn(() => Promise.resolve()));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }));
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

describe("DoctorView ArtCraft apps (design D25)", () => {
  const row = (over = {}) => ({ name: "PhotoCraft", wired: true, install: "Flatpak 0.21.0", also: null, asksEachAction: false, ...over });
  const craft = async (rows: object[] = [], warnings: string[] = []) => {
    commands.craftDoctor!.mockImplementation(() => ok({ rows, warnings }));
    setActivePinia(createPinia());
    const w = mount(DoctorView);
    await flushPromises();
    const h = w.find("h2#artcraft");
    expect(h.text()).toBe("ArtCraft apps");
    return { w, table: w.findAll("table")[2] };
  };

  it("an installed row: badge, install and the Also caption, with row-header a11y", async () => {
    const { table } = await craft([row({ also: "AppImage 0.7.0" })]);
    expect(table!.find("caption.sr-only").text()).toBe("ArtCraft apps");
    const r = table!.find("tbody tr");
    expect(r.find("th[scope=row]").text()).toBe("PhotoCraft");
    expect(r.find(".badge.ok").text()).toBe("Installed");
    expect(r.text()).toContain("Flatpak 0.21.0");
    expect(r.text()).toContain("Also: AppImage 0.7.0");
    expect(r.text()).not.toContain("Asks before each action");
  });

  it("not wired is a warn badge, never danger; write-root-less apps ask before each action", async () => {
    const { table } = await craft([row({ name: "WordCraft", wired: false }), row({ name: "CADCraft", asksEachAction: true })]);
    const [a, b] = table!.findAll("tbody tr");
    expect(a!.find(".badge").text()).toBe("Not wired");
    expect(a!.find(".badge").classes()).toContain("warn");
    expect(a!.find(".badge").classes()).not.toContain("danger");
    expect(b!.text()).toContain("Asks before each action");
  });

  it("captions, the external link, and detection warnings", async () => {
    const { w } = await craft([row()], ["flatpak list failed: boom"]);
    expect(w.text()).toContain("Studio's undo doesn't cover files over 50 MB or files git ignores.");
    expect(w.text()).toContain("flatpak list failed: boom");
    await w.findAll("a").find((a) => a.text().includes("getartcraft.com/apps"))!.trigger("click");
    expect(openUrl).toHaveBeenCalledWith("https://getartcraft.com/apps");
  });

  it("no apps: no table, only the captions and the link", async () => {
    const { w } = await craft();
    expect(w.findAll("table")).toHaveLength(2);
    expect(w.text()).toContain("More apps at");
    expect(w.text()).toContain("Studio's undo doesn't cover");
  });

  it("shows Checking… while the probe runs", async () => {
    commands.craftDoctor!.mockImplementation(() => new Promise(() => {}));
    setActivePinia(createPinia());
    const w = mount(DoctorView);
    await flushPromises();
    expect(w.findAll("table")[2]!.text()).toContain("Checking…");
  });
});
