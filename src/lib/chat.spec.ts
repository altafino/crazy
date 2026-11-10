import { describe, expect, it } from "vitest";
import { editCounts } from "./chat";

describe("editCounts", () => {
  it("counts lines for Edit, Write and MultiEdit, and ignores other tools", () => {
    expect(editCounts("Edit", { old_string: "a\nb", new_string: "a\nb\nc" })).toEqual({ add: 3, del: 2 });
    expect(editCounts("Write", { content: "x\ny" })).toEqual({ add: 2, del: 0 });
    expect(editCounts("MultiEdit", { edits: [{ old_string: "a", new_string: "b\nc" }, { old_string: "", new_string: "d" }] })).toEqual({ add: 3, del: 1 });
    expect(editCounts("Bash", { command: "ls" })).toBeNull();
  });
});

import { buildRows, craftPath, craftTool, stepDetail } from "./chat";
import type { AgentEvent } from "../bindings";
describe("Craft tools (D24)", () => {
  const step = (input: unknown, output: string | null = null) => ({ id: "1", name: "mcp__craft_photocraft__doc_export", summary: "", input, output, isError: false });
  it("splits mcp__craft_<id>__<action> and ignores every other tool", () => {
    expect(craftTool("mcp__craft_photocraft__doc_export")).toEqual({ id: "photocraft", action: "doc export" });
    expect(craftTool("mcp__other__x")).toBeNull();
    expect(craftTool("Bash")).toBeNull();
  });
  it("finds the file a call names, in its input or its result", () => {
    expect(craftPath(step({ path: "assets/hero.png" }))).toBe("assets/hero.png");
    expect(craftPath(step({ format: "png" }, "Saved to assets/hero.png"))).toBe("assets/hero.png");
    expect(craftPath(step({ format: "png" }, "done"))).toBeNull();
  });
  it("marks the two ArtCraft run warnings for Open Doctor, and no other warning", () => {
    const w = (text: string) => ({ kind: "warning", seq: 1, text, details: null }) as unknown as AgentEvent;
    const rows = buildRows([w("PhotoCraft isn't installed anymore. Reinstall it or untick it in ArtCraft."), w("PhotoCraft didn't start, so Claude can't use it in this run."), w("Something else")]);
    expect(rows.map((r) => r.kind === "warning" && r.doctor)).toEqual([true, true, false]);
  });
});

describe("stepDetail", () => {
  it("drops the tool-name prefix the backend summary carries", () => {
    const s = { id: "1", name: "Edit", summary: "Edit: src/a.css", input: {}, output: null, isError: false };
    expect(stepDetail(s)).toBe("src/a.css");
    expect(stepDetail({ ...s, summary: "Edit" })).toBe("Edit");
  });
});

describe("buildRows approvals", () => {
  const ev = (seq: number, e: object) => ({ seq, atMs: seq, ...e }) as never;
  const call = (seq: number, command: string) => ev(seq, { kind: "toolCall", id: `t${seq}`, name: "Bash", summary: command, input: { command } });
  it("folds an allowed approval into the step it allowed, and keeps a denied one as its own row", () => {
    const rows = buildRows([
      call(1, "ls"),
      ev(2, { kind: "autoAllowed", requestId: "r1", tool: "Bash", input: { command: "ls" }, description: "List files" }),
      ev(5, { kind: "toolCall", id: "t5", name: "Edit", summary: "a.ts", input: { file_path: "a.ts", old_string: "a" } }),
      ev(6, { kind: "approvalNeeded", requestId: "r3", tool: "Edit", input: { old_string: "a", file_path: "a.ts" }, description: "a.ts" }),
      ev(7, { kind: "approvalAnswered", requestId: "r3", allowed: true }),
      ev(3, { kind: "approvalNeeded", requestId: "r2", tool: "Bash", input: { command: "rm x" }, description: "Delete x" }),
      ev(4, { kind: "approvalAnswered", requestId: "r2", allowed: false }),
    ]);
    expect(rows.map((r) => r.kind)).toEqual(["tool", "tool", "approval"]);
    expect(rows[0]).toMatchObject({ why: "List files", allowed: "auto" });
    expect(rows[1]).toMatchObject({ allowed: "you" }); // same input, keys in another order
  });
  it("leaves an approval alone when the step next to it is a different call", () => {
    const rows = buildRows([call(1, "ls"), ev(2, { kind: "autoAllowed", requestId: "r1", tool: "Bash", input: { command: "pwd" }, description: "Where am I" })]);
    expect(rows.map((r) => r.kind)).toEqual(["tool", "approval"]);
  });
});
