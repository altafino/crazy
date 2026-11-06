import { describe, expect, it } from "vitest";
import { planPrompt, taskOfPrompt, taskPrompt } from "./plan";

describe("plan prompts", () => {
  it("reads the task back from a Start prompt, with or without detail lines", () => {
    expect(taskOfPrompt(taskPrompt({ text: "Add ghosts", detail: "" }))).toBe("Add ghosts");
    expect(taskOfPrompt(taskPrompt({ text: "Draw the maze", detail: "Arrow keys move\nOne tile per press" }))).toBe("Draw the maze");
  });

  it("does not take an ordinary prompt for a task run", () => {
    expect(taskOfPrompt("Add dark theme")).toBeNull();
    expect(taskOfPrompt("Please Do this task from the project plan: x")).toBeNull();
  });

  it("asks for a - [ ] checklist and keeps the description", () => {
    const p = planPrompt("  a maze game  ");
    expect(p).toContain("Make a plan for this app: a maze game");
    expect(p).toContain('"- [ ] Task"');
  });
});
