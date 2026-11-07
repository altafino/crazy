import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import type { CraftAppsView } from "../../bindings";

const { commands, openUrl } = vi.hoisted(() => ({
  commands: {} as Record<string, ReturnType<typeof vi.fn>>,
  openUrl: vi.fn(),
}));
vi.mock("../../bindings", () => ({ commands }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }));

import { err, mockCommands, ok } from "../../test-helpers";
import { useToastStore } from "../../stores/toast";
import { useNavStore } from "../../stores/nav";
import ArtCraftPopover from "./ArtCraftPopover.vue";

const row = (id: string, name: string, over: Partial<CraftAppsView["apps"][number]> = {}) => ({
  id, name, purpose: "Images and sprites", kind: "flatpak" as const, version: "0.21.0",
  asksEachAction: false, ticked: false, missing: false, ...over,
});
const view = (over: Partial<CraftAppsView> = {}): CraftAppsView => ({
  enabled: true,
  apps: [
    row("photocraft", "PhotoCraft", { ticked: true }),
    row("soundcraft", "SoundCraft", { purpose: "Sound effects and music", version: "0.18.0", ticked: true }),
    row("cadcraft", "CADCraft", { purpose: "Technical drawings", kind: "appImage", version: "0.18.0", asksEachAction: true }),
  ],
  notInstalled: [{ id: "texturecraft", name: "TextureCraft", purpose: "3D texture painting" }],
  notWired: ["FigCraft"],
  defaults: ["photocraft", "soundcraft"],
  ...over,
});

async function setup(v = view()) {
  setActivePinia(createPinia());
  commands.craftApps = vi.fn(() => ok(v));
  const w = mount(ArtCraftPopover, { props: { projectId: "p1", projectName: "Pixel Hopper" }, attachTo: document.body });
  await flushPromises();
  return w;
}
const button = (w: Awaited<ReturnType<typeof setup>>) => w.get("button[aria-haspopup='dialog']");
async function open(w: Awaited<ReturnType<typeof setup>>) {
  await button(w).trigger("click");
  await flushPromises();
}

beforeEach(() => {
  document.body.innerHTML = "";
  Object.assign(commands, mockCommands());
  openUrl.mockReset().mockResolvedValue(undefined);
});

describe("ArtCraftPopover", () => {
  it("is hidden when no ArtCraft app Claude can use is installed", async () => {
    const w = await setup(view({ enabled: false, apps: [] }));
    expect(commands.craftApps).toHaveBeenCalledWith("p1");
    expect(w.find("button").exists()).toBe(false);
  });

  it("counts the ticked apps on the button, and shows just the name when off", async () => {
    expect(button(await setup()).text()).toBe("ArtCraft · 2");
    expect(button(await setup(view({ enabled: false, apps: view().apps.map((a) => ({ ...a, ticked: false })) }))).text()).toBe("ArtCraft");
  });

  it("lists usable apps, apps to get, apps not usable yet and where files go", async () => {
    const w = await setup();
    await open(w);
    const dlg = w.get("[role='dialog']");
    expect(dlg.attributes("aria-label")).toBe("ArtCraft apps for Pixel Hopper");
    expect(dlg.get("[role='switch']").attributes("aria-checked")).toBe("true");
    expect(dlg.text()).toContain("Use ArtCraft apps in this project");
    expect(dlg.get("fieldset legend").text()).toBe("Apps Claude can use");
    const rows = dlg.findAll("fieldset label");
    expect(rows.map((r) => r.find("input").element.checked)).toEqual([true, true, false]);
    expect(rows[0].text()).toContain("PhotoCraft");
    expect(rows[0].text()).toContain("Images and sprites · Flatpak 0.21.0");
    expect(rows[2].text()).toContain("Technical drawings · AppImage 0.18.0 · asks before each action");
    expect(dlg.text()).toContain("TextureCraft");
    expect(dlg.text()).toContain("3D texture painting");
    expect(dlg.text()).toContain("Found but not usable yet: FigCraft");
    expect(dlg.text()).toContain("Claude saves what it makes in assets/. Studio's undo doesn't cover files over 50 MB or files git ignores.");
    expect(dlg.text()).not.toMatch(/Save|Cancel/);
  });

  it("saves a tick at once and shows the new count", async () => {
    const w = await setup();
    await open(w);
    commands.setCraftApps = vi.fn(() => ok(view({ apps: view().apps.map((a) => ({ ...a, ticked: true })) })));
    await w.findAll("fieldset input")[2].setValue(true);
    await flushPromises();
    expect(commands.setCraftApps).toHaveBeenCalledWith("p1", ["photocraft", "soundcraft", "cadcraft"]);
    expect(button(w).text()).toBe("ArtCraft · 3");
  });

  it("the switch turns on with the default apps and off with none", async () => {
    const off = view({ enabled: false, apps: view().apps.map((a) => ({ ...a, ticked: false })) });
    const w = await setup(off);
    await open(w);
    commands.setCraftApps = vi.fn(() => ok(view()));
    await w.get("[role='switch']").trigger("click");
    await flushPromises();
    expect(commands.setCraftApps).toHaveBeenLastCalledWith("p1", ["photocraft", "soundcraft"]);
    commands.setCraftApps = vi.fn(() => ok(off));
    await w.get("[role='switch']").trigger("click");
    await flushPromises();
    expect(commands.setCraftApps).toHaveBeenLastCalledWith("p1", []);
    expect(button(w).text()).toBe("ArtCraft");
  });

  it("Esc closes and gives focus back to the button; so does a click outside", async () => {
    const w = await setup();
    await open(w);
    expect(button(w).attributes("aria-expanded")).toBe("true");
    await w.get("[role='dialog']").trigger("keydown", { key: "Escape" });
    expect(w.find("[role='dialog']").exists()).toBe(false);
    expect(document.activeElement).toBe(button(w).element);
    await open(w);
    document.body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    await flushPromises();
    expect(w.find("[role='dialog']").exists()).toBe(false);
  });

  it("Get it opens getartcraft.com and Details goes to Doctor", async () => {
    const w = await setup();
    await open(w);
    await w.get("button.get").trigger("click");
    expect(openUrl).toHaveBeenCalledWith("https://getartcraft.com/apps");
    await w.get("button.doctor").trigger("click");
    expect(useNavStore().view).toBe("doctor");
  });
});

describe("ArtCraftPopover states and a11y (D22, D27)", () => {
  const missing = () => view({ apps: [row("photocraft", "PhotoCraft", { ticked: true, missing: true, kind: null, version: undefined }), ...view().apps.slice(1)] });

  it("names its state for assistive tech and the tooltip", async () => {
    expect(button(await setup()).attributes("aria-label")).toBe("ArtCraft apps, 2 on");
    expect(button(await setup()).attributes("title")).toBe("ArtCraft apps, 2 on");
    const off = view({ enabled: false, apps: view().apps.map((a) => ({ ...a, ticked: false })) });
    expect(button(await setup(off)).attributes("aria-label")).toBe("ArtCraft apps, off");
  });

  it("flags a ticked app that is missing with an amber alert on the button", async () => {
    const w = await setup(missing());
    expect(button(w).attributes("aria-label")).toBe("ArtCraft apps, 1 not found");
    expect(button(w).find("svg.lucide-triangle-alert").exists()).toBe(true);
    expect((await setup()).find("svg.lucide-triangle-alert").exists()).toBe(false);
  });

  it("keeps a missing row's checkbox, with a Not found badge in its name and what to do", async () => {
    const w = await setup(missing());
    await open(w);
    const r = w.findAll("fieldset label")[0];
    expect(r.get("input").element.checked).toBe(true);
    expect(r.get(".badge.warn").text()).toBe("Not found");
    expect(r.text()).toContain("Reinstall it or untick it.");
    const input = r.get("input").element;
    const name = input.getAttribute("aria-labelledby")!.split(" ").map((id) => document.getElementById(id)!.textContent).join(" ");
    expect(name).toBe("PhotoCraft Not found");
    expect(document.getElementById(input.getAttribute("aria-describedby")!)!.textContent).toContain("Reinstall it or untick it.");
  });

  it("explains a disabled button while a run is active", async () => {
    setActivePinia(createPinia());
    commands.craftApps = vi.fn(() => ok(view()));
    const w = mount(ArtCraftPopover, { props: { projectId: "p1", projectName: "P", disabled: true }, attachTo: document.body });
    await flushPromises();
    expect(button(w).attributes("title")).toBe("Wait until the current run finishes");
    expect(document.getElementById(button(w).attributes("aria-describedby")!)!.textContent).toBe("Wait until the current run finishes");
  });

  it("ties the footer caption to the switch and 'Get it' to its app", async () => {
    const w = await setup();
    await open(w);
    const sw = w.get("[role='switch']").element;
    expect(document.getElementById(sw.getAttribute("aria-describedby")!)!.textContent).toContain("Claude saves what it makes in assets/");
    const get = w.get("button.get").element;
    expect(get.getAttribute("aria-label")).toBe("Get TextureCraft");
    expect(document.getElementById(get.getAttribute("aria-describedby")!)!.textContent).toContain("3D texture painting");
  });

  it("announces a change politely once it is saved", async () => {
    const w = await setup();
    await open(w);
    const live = w.get("[aria-live='polite']");
    commands.setCraftApps = vi.fn(() => ok(view({ apps: view().apps.map((a) => ({ ...a, ticked: a.id !== "photocraft" })) })));
    await w.findAll("fieldset input")[0].setValue(false);
    await flushPromises();
    expect(live.text()).toBe("PhotoCraft off, 2 apps");
    commands.setCraftApps = vi.fn(() => ok(view({ enabled: false, apps: view().apps.map((a) => ({ ...a, ticked: false })) })));
    await w.get("[role='switch']").trigger("click");
    await flushPromises();
    expect(live.text()).toBe("ArtCraft apps off");
  });

  it("toasts a failed save with a Details action that shows the reason", async () => {
    const w = await setup();
    await open(w);
    commands.setCraftApps = vi.fn(() => err("disk full"));
    await w.findAll("fieldset input")[2].setValue(true);
    await flushPromises();
    const t = useToastStore();
    expect(t.current?.text).toBe("Couldn't save ArtCraft apps");
    t.current!.action!.run();
    expect(t.current?.text).toBe("disk full");
  });

  it("keeps the count when the label hides below 900px", async () => {
    const w = await setup();
    expect(button(w).get(".lbl").text()).toContain("ArtCraft");
    expect(button(w).get(".n").text()).toBe("2");
  });
});
