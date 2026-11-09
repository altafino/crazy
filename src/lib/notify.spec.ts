import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { isPermissionGranted, onAction, sendNotification } from "@tauri-apps/plugin-notification";
import { useCraftsStore } from "../stores/crafts";
import { useNavStore } from "../stores/nav";
import { installApprovalNotifications, notifyApproval, notifyFinish, setNotifyFinish, wantsTo } from "./notify";

vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: vi.fn(), requestPermission: vi.fn(), sendNotification: vi.fn(), onAction: vi.fn(() => Promise.resolve({})),
}));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ setFocus: () => Promise.resolve() }) }));

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

describe("finish notifications (D12)", () => {
  const focus = (on: boolean) => vi.spyOn(document, "hasFocus").mockReturnValue(on);
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    vi.mocked(isPermissionGranted).mockResolvedValue(true);
  });

  it("stays silent while the window is focused", async () => {
    focus(true);
    await notifyFinish("p1", "chat", "Pac: run finished");
    expect(sendNotification).not.toHaveBeenCalled();
  });
  it("sends the text, with the project tab to open, while unfocused", async () => {
    focus(false);
    await notifyFinish("p1", "releases", "Pac 1.1.1 installed");
    expect(sendNotification).toHaveBeenCalledWith({ title: "AI App Studio", body: "Pac 1.1.1 installed", extra: { projectId: "p1", tab: "releases" } });
  });
  it("switch off silences finishes but not approvals", async () => {
    focus(false);
    setNotifyFinish(false);
    await notifyFinish("p1", "chat", "Pac: run failed");
    expect(sendNotification).not.toHaveBeenCalled();
    await notifyApproval("Pac", { kind: "approvalNeeded", seq: 1, requestId: "r", tool: "Bash", input: {}, description: "" });
    expect(sendNotification).toHaveBeenCalledOnce();
  });
  it("swallows plugin errors", async () => {
    focus(false);
    vi.mocked(sendNotification).mockImplementationOnce(() => { throw new Error("no"); });
    await expect(notifyFinish("p1", "chat", "x")).resolves.toBeUndefined();
  });
  it("a tap opens the project's tab", () => {
    installApprovalNotifications();
    const tap = vi.mocked(onAction).mock.calls[0]![0] as (n: unknown) => void;
    tap({ actionId: "tap", notification: { extra: { projectId: "p9", tab: "preview" } } });
    const nav = useNavStore();
    expect([nav.view, nav.projectId, nav.tab]).toEqual(["project", "p9", "preview"]);
  });
});
