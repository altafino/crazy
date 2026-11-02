import { describe, expect, it } from "vitest";
import { commitMessage, fixPrompt, stepStates, validateLimits } from "./preview";

const states = (phase: Parameters<typeof stepStates>[0], skipped = false) => stepStates(phase, skipped).map((s) => s.state);

describe("stepStates", () => {
  it("walks Installing — Compiling — App running", () => {
    expect(states({ phase: "starting" })).toEqual(["pending", "pending", "pending"]);
    expect(states({ phase: "installing" })).toEqual(["active", "pending", "pending"]);
    expect(states({ phase: "compiling" })).toEqual(["done", "active", "pending"]);
    expect(states({ phase: "appRunning" })).toEqual(["done", "done", "done"]);
    expect(states({ phase: "buildFailed", firstError: "x" })).toEqual(["done", "failed", "pending"]);
  });
  it("marks a skipped install and renames the lock-wait step (M3-D28)", () => {
    expect(states({ phase: "compiling" }, true)).toEqual(["skipped", "active", "pending"]);
    const w = stepStates({ phase: "waitingForRustBuild" }, false);
    expect(w[1]).toEqual({ label: "Waiting for another Rust build", state: "active" });
  });
});

describe("fixPrompt", () => {
  it("adds the --> location that follows the first error", () => {
    const log = ["Compiling app", "error[E0425]: cannot find value `x`", "  --> src/main.rs:5:3", "  |"];
    expect(fixPrompt("error[E0425]: cannot find value `x`", log)).toContain("Location: src/main.rs:5:3");
  });
  it("still works without a location or error", () => {
    expect(fixPrompt("boom", [])).toContain("boom");
    expect(fixPrompt(null, [])).toMatch(/build failed/);
  });
});

describe("commitMessage", () => {
  it("takes the first line, trimmed to about 72 characters", () => {
    expect(commitMessage("\n Add a dark theme \nmore")).toBe("Add a dark theme");
    const m = commitMessage("x".repeat(100));
    expect(m.length).toBe(72);
    expect(m.endsWith("…")).toBe(true);
  });
});

describe("validateLimits", () => {
  it("accepts the defaults and the range edges", () => {
    expect(validateLimits({ maxLines: 5000, maxLogMb: 10, keepLogs: 5 })).toEqual({});
    expect(validateLimits({ maxLines: 1000, maxLogMb: 100, keepLogs: 1 })).toEqual({});
  });
  it("rejects out-of-range, fractional and empty values per field", () => {
    const e = validateLimits({ maxLines: 999, maxLogMb: 101, keepLogs: NaN });
    expect(Object.keys(e).sort()).toEqual(["keepLogs", "maxLines", "maxLogMb"]);
    expect(validateLimits({ maxLines: 1500.5, maxLogMb: 1, keepLogs: 1 }).maxLines).toBeDefined();
  });
});
