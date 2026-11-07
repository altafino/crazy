import { beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { useCraftsStore } from "../stores/crafts";
import { wantsTo } from "./notify";

beforeEach(() => setActivePinia(createPinia()));

describe("wantsTo", () => {
  it("keeps the built-in phrases", () => {
    expect(wantsTo("Bash")).toBe("run a command");
    expect(wantsTo("Write")).toBe("edit a file");
    expect(wantsTo("mcp__x__y")).toBe("use mcp__x__y");
  });
  it("names the ArtCraft app for a Craft tool, from the store or the capitalised id", () => {
    expect(wantsTo("mcp__craft_photocraft__doc_export")).toBe("use Photocraft");
    useCraftsStore().names.photocraft = "PhotoCraft";
    expect(wantsTo("mcp__craft_photocraft__doc_export")).toBe("use PhotoCraft");
  });
});
