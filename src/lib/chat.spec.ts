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

import { stepDetail } from "./chat";
describe("stepDetail", () => {
  it("drops the tool-name prefix the backend summary carries", () => {
    const s = { id: "1", name: "Edit", summary: "Edit: src/a.css", input: {}, output: null, isError: false };
    expect(stepDetail(s)).toBe("src/a.css");
    expect(stepDetail({ ...s, summary: "Edit" })).toBe("Edit");
  });
});
