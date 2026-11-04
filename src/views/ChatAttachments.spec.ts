import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import type { AttachError, AttachResult_Serialize, AttachmentTypeInfo } from "../bindings";

const { commands, openPath, openDialog, drop } = vi.hoisted(() => ({
  commands: {} as Record<string, ReturnType<typeof vi.fn>>,
  openPath: vi.fn(() => Promise.resolve()),
  openDialog: vi.fn(),
  drop: { handler: null as null | ((e: { payload: { type: string; paths?: string[] } }) => void) },
}));
vi.mock("../bindings", () => ({ commands, events: { agentRunEvent: { listen: vi.fn(async () => () => {}) } } }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openPath, openUrl: vi.fn(), revealItemInDir: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: openDialog }));
vi.mock("@tauri-apps/plugin-notification", () => ({ isPermissionGranted: vi.fn(), requestPermission: vi.fn(), sendNotification: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ convertFileSrc: (p: string) => `asset://${p}` }));
vi.mock("@tauri-apps/api/path", () => ({ dataDir: () => Promise.resolve("/data"), join: (...p: string[]) => Promise.resolve(p.join("/")) }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: async (fn: typeof drop.handler) => ((drop.handler = fn), () => (drop.handler = null)) }),
}));

import { mockCommands, ok, project, session } from "../test-helpers";
import { useRunsStore } from "../stores/runs";
import ChatTab from "./ChatTab.vue";

const types: AttachmentTypeInfo[] = [
  { kind: "image", extensions: ["png", "jpg"], mime: "image/png" },
  { kind: "pdf", extensions: ["pdf"], mime: "application/pdf" },
  { kind: "text", extensions: [".md", "txt"], mime: "text/plain" },
  { kind: "docx", extensions: ["docx"], mime: "application/vnd.openxmlformats" },
];
const profile = { id: "pr1", name: "Claude", provider: "claude", executable: "claude", env: [], model: null, inactivitySecs: 600, instructions: "", isDefault: true, enabled: true };
const staged = (name: string, kind: AttachmentTypeInfo["kind"], sizeBytes: number, id = `id-${name}`): AttachResult_Serialize => ({
  name, error: null, staged: { id, name, kind, sizeBytes, storedPath: `${id}/${name}`, convertedPath: kind === "docx" ? `${id}/${name}.md` : null },
});
const failed = (name: string, error: AttachError): AttachResult_Serialize => ({ name, staged: null, error });
const MB = 1024 * 1024;

async function setup(sessions = [] as ReturnType<typeof session>[]) {
  commands.listSessions!.mockImplementation(() => ok(sessions));
  commands.getSessionEvents!.mockImplementation(() => ok([]));
  commands.listAgentProfiles!.mockImplementation(() => ok([profile]));
  commands.attachmentTypes!.mockImplementation(() => Promise.resolve(types));
  const pinia = createPinia();
  setActivePinia(pinia);
  await useRunsStore().loadSessions("p1");
  const w = mount(ChatTab, { props: { project: project() }, global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return w;
}
type W = Awaited<ReturnType<typeof setup>>;
const attachBtn = (w: W) => w.find("button[aria-label='Attach files']");
const runBtn = (w: W) => w.findAll("button").find((b) => b.text() === "Run")!;
const chips = (w: W) => w.findAll("[role=list][aria-label='Attached files'] > li");
const labels = (w: W) => chips(w).map((c) => c.attributes("aria-label"));
async function attach(w: W, results: AttachResult_Serialize[]) {
  openDialog.mockResolvedValue(results.map((r) => `/home/u/${r.name}`));
  commands.attachFiles!.mockImplementation(() => ok(results));
  await attachBtn(w).trigger("click");
  await flushPromises();
}

beforeEach(() => {
  document.body.innerHTML = "";
  localStorage.clear();
  Object.assign(commands, mockCommands());
  openPath.mockClear();
  openDialog.mockReset();
  drop.handler = null;
});
afterEach(() => vi.useRealTimers());

describe("attach button and picker", () => {
  it("is first in the controls row, labelled for icon-only layouts, and builds the filter from attachmentTypes", async () => {
    const w = await setup();
    expect(w.find(".controls").element.firstElementChild).toBe(attachBtn(w).element);
    expect(attachBtn(w).attributes("title")).toBe("Attach files");
    await attach(w, [staged("a.png", "image", MB)]);
    expect(openDialog).toHaveBeenCalledWith({ multiple: true, directory: false, filters: [{ name: "Supported files", extensions: ["png", "jpg", "pdf", "md", "txt", "docx"] }] });
    expect(commands.attachFiles).toHaveBeenCalledWith(expect.any(String), ["/home/u/a.png"]);
  });

  it("is off while a run is active", async () => {
    const w = await setup([session({ jobState: "running" })]);
    expect(attachBtn(w).attributes("disabled")).toBeDefined();
  });
});

describe("chip states and accessible names", () => {
  it("names ready, converted and error chips; clicking opens or previews by kind; error and x names", async () => {
    const w = await setup();
    await attach(w, [
      staged("screenshot.png", "image", 1.2 * MB), staged("spec.pdf", "pdf", 2.4 * MB), staged("requirements.docx", "docx", 30000),
      failed("notes.xlsx", { reason: "unsupported" }),
    ]);
    expect(labels(w)).toEqual([
      "screenshot.png, 1.2 MB", "spec.pdf, 2.4 MB", "requirements.docx, converted", "notes.xlsx, can't attach: This file type isn't supported yet",
    ]);
    const names = w.findAll("[role=list] button").map((b) => b.attributes("aria-label"));
    expect(names).toEqual([
      "Preview screenshot.png", "Remove screenshot.png", "Open spec.pdf", "Remove spec.pdf", "Open requirements.docx", "Remove requirements.docx", "Remove notes.xlsx",
    ]);
    expect(w.find("img").attributes("src")).toMatch(/^asset:\/\/\/data\/ai-app-studio\/attachments\/staging\/.+\/id-screenshot\.png\/screenshot\.png$/);
    expect(w.find("img").attributes("alt")).toBe("screenshot.png");
  });

  it("maps every error reason to its exact text, using the file's own type and limit", async () => {
    const w = await setup();
    await attach(w, [
      failed("big.png", { reason: "tooLarge", mb: 20 }), failed("big.pdf", { reason: "tooLarge", mb: 32 }), failed("big.txt", { reason: "tooLarge", mb: 2 }),
      failed("big.docx", { reason: "tooLarge", mb: 20 }), failed(".env", { reason: "secretFile" }), failed("a.docx", { reason: "pandocMissing" }),
      failed("b.docx", { reason: "conversionTimedOut" }), failed("c.txt", { reason: "copyFailed", detail: "x" }), failed("d.txt", { reason: "tooManyFiles", max: 10 }),
    ]);
    const reasons = w.findAll(".chip .reason").map((r) => r.text());
    expect(reasons).toEqual([
      "Too large: images up to 20 MB", "Too large: PDFs up to 32 MB", "Too large: text files up to 2 MB", "Too large: Word files up to 20 MB",
      "Secret-looking files can't be attached", "Install pandoc to attach Word files", "Conversion took too long", "Couldn't copy this file", "Limit: 10 files per prompt",
    ]);
  });

  it("shows Converting at once for Word files and Adding only after 300 ms", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout"] });
    const w = await setup();
    openDialog.mockResolvedValue(["/home/u/requirements.docx", "/home/u/a.txt"]);
    let finish!: (v: unknown) => void;
    commands.attachFiles!.mockImplementation(() => new Promise((r) => (finish = r)));
    await attachBtn(w).trigger("click");
    await flushPromises();
    expect(labels(w)).toEqual(["requirements.docx, converting"]);
    expect(w.find(".spin").exists()).toBe(true);
    vi.advanceTimersByTime(300);
    await flushPromises();
    expect(labels(w)).toEqual(["requirements.docx, converting", "a.txt, adding"]);
    expect(w.text()).toContain("Adding…");
    finish(await ok([staged("requirements.docx", "docx", 1), staged("a.txt", "text", 1)]));
    await flushPromises();
    expect(labels(w)).toEqual(["requirements.docx, converted", "a.txt, 1 B"]);
  });

  it("truncates long names in the middle and keeps the full name in the tooltip", async () => {
    const w = await setup();
    const long = "quarterly-report-final-version-2.pdf";
    await attach(w, [staged(long, "pdf", MB)]);
    const shown = w.find(".name").text();
    expect(shown).toHaveLength(24);
    expect(shown).toContain("…");
    expect(shown.endsWith(".pdf")).toBe(true);
    expect(chips(w)[0]!.attributes("title")).toBe(long);
  });
});

describe("Run reasons", () => {
  it("an error blocks Run with its reason tied by aria-describedby; plural; error wins over converting", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout"] });
    const w = await setup();
    await w.find("textarea").setValue("Use the files");
    expect(runBtn(w).attributes("disabled")).toBeUndefined();
    await attach(w, [failed("notes.xlsx", { reason: "unsupported" })]);
    const reason = () => w.find("#run-reason");
    expect(runBtn(w).attributes("disabled")).toBeDefined();
    expect(runBtn(w).attributes("aria-describedby")).toBe("run-reason");
    expect(reason().text()).toBe("Remove 1 file that can't be attached");
    // a Word file still converting does not change the message: the error wins
    openDialog.mockResolvedValue(["/home/u/r.docx"]);
    commands.attachFiles!.mockImplementation(() => new Promise(() => {}));
    await attachBtn(w).trigger("click");
    await flushPromises();
    expect(reason().text()).toBe("Remove 1 file that can't be attached");
    await w.find("button[aria-label='Remove notes.xlsx']").trigger("click");
    expect(reason().text()).toBe("Waiting for 1 file to finish converting");
    expect(runBtn(w).attributes("disabled")).toBeDefined();
  });

  it("pluralises and clears once errors are removed", async () => {
    const w = await setup();
    await w.find("textarea").setValue("x");
    await attach(w, [failed("a.xlsx", { reason: "unsupported" }), failed("b.xlsx", { reason: "unsupported" })]);
    expect(w.find("#run-reason").text()).toBe("Remove 2 files that can't be attached");
    for (const n of ["a.xlsx", "b.xlsx"]) await w.find(`button[aria-label='Remove ${n}']`).trigger("click");
    expect(w.find("#run-reason").exists()).toBe(false);
    expect(runBtn(w).attributes("disabled")).toBeUndefined();
  });

  it("announces attached, converted and failures in the status strip", async () => {
    const w = await setup();
    await attach(w, [staged("a.png", "image", MB)]);
    expect(w.find(".strip").attributes("aria-live")).toBe("polite");
    expect(w.find(".strip .sr-only").text()).toBe("a.png attached");
    await attach(w, [staged("r.docx", "docx", MB)]);
    expect(w.find(".strip .sr-only").text()).toBe("r.docx converted");
    await attach(w, [failed("n.xlsx", { reason: "unsupported" })]);
    expect(w.find(".strip .sr-only").text()).toBe("n.xlsx can't attach: This file type isn't supported yet");
  });
});

describe("sending", () => {
  it("passes the draft id and chip ids in order, clears the chips and starts a new draft", async () => {
    commands.startRun!.mockImplementation(() => ok("j2"));
    const w = await setup();
    await attach(w, [staged("b.pdf", "pdf", MB, "id-b"), staged("a.png", "image", MB, "id-a")]);
    await w.find("textarea").setValue("Look at these");
    await runBtn(w).trigger("click");
    await flushPromises();
    const [, , , , draftId, ids] = commands.startRun!.mock.calls[0]!;
    expect(ids).toEqual(["id-b", "id-a"]);
    expect(commands.attachFiles!.mock.calls[0]![0]).toBe(draftId);
    expect(chips(w)).toHaveLength(0);
    await attach(w, [staged("c.png", "image", MB, "id-c")]);
    expect(commands.attachFiles!.mock.calls[1]![0]).not.toBe(draftId);
  });

  it("keeps the chips and shows the error when the run fails to start", async () => {
    commands.startRun!.mockImplementation(() => Promise.resolve({ status: "error", error: { message: "A file is gone. Remove it and try again.", fieldErrors: [] } }));
    const w = await setup();
    await attach(w, [staged("a.png", "image", MB)]);
    await w.find("textarea").setValue("go");
    await runBtn(w).trigger("click");
    await flushPromises();
    expect(w.find("[role=alert]").text()).toContain("A file is gone");
    expect(chips(w)).toHaveLength(1);
  });

  it("sends null draft and ids when nothing is attached", async () => {
    commands.startRun!.mockImplementation(() => ok("j2"));
    const w = await setup();
    await w.find("textarea").setValue("hi");
    await runBtn(w).trigger("click");
    expect(commands.startRun).toHaveBeenCalledWith("p1", "hi", "plan", null, null, null, false);
  });
});

describe("keyboard and paste", () => {
  it("Delete removes a focused chip and moves focus to the next chip, then to the textarea", async () => {
    const w = await setup();
    await attach(w, [staged("a.png", "image", MB, "id-a"), staged("b.pdf", "pdf", MB, "id-b")]);
    const first = w.find("button[aria-label='Preview a.png']");
    (first.element as HTMLElement).focus();
    await first.trigger("keydown", { key: "Delete" });
    await flushPromises();
    expect(commands.removeAttachment).toHaveBeenCalledWith(expect.any(String), "id-a");
    expect(labels(w)).toEqual(["b.pdf, 1 MB"]);
    expect(document.activeElement).toBe(w.find("button[aria-label='Open b.pdf']").element);
    await w.find("button[aria-label='Open b.pdf']").trigger("keydown", { key: "Backspace" });
    await flushPromises();
    expect(document.activeElement).toBe(w.find("textarea").element);
  });

  it("Ctrl+V with an image attaches the pasted bytes; text paste is left alone", async () => {
    const w = await setup();
    commands.attachPasted!.mockImplementation(() => ok(staged("shot.png", "image", MB)));
    const file = new File([new Uint8Array([1, 2, 3])], "shot.png", { type: "image/png" });
    (file as unknown as { arrayBuffer: () => Promise<ArrayBuffer> }).arrayBuffer = () => Promise.resolve(new Uint8Array([1, 2, 3]).buffer);
    const ta = w.find("textarea");
    const ev = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(ev, "clipboardData", { value: { files: [file] } });
    ta.element.dispatchEvent(ev);
    await flushPromises();
    expect(ev.defaultPrevented).toBe(true);
    expect(commands.attachPasted).toHaveBeenCalledWith(expect.any(String), "shot.png", [1, 2, 3]);
    expect(labels(w)).toEqual(["shot.png, 1 MB"]);
    const text = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(text, "clipboardData", { value: { files: [] } });
    ta.element.dispatchEvent(text);
    expect(text.defaultPrevented).toBe(false);
  });
});

describe("paste and draft", () => {
  it("Ctrl+V of files copied in a file manager (text/uri-list) attaches them by path", async () => {
    const w = await setup();
    commands.attachFiles!.mockImplementation(() => ok([staged("my notes.txt", "text", 11), staged("b.png", "image", 5)]));
    commands.clipboardFilePaths!.mockImplementation(() => Promise.resolve(["/home/u/my notes.txt", "/home/u/b.png"]));
    const paste = (types: string[], text = "") => {
      const ev = new Event("paste", { bubbles: true, cancelable: true });
      Object.defineProperty(ev, "clipboardData", { value: { types, files: [], items: [], getData: (t: string) => (t === "text/plain" ? text : "") } });
      w.find("textarea").element.dispatchEvent(ev);
      return ev;
    };
    const ev = paste(["text/uri-list"]);
    await flushPromises();
    expect(ev.defaultPrevented).toBe(true);
    expect(commands.attachFiles).toHaveBeenCalledWith(expect.any(String), ["/home/u/my notes.txt", "/home/u/b.png"]);
    expect(commands.attachPasted).not.toHaveBeenCalled();
    // A copied link (uri-list without local files) still pastes as text.
    commands.clipboardFilePaths!.mockImplementation(() => Promise.resolve([]));
    commands.attachFiles!.mockClear();
    const exec = vi.fn();
    document.execCommand = exec;
    paste(["text/uri-list", "text/plain"], "https://example.com/x");
    await flushPromises();
    expect(commands.attachFiles).not.toHaveBeenCalled();
    expect(exec).toHaveBeenCalledWith("insertText", false, "https://example.com/x");
  });

  it("Ctrl+V reads the image from clipboard items when files is empty (WebKitGTK)", async () => {
    const w = await setup();
    commands.attachPasted!.mockImplementation(() => ok(staged("shot.png", "image", MB)));
    const file = new File([new Uint8Array([7])], "", { type: "image/png" });
    (file as unknown as { arrayBuffer: () => Promise<ArrayBuffer> }).arrayBuffer = () => Promise.resolve(new Uint8Array([7]).buffer);
    const ev = new Event("paste", { bubbles: true, cancelable: true });
    Object.defineProperty(ev, "clipboardData", { value: { files: [], items: [{ kind: "file", getAsFile: () => file }, { kind: "string", getAsFile: () => null }] } });
    w.find("textarea").element.dispatchEvent(ev);
    await flushPromises();
    expect(ev.defaultPrevented).toBe(true);
    expect(commands.attachPasted).toHaveBeenCalledWith(expect.any(String), expect.stringMatching(/^Pasted image .+\.png$/), [7]);
  });

  it("an empty paste event falls back to the Clipboard API for an image (WebKitGTK 2.52)", async () => {
    const w = await setup();
    commands.attachPasted!.mockImplementation(() => ok(staged("shot.png", "image", MB)));
    const blob = new Blob([new Uint8Array([9])], { type: "image/png" });
    const read = vi.fn(() => Promise.resolve([{ types: ["image/png"], getType: () => Promise.resolve(blob) }]));
    Object.defineProperty(navigator, "clipboard", { value: { read }, configurable: true });
    const OrigFile = globalThis.File;
    // jsdom's File has no arrayBuffer(); give the fallback's File one.
    globalThis.File = class extends OrigFile { arrayBuffer() { return Promise.resolve(new Uint8Array([9]).buffer); } } as typeof File;
    try {
      const ev = new Event("paste", { bubbles: true, cancelable: true });
      Object.defineProperty(ev, "clipboardData", { value: { types: [], files: [], items: [] } });
      w.find("textarea").element.dispatchEvent(ev);
      await flushPromises();
      expect(read).toHaveBeenCalled();
      expect(commands.attachPasted).toHaveBeenCalledWith(expect.any(String), expect.stringMatching(/^Pasted image .+\.png$/), [9]);
      const text = new Event("paste", { bubbles: true, cancelable: true });
      Object.defineProperty(text, "clipboardData", { value: { types: ["text/plain"], files: [], items: [] } });
      read.mockClear();
      w.find("textarea").element.dispatchEvent(text);
      await flushPromises();
      expect(read).not.toHaveBeenCalled();
    } finally {
      globalThis.File = OrigFile;
    }
  });

  it("the draft (text and chips) survives leaving the Chat tab", async () => {
    const w = await setup();
    await attach(w, [staged("a.png", "image", MB, "id-a")]);
    await w.find("textarea").setValue("keep me");
    const pinia = (w.vm as unknown as { $pinia: ReturnType<typeof createPinia> }).$pinia;
    w.unmount();
    const again = mount(ChatTab, { props: { project: project() }, global: { plugins: [pinia] }, attachTo: document.body });
    await flushPromises();
    expect((again.find("textarea").element as HTMLTextAreaElement).value).toBe("keep me");
    expect(labels(again)).toEqual(["a.png, 1 MB"]);
    expect(commands.removeAttachment).not.toHaveBeenCalled();
  });
});

describe("drop overlay", () => {
  it("shows Drop files to attach on hover and attaches dropped paths", async () => {
    const w = await setup();
    commands.attachFiles!.mockImplementation(() => ok([staged("a.png", "image", MB)]));
    drop.handler!({ payload: { type: "over" } });
    await flushPromises();
    expect(w.find(".drop").text()).toBe("Drop files to attach");
    expect(w.find(".drop").attributes("aria-hidden")).toBe("true");
    drop.handler!({ payload: { type: "drop", paths: ["/home/u/a.png"] } });
    await flushPromises();
    expect(w.find(".drop").exists()).toBe(false);
    expect(commands.attachFiles).toHaveBeenCalledWith(expect.any(String), ["/home/u/a.png"]);
  });

  it("during a run says to wait and a drop attaches nothing", async () => {
    const w = await setup([session({ jobState: "running" })]);
    drop.handler!({ payload: { type: "over" } });
    await flushPromises();
    expect(w.find(".drop").text()).toBe("Attach files after this run finishes");
    drop.handler!({ payload: { type: "drop", paths: ["/home/u/a.png"] } });
    await flushPromises();
    expect(commands.attachFiles).not.toHaveBeenCalled();
    expect(chips(w)).toHaveLength(0);
  });

  it("stops listening when the Chat tab goes away", async () => {
    const w = await setup();
    w.unmount();
    expect(drop.handler).toBeNull();
  });
});

describe("timeline chips", () => {
  const sent = [
    { runId: "s1", ordinal: 0, name: "settings-mockup.png", kind: "image", sizeBytes: 1000, path: "/data/ai-app-studio/attachments/c/settings-mockup.png", removed: true },
    { runId: "s1", ordinal: 1, name: "spec.pdf", kind: "pdf", sizeBytes: 2.4 * MB, path: "/data/ai-app-studio/attachments/c/spec.pdf", removed: false },
    { runId: "s1", ordinal: 2, name: "shot.png", kind: "image", sizeBytes: 1.2 * MB, path: "/data/ai-app-studio/attachments/c/shot.png", removed: false },
  ];
  async function withSent() {
    commands.listRunAttachments!.mockImplementation(() => ok(sent));
    return setup([session()]);
  }

  it("loads them with one call and shows File removed for pruned files, no x anywhere", async () => {
    const w = await withSent();
    expect(commands.listRunAttachments).toHaveBeenCalledTimes(1);
    expect(commands.listRunAttachments).toHaveBeenCalledWith(["s1"]);
    const tl = w.find("[role=log]");
    expect(tl.findAll("[role=list] > li").map((l) => l.attributes("aria-label"))).toEqual([
      "settings-mockup.png, file removed", "spec.pdf, 2.4 MB", "shot.png, 1.2 MB",
    ]);
    expect(tl.text()).toContain("File removed");
    expect(tl.findAll("button[aria-label^='Remove']")).toHaveLength(0);
    expect(tl.find("li button[aria-label*='settings-mockup']").exists()).toBe(false);
  });

  it("opens non-image chips with the default app by absolute path", async () => {
    const w = await withSent();
    await w.find("button[aria-label='Open spec.pdf']").trigger("click");
    expect(openPath).toHaveBeenCalledWith("/data/ai-app-studio/attachments/c/spec.pdf");
  });

  it("previews images in a modal titled with the name; Esc returns focus to the chip", async () => {
    const w = await withSent();
    const chip = w.find("button[aria-label='Preview shot.png']");
    (chip.element as HTMLElement).focus();
    await chip.trigger("click");
    const dlg = w.find("[role=dialog]");
    expect(dlg.find("h2").text()).toBe("shot.png");
    expect(dlg.find("img").attributes("src")).toBe("asset:///data/ai-app-studio/attachments/c/shot.png");
    await dlg.findAll("button").find((b) => b.text() === "Open in default app")!.trigger("click");
    expect(openPath).toHaveBeenCalledWith("/data/ai-app-studio/attachments/c/shot.png");
    await dlg.trigger("keydown", { key: "Escape" });
    await flushPromises();
    expect(w.find("[role=dialog]").exists()).toBe(false);
    expect(document.activeElement).toBe(chip.element);
  });
});
