import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia } from "pinia";
import ApprovalCard from "./ApprovalCard.vue";

describe("ApprovalCard", () => {
  it("titles a Craft tool with the app and shows the action and params", () => {
    const approval = { requestId: "r", tool: "mcp__craft_photocraft__doc_export", input: { path: "assets/a.png" }, description: "", answered: null };
    const w = mount(ApprovalCard, { props: { approval, cwd: "/p" }, global: { plugins: [createPinia()] } });
    expect(w.get("h3").text()).toBe("Claude wants to use Photocraft");
    expect(w.get("section").attributes("aria-label")).toBe("Claude wants to use Photocraft");
    expect(w.get("pre.detail").text()).toContain("assets/a.png");
  });
});
