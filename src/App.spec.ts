import { describe, expect, it, vi } from "vitest";
import { nextTick } from "vue";
import { shallowMount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

vi.mock("./lib/notify", () => ({ installApprovalNotifications: vi.fn() }));
vi.mock("./lib/startup", () => ({ startUiState: vi.fn(() => Promise.resolve()) }));

import { useNavStore } from "./stores/nav";
import { usePreviewStore } from "./stores/preview";
import { useSettingsStore } from "./stores/settings";
import App from "./App.vue";

function setup() {
  setActivePinia(createPinia());
  vi.spyOn(useSettingsStore(), "load").mockResolvedValue(undefined as never);
  vi.spyOn(usePreviewStore(), "listen").mockResolvedValue(undefined as never);
  return shallowMount(App);
}

describe("App shell", () => {
  it("opens each view at its top: the scroll container is new after a view change", async () => {
    const w = setup();
    const nav = useNavStore();
    nav.go("settings");
    await nextTick();
    const settings = w.get("main").element;
    settings.scrollTop = 2000;
    nav.go("help");
    await nextTick();
    expect(w.get("main").element).not.toBe(settings);
    expect(w.get("main").element.scrollTop).toBe(0);
  });
});
