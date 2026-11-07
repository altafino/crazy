import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { useCraftsStore } from "../../stores/crafts";
import { useNavStore } from "../../stores/nav";
import type { Row } from "../../lib/chat";
import Timeline from "./Timeline.vue";

const tool = (name: string, input: unknown, output: string | null = null): Row => ({
  kind: "tool", key: "k1", at: 0, step: { id: "1", name, summary: "", input, output, isError: false },
});
function mountRows(rows: Row[]) {
  const pinia = createPinia();
  setActivePinia(pinia);
  useCraftsStore().names.photocraft = "PhotoCraft";
  return mount(Timeline, {
    props: { prompt: "p", rows, cwd: "/p", active: false, end: null, planActions: false, canRetry: false },
    global: { plugins: [pinia] },
  });
}

describe("Timeline Craft rows", () => {
  it("labels a Craft tool '<App> · <action>' with an app icon and the file in mono", () => {
    const w = mountRows([tool("mcp__craft_photocraft__doc_export", { path: "assets/hero.png" })]);
    const step = w.get("button.step");
    expect(step.text()).toContain("PhotoCraft · doc export");
    expect(step.get(".path").text()).toBe("assets/hero.png");
    expect(w.find("svg.lucide-image").exists()).toBe(true);
  });
  it("falls back to the capitalised id and the shapes icon for an unknown app", () => {
    const w = mountRows([tool("mcp__craft_texturecraft__paint", {})]);
    expect(w.get("button.step").text()).toContain("Texturecraft · paint");
    expect(w.find("svg.lucide-shapes").exists()).toBe(true);
  });
  it("offers Open Doctor on an ArtCraft warning", async () => {
    const w = mountRows([{ kind: "warning", key: "w", text: "x", details: null, doctor: true }]);
    await w.get("button.doctor").trigger("click");
    expect(useNavStore().view).toBe("doctor");
  });
});
