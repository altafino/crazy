import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import type { PreviewStatus } from "../bindings";

const { commands, bus } = vi.hoisted(() => ({
  commands: {} as Record<string, ReturnType<typeof vi.fn>>,
  bus: { status: null as null | ((e: unknown) => void), log: null as null | ((e: unknown) => void) },
}));
vi.mock("../bindings", () => ({
  commands,
  events: {
    previewStatusEvent: { listen: vi.fn(async (fn) => ((bus.status = fn), () => {})) },
    previewLogEvent: { listen: vi.fn(async (fn) => ((bus.log = fn), () => {})) },
  },
}));

import { ok } from "../test-helpers";
import { usePreviewStore } from "./preview";

const status = (over: Partial<PreviewStatus> = {}): PreviewStatus => ({
  jobId: "j1", projectId: "p1", state: "running", phase: { phase: "compiling" }, port: 1420,
  startedAt: null, completedAt: null, errorMessage: null, live: true, hasLog: true, ...over,
});
const pushLog = (fromSeq: number, lines: string[], jobId = "j1") => bus.log!({ payload: { projectId: "p1", jobId, fromSeq, lines } });

beforeEach(() => {
  setActivePinia(createPinia());
  Object.assign(commands, {
    getPreviewStatus: vi.fn(() => ok(null)), getPreviewLog: vi.fn(() => ok(null)), getPreviewSettings: vi.fn(() => ok({ maxLines: 5000, maxLogMb: 10, keepLogs: 5 })),
    startPreview: vi.fn(), restartPreview: vi.fn(), stopPreview: vi.fn(),
  });
});

describe("preview store", () => {
  it("drops log events below the tail's nextSeq and applies later ones in order", async () => {
    commands.getPreviewStatus!.mockImplementation(() => ok(status()));
    commands.getPreviewLog!.mockImplementation(() => ok({ jobId: "j1", lines: ["a", "b", "c"], nextSeq: 3, logTruncated: false }));
    const p = usePreviewStore();
    await p.load("p1");
    pushLog(1, ["b", "c"]); // already in the tail
    pushLog(3, ["d", "e"]);
    pushLog(3, ["d", "e"]); // duplicate delivery
    pushLog(5, ["f"]);
    expect(p.logs.p1!.lines).toEqual(["a", "b", "c", "d", "e", "f"]);
    expect(p.logs.p1!.nextSeq).toBe(6);
    expect(p.logs.p1!.firstSeq).toBe(0);
  });

  it("buffers batches that arrive before the log is loaded and replays them after the tail", async () => {
    commands.getPreviewStatus!.mockImplementation(() => ok(status({ live: false, state: "succeeded" })));
    let release!: () => void;
    commands.getPreviewLog!.mockImplementation(
      () => new Promise((res) => (release = () => res({ status: "ok", data: { jobId: "j1", lines: ["a", "b"], nextSeq: 2, logTruncated: false } }))),
    );
    const p = usePreviewStore();
    const loading = p.load("p1");
    await vi.waitFor(() => expect(release).toBeTypeOf("function"));
    pushLog(2, ["c"]);
    release();
    await loading;
    expect(p.logs.p1!.lines).toEqual(["a", "b", "c"]);
  });

  it("keeps only the newest maxLines and numbers from the right offset", async () => {
    commands.getPreviewSettings!.mockImplementation(() => ok({ maxLines: 3, maxLogMb: 10, keepLogs: 5 }));
    const p = usePreviewStore();
    await p.load("p1");
    bus.status!({ payload: { status: status() } });
    pushLog(0, ["1", "2", "3", "4", "5"]);
    expect(p.logs.p1!.lines).toEqual(["3", "4", "5"]);
    expect(p.logs.p1!.firstSeq).toBe(2);
  });

  it("starts a fresh log for a new job and tracks the indicator and session flag", async () => {
    const p = usePreviewStore();
    await p.load("p1");
    expect(p.thisSession.p1).toBeUndefined();
    bus.status!({ payload: { status: status() } });
    pushLog(0, ["old"]);
    expect(p.indicatorOf("p1")).toEqual({ tone: "accent", label: "starting" });
    bus.status!({ payload: { status: status({ jobId: "j2", phase: { phase: "buildFailed", firstError: "e" } }) } });
    expect(p.logs.p1!.lines).toEqual([]);
    expect(p.indicatorOf("p1")).toEqual({ tone: "danger", label: "build failed" });
    bus.status!({ payload: { status: status({ jobId: "j2", phase: { phase: "appRunning" } }) } });
    expect(p.indicatorOf("p1")!.tone).toBe("ok");
    bus.status!({ payload: { status: status({ jobId: "j2", live: false, phase: null, state: "cancelled" }) } });
    expect(p.indicatorOf("p1")).toBeNull();
    expect(p.thisSession.p1).toBe(true);
  });

  it("surfaces a failed start (preflight message) without changing status", async () => {
    commands.startPreview!.mockImplementation(() => Promise.resolve({ status: "error", error: { message: "node is missing", fieldErrors: [] } }));
    const p = usePreviewStore();
    await p.start("p1");
    expect(p.error.p1!.message).toBe("node is missing");
    expect(p.status.p1).toBeUndefined();
  });
});
