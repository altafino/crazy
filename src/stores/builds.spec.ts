import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import type { BuildLogEvent, BuildStatus } from "../bindings";

const { commands, ev } = vi.hoisted(() => ({
  commands: {} as Record<string, ReturnType<typeof vi.fn>>,
  ev: { status: null as null | ((e: unknown) => void), log: null as null | ((e: unknown) => void) },
}));
vi.mock("../bindings", () => ({
  commands,
  events: {
    buildStatusEvent: { listen: vi.fn(async (fn) => ((ev.status = fn), () => {})) },
    buildLogEvent: { listen: vi.fn(async (fn) => ((ev.log = fn), () => {})) },
  },
}));

import { ok, err } from "../test-helpers";
import { useBuildsStore } from "./builds";
import { useToastStore } from "./toast";

export const status = (over: Partial<BuildStatus> = {}): BuildStatus => ({
  jobId: "b1", projectId: "p1", version: "1.0.0", state: "running", phase: "compiling", startedAt: "2026-11-02T14:32:00Z",
  completedAt: null, errorKind: null, errorMessage: null, releaseId: null, live: true, hasLog: true, ...over,
});
const logEv = (fromSeq: number, lines: string[], jobId = "b1"): { payload: BuildLogEvent } => ({ payload: { projectId: "p1", jobId, fromSeq, lines } });

beforeEach(() => {
  setActivePinia(createPinia());
  Object.assign(commands, {
    getBuildStatus: vi.fn(() => ok(null)), getBuildLog: vi.fn(() => ok(null)), listReleases: vi.fn(() => ok([])),
    getInstallState: vi.fn(() => ok({ kind: "notInstalled" })), listInstalledApps: vi.fn(() => ok([])),
    startBuild: vi.fn(), stopBuild: vi.fn(() => ok(null)), activateRelease: vi.fn(), deleteRelease: vi.fn(() => ok(null)),
    nextBuildVersion: vi.fn(() => ok("1.0.1")),
  });
});

describe("builds store", () => {
  it("load fetches status, log, releases (including removed) and install state", async () => {
    commands.getBuildStatus!.mockImplementation(() => ok(status({ live: false, state: "succeeded" })));
    commands.getBuildLog!.mockImplementation(() => ok({ jobId: "b1", lines: ["a", "b"], nextSeq: 2, logTruncated: false }));
    const s = useBuildsStore();
    await s.load("p1");
    expect(commands.listReleases).toHaveBeenCalledWith("p1", true);
    expect(s.logs.p1!.lines).toEqual(["a", "b"]);
    expect(s.isBuilding("p1")).toBe(false);
  });

  it("keeps log batches that arrive before the tail, drops ones the tail already has", async () => {
    commands.getBuildStatus!.mockImplementation(() => ok(status()));
    commands.getBuildLog!.mockImplementation(() => ok({ jobId: "b1", lines: ["l0", "l1"], nextSeq: 2, logTruncated: false }));
    const s = useBuildsStore();
    await s.listen();
    ev.log!(logEv(1, ["l1"])); // before the tail: held, then dropped as a duplicate
    ev.log!(logEv(2, ["l2"])); // held, replayed
    await s.load("p1");
    ev.log!(logEv(3, ["l3"]));
    expect(s.logs.p1!.lines).toEqual(["l0", "l1", "l2", "l3"]);
  });

  it("a build that ends refreshes releases and install state", async () => {
    const s = useBuildsStore();
    await s.listen();
    ev.status!({ payload: { status: status() } });
    expect(s.isBuilding("p1")).toBe(true);
    commands.listReleases!.mockClear();
    ev.status!({ payload: { status: status({ live: false, state: "succeeded", releaseId: "r1" }) } });
    await flushPromises();
    expect(s.isBuilding("p1")).toBe(false);
    expect(commands.listReleases).toHaveBeenCalledWith("p1", true);
    expect(commands.getInstallState).toHaveBeenCalledWith("p1");
  });

  it("start stores the status; a refusal is kept as the error", async () => {
    const s = useBuildsStore();
    commands.startBuild!.mockImplementationOnce(() => ok(status()));
    await s.start("p1", "1.0.0", null);
    expect(commands.startBuild).toHaveBeenCalledWith("p1", "1.0.0", null);
    expect(s.isBuilding("p1")).toBe(true);
    commands.startBuild!.mockImplementationOnce(() => err("Claude is changing this project"));
    await s.start("p1", "1.0.1", "n");
    expect(s.error.p1!.message).toBe("Claude is changing this project");
  });

  it("activate toasts with Undo that activates the previous release; first install mentions Activities", async () => {
    const s = useBuildsStore();
    const toast = useToastStore();
    commands.activateRelease!.mockImplementationOnce(() => ok({ state: { kind: "installed", releaseId: "r1", version: "1.0.0" }, previousReleaseId: null, previousVersion: null }));
    await s.activate("p1", "r1", "Notes");
    expect(toast.current!.text).toBe("GNOME now starts Notes 1.0.0. It can take a few seconds to appear in Activities.");
    expect(toast.current!.action).toBeUndefined();

    commands.activateRelease!.mockImplementationOnce(() => ok({ state: { kind: "installed", releaseId: "r2", version: "1.1.0" }, previousReleaseId: "r1", previousVersion: "1.0.0" }));
    await s.activate("p1", "r2", "Notes");
    expect(toast.current!.text).toBe("GNOME now starts Notes 1.1.0");
    expect(toast.current!.action!.label).toBe("Undo");

    commands.activateRelease!.mockImplementationOnce(() => ok({ state: { kind: "installed", releaseId: "r1", version: "1.0.0" }, previousReleaseId: "r2", previousVersion: "1.1.0" }));
    toast.current!.action!.run();
    await flushPromises();
    expect(commands.activateRelease).toHaveBeenLastCalledWith("p1", "r1");
    expect(toast.current!.text).toBe("GNOME now starts Notes 1.0.0");
    expect(toast.current!.action).toBeUndefined(); // no Undo chain
  });
});
