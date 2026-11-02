import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands, ev } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>>, ev: { current: null as unknown } }));
vi.mock("../bindings", () => ({ commands, get events() { return (ev.current as { events: unknown }).events; } }));

import { mockCommands, mockEvents, ok, session } from "../test-helpers";
import { useRunsStore } from "./runs";

let bus: ReturnType<typeof mockEvents>;
beforeEach(() => {
  Object.assign(commands, mockCommands());
  bus = mockEvents();
  ev.current = bus;
  setActivePinia(createPinia());
});

describe("runs store", () => {
  it("merges replayed and live events without duplicates, in seq order", async () => {
    commands.listSessions!.mockImplementation(() => ok([session({ jobState: "running" })]));
    commands.getSessionEvents!.mockImplementation(() => ok([{ kind: "text", seq: 0, text: "a" }, { kind: "text", seq: 1, text: "b" }]));
    const runs = useRunsStore();
    await runs.loadSessions("p1");
    await runs.loadEvents("s1");
    await bus.emit("j1", { kind: "text", seq: 1, text: "b" });
    await bus.emit("j1", { kind: "text", seq: 2, text: "c" });
    await flushPromises();
    expect(runs.eventsBySession.s1!.map((e) => e.seq)).toEqual([0, 1, 2]);
  });

  it("selects the newest run and the run it just started", async () => {
    commands.listSessions!.mockImplementationOnce(() => ok([session({ id: "s1" })]));
    const runs = useRunsStore();
    await runs.loadSessions("p1");
    expect(runs.selected.p1).toBe("s1");
    commands.startRun!.mockImplementation(() => ok("j2"));
    commands.listSessions!.mockImplementation(() => ok([session({ id: "s2", jobId: "j2", jobState: "running" }), session({ id: "s1" })]));
    await runs.start("p1", "hi", "plan", null);
    expect(runs.selected.p1).toBe("s2");
  });

  it("lists projects with a run waiting for approval", async () => {
    commands.listSessions!.mockImplementation(() => ok([session({ jobState: "waitingForApproval" })]));
    const runs = useRunsStore();
    await runs.loadSessions("p1");
    expect(runs.waitingProjects).toEqual(["p1"]);
  });
});
