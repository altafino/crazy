import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";

const { commands, version } = vi.hoisted(() => ({
  commands: {} as Record<string, ReturnType<typeof vi.fn>>,
  version: { value: "0.3.0" as string | null },
}));
vi.mock("../bindings", () => ({ commands }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: () => (version.value ? Promise.resolve(version.value) : Promise.reject(new Error("no app"))) }));

import { useNavStore } from "../stores/nav";
import { useToastStore } from "../stores/toast";
import { notifyFinishOn, setNotifyFinish } from "./notify";
import { startUiState } from "./startup";

const ok = <T>(data: T) => Promise.resolve({ status: "ok" as const, data });
const err = (message: string) => Promise.resolve({ status: "error" as const, error: { message, fieldErrors: [] } });

function state(notifyFinish: boolean, lastSeenVersion: string | null) {
  commands.getUiState = vi.fn(() => ok({ notifyFinish, lastSeenVersion }));
}

beforeEach(() => {
  setActivePinia(createPinia());
  localStorage.clear();
  version.value = "0.3.0";
  setNotifyFinish(true);
  commands.setNotifyFinish = vi.fn(() => ok(null));
  commands.setLastSeenVersion = vi.fn(() => ok(null));
  state(true, "0.3.0");
});

describe("What's new (D9, eng D3)", () => {
  it("a fresh install stores the version quietly", async () => {
    state(true, null);
    await startUiState();
    expect(commands.setLastSeenVersion).toHaveBeenCalledWith("0.3.0");
    expect(useToastStore().current).toBeNull();
  });

  it("a newer version stores it and offers What's new, which opens About at the notes", async () => {
    state(true, "0.2.0");
    await startUiState();
    expect(commands.setLastSeenVersion).toHaveBeenCalledWith("0.3.0");
    const t = useToastStore().current!;
    expect(t.text).toBe("Updated to 0.3.0");
    expect(t.action?.label).toBe("What's new");
    t.action!.run();
    const nav = useNavStore();
    expect(nav.view).toBe("about");
    expect(nav.focusReleaseNotes).toBe(true);
  });

  it("the same version does nothing", async () => {
    await startUiState();
    expect(commands.setLastSeenVersion).not.toHaveBeenCalled();
    expect(useToastStore().current).toBeNull();
  });

  it("a failed store shows no toast, so it can't repeat every start", async () => {
    state(true, "0.2.0");
    commands.setLastSeenVersion = vi.fn(() => err("disk full"));
    await startUiState();
    expect(useToastStore().current).toBeNull();
  });

  it("an unreadable version changes nothing", async () => {
    state(true, "0.2.0");
    version.value = null;
    await startUiState();
    expect(commands.setLastSeenVersion).not.toHaveBeenCalled();
    expect(useToastStore().current).toBeNull();
  });
});

describe("finish-notification switch (D8)", () => {
  it("applies the stored switch", async () => {
    state(false, "0.3.0");
    await startUiState();
    expect(notifyFinishOn()).toBe(false);
  });

  it("copies a 0.2.0 'off' once and removes the old key", async () => {
    localStorage.setItem("studio.notifyFinish", "off");
    await startUiState();
    expect(commands.setNotifyFinish).toHaveBeenCalledWith(false);
    expect(notifyFinishOn()).toBe(false);
    expect(localStorage.getItem("studio.notifyFinish")).toBeNull();
  });

  it("an old 'on' is just removed", async () => {
    localStorage.setItem("studio.notifyFinish", "on");
    await startUiState();
    expect(commands.setNotifyFinish).not.toHaveBeenCalled();
    expect(localStorage.getItem("studio.notifyFinish")).toBeNull();
  });

  it("keeps the old key when copying fails, to retry next start", async () => {
    localStorage.setItem("studio.notifyFinish", "off");
    commands.setNotifyFinish = vi.fn(() => err("locked"));
    await startUiState();
    expect(notifyFinishOn()).toBe(true);
    expect(localStorage.getItem("studio.notifyFinish")).toBe("off");
  });

  it("an unreadable state leaves notifications on and shows nothing", async () => {
    commands.getUiState = vi.fn(() => err("db"));
    vi.spyOn(console, "warn").mockImplementation(() => {});
    await startUiState();
    expect(notifyFinishOn()).toBe(true);
    expect(useToastStore().current).toBeNull();
  });
});
