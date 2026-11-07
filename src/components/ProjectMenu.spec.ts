import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../bindings", () => ({ commands }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(), revealItemInDir: vi.fn() }));

import { mockCommands, project } from "../test-helpers";
import ProjectMenu from "./ProjectMenu.vue";

async function setup(p = project()) {
  setActivePinia(createPinia());
  const w = mount(ProjectMenu, { props: { project: p }, attachTo: document.body });
  await w.get("button[aria-haspopup='menu']").trigger("click");
  await flushPromises();
  return w;
}
const items = (w: Awaited<ReturnType<typeof setup>>) => w.findAll("[role='menuitem']");

beforeEach(() => {
  document.body.innerHTML = "";
  Object.assign(commands, mockCommands());
});

describe("ProjectMenu", () => {
  it("↑ from the first item wraps to the last; a click outside closes the menu", async () => {
    const w = await setup();
    await w.get("[role='menu']").trigger("keydown", { key: "ArrowUp" });
    expect(document.activeElement?.textContent).toBe("Remove from Studio…");
    document.body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    await flushPromises();
    expect(w.find("[role='menu']").exists()).toBe(false);
  });

  it("disables the folder actions and skips the repo status when the folder is missing", async () => {
    commands.sourceStatus = vi.fn();
    const w = await setup(project({ state: { kind: "missingWorkspace" } }));
    const disabled = items(w).filter((b) => b.attributes("disabled") !== undefined).map((b) => b.text());
    expect(disabled).toEqual(["Show in Files", "Change app icon…"]);
    expect(commands.sourceStatus).not.toHaveBeenCalled();
  });
});
