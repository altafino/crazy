import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { useNavStore } from "../stores/nav";
import HelpView from "./HelpView.vue";

function setup() {
  setActivePinia(createPinia());
  return mount(HelpView);
}

describe("HelpView", () => {
  it("walks through getting started and explains every project tab", () => {
    const w = setup();
    expect(w.get("h1").text()).toBe("Help");
    expect(w.findAll("h2").map((h) => h.text())).toEqual(["Getting started", "Project tabs", "Approvals", "ArtCraft apps", "Keyboard", "When something goes wrong"]);
    const tabs = w.findAll("#help-tabs dt").map((d) => d.text());
    expect(tabs).toEqual(["Chat", "Plan", "Changes", "Source", "Preview", "Releases", "GitHub", "Database"]);
  });

  it("opens AI Agents settings from the first step", async () => {
    const w = setup();
    await w.get("button.agents").trigger("click");
    const nav = useNavStore();
    expect(nav.view).toBe("settings");
    expect(nav.settingsSection).toBe("agents");
  });

  it("opens the Environment Doctor, Approvals settings and About", async () => {
    const w = setup();
    const nav = useNavStore();
    await w.get("button.doctor").trigger("click");
    expect(nav.view).toBe("doctor");
    await w.get("button.approvals").trigger("click");
    expect([nav.view, nav.settingsSection]).toEqual(["settings", "approvals"]);
    await w.get("button.about").trigger("click");
    expect(nav.view).toBe("about");
  });
});
