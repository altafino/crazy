import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const app = vi.hoisted(() => ({ getVersion: vi.fn(), getTauriVersion: vi.fn() }));
vi.mock("@tauri-apps/api/app", () => app);

const opener = vi.hoisted(() => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => opener);

import { useNavStore } from "../stores/nav";
import { useToastStore } from "../stores/toast";
import AboutView from "./AboutView.vue";

const writeText = vi.fn((_text: string) => Promise.resolve());

async function setup() {
  setActivePinia(createPinia());
  const w = mount(AboutView);
  await flushPromises();
  return w;
}

beforeEach(() => {
  app.getVersion.mockReset().mockResolvedValue("0.1.0");
  app.getTauriVersion.mockReset().mockResolvedValue("2.5.1");
  opener.openUrl.mockReset().mockResolvedValue(undefined);
  writeText.mockClear();
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
});

describe("AboutView", () => {
  it("shows the release: version, commit, build date and Tauri version", async () => {
    const w = await setup();
    expect(w.get("h1").text()).toBe("About AI App Studio");
    const rows = Object.fromEntries(w.findAll("dt").map((dt, i) => [dt.text(), w.findAll("dd")[i]!.text()]));
    expect(rows.Version).toBe("0.1.0");
    expect(rows.Commit).toBe("abc1234");
    expect(rows.Built).toContain("2026");
    expect(rows.Tauri).toBe("2.5.1");
  });

  it("copies the details for a bug report and says so", async () => {
    const w = await setup();
    await w.get("button.copy").trigger("click");
    await flushPromises();
    const text = writeText.mock.calls[0]![0];
    expect(text).toContain("AI App Studio 0.1.0 (abc1234)");
    expect(text).toContain("Tauri 2.5.1");
    expect(useToastStore().current?.text).toBe("Details copied.");
  });

  it("still shows the build when the version can't be read", async () => {
    app.getVersion.mockRejectedValue(new Error("no ipc"));
    const w = await setup();
    expect(w.text()).toContain("abc1234");
    expect(w.findAll("dd")[0]!.text()).toBe("Unknown");
  });

  it("links to Help", async () => {
    const w = await setup();
    await w.get("button.help").trigger("click");
    expect(useNavStore().view).toBe("help");
  });

  it("lists the changelog as details with the badge in the summary; this version open, older closed", async () => {
    app.getVersion.mockResolvedValue("0.2.0");
    const w = await setup();
    const blocks = w.findAll("details");
    expect(blocks.length).toBeGreaterThanOrEqual(2);
    // Found by title: an Unreleased block with entries may come first (D7).
    const current = blocks.find((b) => b.get("summary h3").text() === "0.2.0 · 9 Nov 2026")!;
    expect(current.get("summary .badge.ok").text()).toBe("This version");
    expect(current.attributes("open")).toBeDefined();
    expect(blocks[blocks.length - 1]!.attributes("open")).toBeUndefined();
    expect(current.findAll("h4").length).toBeGreaterThan(0);
  });

  it("shows only the heading and link when the version isn't in the changelog", async () => {
    app.getVersion.mockResolvedValue("9.9.9");
    const w = await setup();
    expect(w.get("h2#release-notes").text()).toBe("Release notes");
    expect(w.find(".badge.ok").exists()).toBe(false);
    expect(w.findAll("details")[0]!.attributes("open")).toBeDefined();
  });

  it("opens all releases on GitHub", async () => {
    const w = await setup();
    await w.get(".rn-link").trigger("click");
    expect(opener.openUrl).toHaveBeenCalledWith("https://github.com/altafino/crazy/releases");
  });

  it("What's new moves focus to the Release notes heading once (D9)", async () => {
    setActivePinia(createPinia());
    useNavStore().focusReleaseNotes = true;
    const w = mount(AboutView, { attachTo: document.body });
    await flushPromises();
    expect(document.activeElement?.id).toBe("release-notes");
    expect(useNavStore().focusReleaseNotes).toBe(false);
    w.unmount();
  });
});
