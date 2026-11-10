import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../../bindings", () => ({ commands }));

import { err, ok } from "../../test-helpers";
import { useDoctorStore } from "../../stores/doctor";
import AgentProfiles from "./AgentProfiles.vue";

const PROFILE = { id: "a1", name: "Claude", provider: "claude", executable: "/usr/bin/claude", env: [], model: null, inactivitySecs: 600, instructions: "", isDefault: true, enabled: true };
const zero = { runs: 0, inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0, costUsd: 0 };
const run = { runs: 2, inputTokens: 30, outputTokens: 29_550, cacheReadTokens: 1_522_834, cacheWriteTokens: 42_817, costUsd: 1.04 };

async function setup(usage: ReturnType<typeof ok> | ReturnType<typeof err>, profiles = [PROFILE]) {
  commands.listAgentProfiles = vi.fn(() => ok(profiles));
  commands.proposeAgentProfile = vi.fn(() => err("none"));
  commands.agentProfileUsage = vi.fn(() => usage);
  const pinia = createPinia();
  setActivePinia(pinia);
  useDoctorStore().report = { tools: [] } as never;
  const w = mount(AgentProfiles, { global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return w;
}

beforeEach(() => {
  document.body.innerHTML = "";
});

describe("Settings › AI Agents usage", () => {
  it("shows tokens and cost per period for the selected profile", async () => {
    const w = await setup(ok({ today: zero, week: run, month: run, all: run }));
    expect(commands.agentProfileUsage).toHaveBeenCalledWith("a1");
    const rows = w.findAll("tbody tr").map((r) => r.findAll("th, td").map((c) => c.text()));
    expect(rows.map((r) => r[0])).toEqual(["Today", "Last 7 days", "Last 30 days", "All time"]);
    expect(rows[3]).toEqual(["All time", "2", "30", "29.6K", "1.5M", "42.8K", "$1.04"]);
    expect(w.find("tbody tr").classes()).toContain("empty");
    expect(w.find('td[title="1,522,834 tokens"]').exists()).toBe(true);
    expect(w.find("section .head .caption").text()).toBe("Last 30 days: 1.6M tokens · $1.04");
  });

  it("says when the profile has no finished runs", async () => {
    const w = await setup(ok({ today: zero, week: zero, month: zero, all: zero }));
    expect(w.text()).toContain("No finished runs with this profile yet.");
    expect(w.find("table").exists()).toBe(false);
  });

  it("reports a failed load", async () => {
    const w = await setup(err("database is locked"));
    expect(w.text()).toContain("Couldn't load usage: database is locked");
  });

  it("is hidden for a profile that isn't saved yet", async () => {
    const w = await setup(ok({ today: zero, week: zero, month: zero, all: zero }), []);
    expect(commands.agentProfileUsage).not.toHaveBeenCalled();
    expect(w.find("#ag-usage").exists()).toBe(false);
  });
});
