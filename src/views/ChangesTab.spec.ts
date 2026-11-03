import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../bindings", () => ({ commands, events: {} }));

import type { FileChange, FileDiff } from "../bindings";
import { err, mockCommands, ok, project, session } from "../test-helpers";
import { useRunsStore } from "../stores/runs";
import ChangesTab from "./ChangesTab.vue";

const file = (over: Partial<FileChange> = {}): FileChange => ({
  path: "src/style.css", oldPath: null, status: "modified", additions: 24, deletions: 2, skippedReason: null, ...over,
});
const diff = (over: Partial<FileDiff> = {}): FileDiff => ({
  binary: false, truncated: false, oldSize: null, newSize: null,
  hunks: [{ oldStart: 1, oldLines: 2, newStart: 1, newLines: 2, lines: [
    { kind: "removed", text: "color: red;" }, { kind: "added", text: "color: blue;" }, { kind: "context", text: "}" },
  ] }],
  ...over,
});

async function setup(s = session(), files: FileChange[] = [file(), file({ path: "src/App.vue", additions: 11, deletions: 1 })], fd: FileDiff = diff()) {
  commands.runDiff!.mockImplementation(() => ok(files));
  commands.runFileDiff!.mockImplementation(() => ok(fd));
  const pinia = createPinia();
  setActivePinia(pinia);
  const runs = useRunsStore();
  runs.sessions.p1 = [s];
  runs.selected.p1 = s.id;
  const w = mount(ChangesTab, { props: { project: project() }, global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return { w, runs };
}

beforeEach(() => {
  document.body.innerHTML = "";
  Object.assign(commands, mockCommands());
});

describe("ChangesTab", () => {
  it("renders the file list and the diff of the first file", async () => {
    const { w } = await setup();
    const rows = w.findAll(".row");
    expect(rows).toHaveLength(2);
    expect(rows[0]!.text()).toContain("M");
    expect(rows[0]!.text()).toContain("+24");
    expect(rows[0]!.attributes("title")).toBe("src/style.css");
    expect(commands.runFileDiff).toHaveBeenCalledWith("s1", "src/style.css", false);
    expect(w.find(".ln.removed").text()).toContain("color: red;");
    expect(w.find(".ln.added").text()).toContain("color: blue;");
    expect(w.find(".hunk").text()).toBe("@@ -1,2 +1,2 @@");
    await rows[1]!.trigger("click");
    expect(commands.runFileDiff).toHaveBeenLastCalledWith("s1", "src/App.vue", false);
  });

  it("has a select for the narrow layout", async () => {
    const { w } = await setup();
    expect(w.find("select").exists()).toBe(true);
    expect(w.findAll("select option")).toHaveLength(2);
  });

  it("no run yet", async () => {
    const pinia = createPinia();
    setActivePinia(pinia);
    const w = mount(ChangesTab, { props: { project: project() }, global: { plugins: [pinia] } });
    await flushPromises();
    expect(w.text()).toContain("No runs yet");
  });

  it("run without changes", async () => {
    const { w } = await setup(session(), []);
    expect(w.text()).toContain("This run didn't change any files.");
  });

  it("live run", async () => {
    const { w } = await setup(session({ jobState: "running" }));
    expect(w.text()).toContain("Changes appear when the run finishes.");
    expect(commands.runDiff).not.toHaveBeenCalled();
  });

  it("reloads when the run finishes", async () => {
    const { w, runs } = await setup(session({ jobState: "running" }));
    runs.sessions.p1 = [session({ jobState: "succeeded" })];
    await flushPromises();
    expect(commands.runDiff).toHaveBeenCalledTimes(1);
    expect(w.findAll(".row")).toHaveLength(2);
  });

  it("interrupted run shows the banner and unticked checkboxes", async () => {
    const { w } = await setup(session({ jobState: "interrupted", hasAfter: false }));
    expect(w.find(".banner").text()).toContain("Studio closed during this run.");
    await w.find(".revert").trigger("click");
    const boxes = document.body.querySelectorAll<HTMLInputElement>("input[type=checkbox]");
    expect(boxes).toHaveLength(2);
    expect([...boxes].every((b) => !b.checked)).toBe(true);
    expect(document.body.querySelector<HTMLButtonElement>(".filled")!.disabled).toBe(true);
  });

  it("binary file shows sizes", async () => {
    const f = [file({ path: "logo.png", status: "binary" })];
    const { w } = await setup(session(), f, diff({ binary: true, hunks: [], oldSize: 12288, newSize: 15360 }));
    expect(w.text()).toContain("Binary file changed (12 KB \u2192 15 KB)");
  });

  it("binary file added or deleted", async () => {
    const f = [file({ path: "logo.png", status: "binary" })];
    const added = await setup(session(), f, diff({ binary: true, hunks: [], newSize: 15360 }));
    expect(added.w.text()).toContain("Binary file added (15 KB)");
    const del = await setup(session(), f, diff({ binary: true, hunks: [], oldSize: 12288 }));
    expect(del.w.text()).toContain("Binary file deleted (12 KB)");
  });

  it("cut diff loads the full diff on demand", async () => {
    const { w } = await setup(session(), undefined, diff({ truncated: true }));
    expect(w.text()).toContain("Showing the first 5,000 lines.");
    commands.runFileDiff!.mockImplementation(() => ok(diff()));
    await w.find(".cut button").trigger("click");
    await flushPromises();
    expect(commands.runFileDiff).toHaveBeenLastCalledWith("s1", "src/style.css", true);
    expect(w.find(".cut").exists()).toBe(false);
  });

  it("pruned snapshots disable Revert and don't load", async () => {
    const { w } = await setup(session({ snapshotPruned: true }));
    expect(w.text()).toContain("Snapshots for this run were cleaned up");
    expect(w.find("button.destructive").attributes("disabled")).toBeDefined();
    expect(commands.runDiff).not.toHaveBeenCalled();
  });

  it("another load failure is generic, with the raw error under Details", async () => {
    commands.runDiff!.mockImplementation(() => err("git: bad object abc123"));
    const pinia = createPinia();
    setActivePinia(pinia);
    const runs = useRunsStore();
    runs.sessions.p1 = [session()];
    runs.selected.p1 = "s1";
    const w = mount(ChangesTab, { props: { project: project() }, global: { plugins: [pinia] } });
    await flushPromises();
    expect(w.text()).toContain("Couldn't load this run's changes.");
    expect(w.text()).not.toContain("cleaned up");
    expect(w.find("details pre").text()).toContain("bad object");
    expect(w.find("p").text()).not.toContain("bad object");
  });

  it("lists files skipped at snapshot time in a footer", async () => {
    const { w } = await setup(session(), [file(), file({ path: "assets/demo.mp4", status: "notCaptured", skippedReason: "tooLarge", additions: 0, deletions: 0 })]);
    expect(w.findAll(".row")).toHaveLength(1);
    expect(w.find(".foot").text()).toContain("assets/demo.mp4");
    expect(w.find(".foot").text()).toContain("larger than 50 MB");
  });

  it("reverts the ticked files and shows the report", async () => {
    commands.revertRun!.mockImplementation(() => ok({ restored: ["src/style.css"], skipped: [{ path: "src/App.vue", reason: "editedSince" }] }));
    const { w } = await setup();
    await w.find(".revert").trigger("click");
    const boxes = () => [...document.body.querySelectorAll<HTMLInputElement>("input[type=checkbox]")];
    expect(boxes().every((b) => b.checked)).toBe(true);
    const go = document.body.querySelector<HTMLButtonElement>(".filled")!;
    expect(go.textContent).toContain("Revert 2 files");
    boxes()[1]!.click();
    await flushPromises();
    expect(go.textContent).toContain("Revert 1 file");
    go.click();
    await flushPromises();
    expect(commands.revertRun).toHaveBeenCalledWith("s1", ["src/style.css"]);
    expect(document.body.querySelector("[role=dialog]")).toBeNull();
    const rep = w.find(".report").text();
    expect(rep).toContain("1 file restored");
    expect(rep).toContain("src/App.vue, edited since the run");
  });

  it("shows a revert failure under Details", async () => {
    commands.revertRun!.mockImplementation(() => err("git exploded"));
    const { w } = await setup();
    await w.find(".revert").trigger("click");
    document.body.querySelector<HTMLButtonElement>(".filled")!.click();
    await flushPromises();
    expect(document.body.querySelector("details pre")!.textContent).toContain("git exploded");
    expect(document.body.querySelector(".callout")!.textContent).toContain("Couldn't revert.");
  });
});
