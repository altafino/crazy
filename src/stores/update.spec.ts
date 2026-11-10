import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../bindings", () => ({ commands }));

import { err, ok } from "../test-helpers";
import { useToastStore } from "./toast";
import { useUpdateStore } from "./update";

const found = (over = {}) => ({ current: "0.4.1", latest: "0.5.0", newer: true, canInstall: true, releaseUrl: "https://x/v0.5.0", ...over });

beforeEach(() => {
  setActivePinia(createPinia());
  commands.checkForUpdate = vi.fn(() => ok(found()));
  commands.installUpdate = vi.fn(() => ok(null));
});

describe("update store", () => {
  it("offers a newer version once, however often the background check runs", async () => {
    const update = useUpdateStore();
    await update.auto();
    expect(useToastStore().current?.text).toBe("Version 0.5.0 is available");
    useToastStore().dismiss();
    await update.auto();
    expect(useToastStore().current).toBeNull();
  });

  it("stays silent in the background when the check fails or finds nothing newer", async () => {
    const update = useUpdateStore();
    commands.checkForUpdate = vi.fn(() => err("Couldn't reach GitHub."));
    await update.auto();
    expect(update.error).toBeNull();
    commands.checkForUpdate = vi.fn(() => ok(found({ latest: "0.4.1", newer: false })));
    await update.auto();
    expect(useToastStore().current).toBeNull();
  });

  it("shows the error of a check the user asked for", async () => {
    commands.checkForUpdate = vi.fn(() => err("Couldn't reach GitHub."));
    const update = useUpdateStore();
    await update.check();
    expect(update.error).toBe("Couldn't reach GitHub.");
  });

  it("installs the version it found, and keeps the error when that fails", async () => {
    const update = useUpdateStore();
    await update.check();
    commands.installUpdate = vi.fn(() => err("The download doesn't match its checksum. Nothing was changed."));
    await update.install();
    expect(update.state).toBe("idle");
    expect(update.error).toContain("checksum");
    commands.installUpdate = vi.fn(() => ok(null));
    await update.install();
    expect(commands.installUpdate).toHaveBeenCalledWith("0.5.0");
    expect(update.state).toBe("installed");
  });

  it("does not install from a copy that can't replace itself", async () => {
    commands.checkForUpdate = vi.fn(() => ok(found({ canInstall: false })));
    const update = useUpdateStore();
    await update.check();
    await update.install();
    expect(commands.installUpdate).not.toHaveBeenCalled();
  });
});
