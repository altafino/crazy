import { describe, expect, it } from "vitest";
import { session } from "../test-helpers";
import { runLabel } from "./runs";

const at = new Date(2026, 10, 8, 14, 32).toISOString();
const now = new Date(2026, 10, 8, 18, 0);
const s = (over = {}) => session({ prompt: "Add a high-score table", createdAt: at, startedAt: at, ...over });

describe("runLabel (D3)", () => {
  it("is prompt first, then the time and the file count; no state when Succeeded", () => {
    expect(runLabel(s(), now, 3)).toMatch(/^Add a high-score table · .*2:32.* · 3 files$/);
    expect(runLabel(s(), now, 1)).toMatch(/ · 1 file$/);
    expect(runLabel(s(), now)).not.toContain("files");
  });
  it("cuts a long first line to 48 characters and ignores later lines", () => {
    const label = runLabel(s({ prompt: `${"x".repeat(60)}\nsecond` }), now);
    expect(label.startsWith(`${"x".repeat(48)}… · `)).toBe(true);
    expect(label).not.toContain("second");
  });
  it("names the state only when it isn't Succeeded", () => {
    expect(runLabel(s({ jobState: "failed" }), now)).toMatch(/ · Failed$/);
    expect(runLabel(s({ jobState: "cancelled" }), now)).toMatch(/ · Cancelled$/);
    expect(runLabel(s({ jobState: "interrupted" }), now)).toMatch(/ · Interrupted$/);
    expect(runLabel(s({ jobState: "running" }), now)).toMatch(/ · Running$/);
  });
  it("puts the day before the time for an older day", () => {
    expect(runLabel(s(), new Date(2026, 10, 9, 9, 0))).toMatch(/^Add a high-score table · .*8.*2:32/);
  });
});
