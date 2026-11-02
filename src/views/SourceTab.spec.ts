import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../bindings", () => ({ commands, events: {} }));

import type { RepoStatus, SourceError, StatusEntry } from "../bindings";
import { mockCommands, ok, project, session } from "../test-helpers";
import { useRunsStore } from "../stores/runs";
import { useNavStore } from "../stores/nav";
import { useToastStore } from "../stores/toast";
import SourceTab from "./SourceTab.vue";

const fail = (e: SourceError) => Promise.resolve({ status: "error" as const, error: e });
const entry = (path: string, over: Partial<StatusEntry> = {}): StatusEntry => ({
  path, oldPath: null, kind: "modified", staged: false, unstaged: true, submodule: false, nonUtf8: false, binary: false,
  additions: 1, deletions: 0, secret: false, ...over,
});
const status = (entries: StatusEntry[] = [entry("a.ts"), entry("b.ts"), entry("c.ts")], over: Partial<RepoStatus> = {}): RepoStatus => ({
  location: { top: "/p", prefix: "", commonDir: "/p/.git", isSubfolder: false, isLinked: false },
  head: { kind: "branch", name: "main" }, entries, total: entries.length, truncated: false, conflicts: 0, stagedOutside: 0,
  lastCommit: { hash: "h", short: "abc123", subject: "Initial", author: "Me", time: 1 }, undoRestoreAvailable: false, ...over,
} as RepoStatus);
const outcome = { commit: { hash: "h2", short: "def", subject: "x", author: "Me", time: 2 }, files: 2 };
const emptyDiff = { hunks: [], binary: false, truncated: false, oldSize: null, newSize: null };

type W = ReturnType<typeof mount>;
function mockStatus(st: RepoStatus | SourceError) {
  commands.sourceStatus!.mockImplementation(() => ("entries" in st ? ok(st) : fail(st)));
  commands.sourceFileDiff!.mockImplementation(() => ok(emptyDiff));
  commands.sourceSecretCheck!.mockImplementation(() => ok([]));
  commands.sourceHistory!.mockImplementation(() => ok([]));
}
async function setup(st: RepoStatus | SourceError = status(), prep?: (pinia: ReturnType<typeof createPinia>) => void) {
  mockStatus(st);
  const pinia = createPinia();
  setActivePinia(pinia);
  prep?.(pinia);
  const w = mount(SourceTab, { props: { project: project() }, global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return { w, pinia };
}
const list = (w: W) => w.find('[role="listbox"]');
const type = (w: W, msg: string) => w.find("#commit-msg").setValue(msg);
const btn = (w: W, text: string) => w.findAll("button").find((b) => b.text() === text)!;

beforeEach(() => {
  document.body.innerHTML = "";
  localStorage.clear();
  Object.assign(commands, mockCommands(), Object.fromEntries(
    ["sourceStatus", "sourceFileDiff", "sourceCommit", "sourceGetIdentity", "sourceSetIdentity", "sourceSecretCheck", "sourceListBranches",
      "sourceCreateBranch", "sourceSwitchBranch", "sourceRestorePreview", "sourceRestore", "sourceUndoRestore", "sourceHistory", "sourceInitGit"]
      .map((k) => [k, vi.fn()]),
  ));
});

describe("SourceTab states", () => {
  it("clean tree: nothing to commit, last commit, no commit box", async () => {
    const { w } = await setup(status([]));
    expect(w.text()).toContain("Nothing to commit");
    expect(w.text()).toContain("Initial");
    expect(w.find("#commit-msg").exists()).toBe(false);
  });

  it("not a repo: explains and initializes with the identity flow", async () => {
    const { w } = await setup({ kind: "notARepo" });
    expect(w.text()).toContain("This project isn't using Git yet");
    commands.sourceInitGit!.mockImplementationOnce(() => fail({ kind: "identityMissing" }));
    await btn(w, "Initialize Git").trigger("click");
    await flushPromises();
    expect(w.find("form[aria-label='Your Git name and email']").exists()).toBe(true);
    commands.sourceInitGit!.mockImplementation(() => ok({ top: "/p", commit: null }));
    const inputs = w.findAll("form input");
    await inputs[0]!.setValue("Ada");
    await inputs[1]!.setValue("ada@example.com");
    await w.find("form").trigger("submit");
    await flushPromises();
    expect(commands.sourceInitGit).toHaveBeenLastCalledWith("p1", { identity: { name: "Ada", email: "ada@example.com" }, confirmSecrets: false });
  });

  it("not a repo: secret files need confirmation", async () => {
    const { w } = await setup({ kind: "notARepo" });
    commands.sourceInitGit!.mockImplementationOnce(() => fail({ kind: "secretsFound", paths: [".env"] }));
    await btn(w, "Initialize Git").trigger("click");
    await flushPromises();
    expect(w.text()).toContain(".env");
    commands.sourceInitGit!.mockImplementation(() => ok({ top: "/p", commit: null }));
    await btn(w, "Initialize anyway").trigger("click");
    expect(commands.sourceInitGit).toHaveBeenLastCalledWith("p1", { identity: null, confirmSecrets: true });
  });

  it("alreadyInRepo", async () => {
    const { w } = await setup({ kind: "alreadyInRepo", top: "/big" });
    expect(w.text()).toContain("/big");
  });

  it("conflicts sort first and block Commit", async () => {
    const { w } = await setup(status([entry("a.ts"), entry("z.ts", { kind: "conflicted" })], { conflicts: 1 }));
    expect(w.findAll('[role="option"]')[0]!.text()).toContain("z.ts");
    await list(w).trigger("keydown", { key: "a", ctrlKey: true });
    await type(w, "msg");
    expect(w.find("button[type=submit]").attributes("disabled")).toBeDefined();
  });
});

describe("file list keyboard", () => {
  it("arrows move and load the diff, Space ticks, Ctrl+A ticks all", async () => {
    const { w } = await setup();
    expect(list(w).attributes("tabindex")).toBe("0");
    expect(w.findAll('[role="option"] input').every((i) => i.attributes("tabindex") === "-1")).toBe(true);
    await list(w).trigger("keydown", { key: "ArrowDown" });
    await flushPromises();
    expect(commands.sourceFileDiff).toHaveBeenLastCalledWith("p1", "b.ts", null, false);
    await list(w).trigger("keydown", { key: " " });
    expect(w.findAll('[role="option"]')[1]!.attributes("aria-selected")).toBe("true");
    expect(w.find("button[type=submit]").text()).toBe("Commit 1 file");
    await list(w).trigger("keydown", { key: "a", ctrlKey: true });
    expect(w.find("button[type=submit]").text()).toBe("Commit 3 files");
  });
});

describe("commit flow", () => {
  async function ready() {
    const s = await setup();
    await list(s.w).trigger("keydown", { key: "a", ctrlKey: true });
    await type(s.w, "Add things");
    return s;
  }

  it("commits the ticked files and toasts", async () => {
    const { w, pinia } = await ready();
    commands.sourceCommit!.mockImplementation(() => ok(outcome));
    await w.find("form.commit").trigger("submit");
    await flushPromises();
    expect(commands.sourceCommit).toHaveBeenCalledWith("p1", "Add things", ["a.ts", "b.ts", "c.ts"]);
    expect(useToastStore(pinia).current?.text).toBe("Committed 2 files");
  });

  it("asks for identity, saves it, then commits", async () => {
    const { w } = await ready();
    commands.sourceCommit!.mockImplementationOnce(() => fail({ kind: "identityMissing" }));
    commands.sourceGetIdentity!.mockImplementation(() => ok({ name: "Ada", email: null }));
    await w.find("form.commit").trigger("submit");
    await flushPromises();
    const form = w.find("form[aria-label='Your Git name and email']");
    expect(form.exists()).toBe(true);
    await form.findAll("input")[1]!.setValue("ada@example.com");
    commands.sourceSetIdentity!.mockImplementation(() => ok(null));
    commands.sourceCommit!.mockImplementation(() => ok(outcome));
    await form.trigger("submit");
    await flushPromises();
    expect(commands.sourceSetIdentity).toHaveBeenCalledWith("p1", { name: "Ada", email: "ada@example.com" });
    expect(commands.sourceCommit).toHaveBeenCalledTimes(2);
    expect(w.find("form[aria-label='Your Git name and email']").exists()).toBe(false);
  });

  it("shows hook output and keeps the message", async () => {
    const { w } = await ready();
    commands.sourceCommit!.mockImplementation(() => fail({ kind: "hookRejected", output: "lint failed: x.ts" }));
    await w.find("form.commit").trigger("submit");
    await flushPromises();
    expect(w.find(".callout.danger pre").text()).toBe("lint failed: x.ts");
    expect((w.find("#commit-msg").element as HTMLTextAreaElement).value).toBe("Add things");
  });

  it("repoLocked offers Retry", async () => {
    const { w } = await ready();
    commands.sourceCommit!.mockImplementationOnce(() => fail({ kind: "repoLocked" }));
    await w.find("form.commit").trigger("submit");
    await flushPromises();
    expect(w.text()).toContain("Another Git program is using this repository");
    commands.sourceCommit!.mockImplementation(() => ok(outcome));
    await btn(w, "Retry").trigger("click");
    await flushPromises();
    expect(commands.sourceCommit).toHaveBeenCalledTimes(2);
  });

  it("secret warning never blocks", async () => {
    const s = await setup();
    commands.sourceSecretCheck!.mockImplementation(() => ok([{ path: "a.ts", line: 3, kind: "AWS key", masked: "AKIA****" }]));
    await list(s.w).trigger("keydown", { key: " " });
    await flushPromises();
    await type(s.w, "x");
    expect(s.w.text()).toContain("Possible secrets");
    expect(s.w.find("button[type=submit]").attributes("disabled")).toBeUndefined();
  });
});

describe("restore", () => {
  it("lists what can't be undone, then toasts with Undo", async () => {
    const { w, pinia } = await setup(status(undefined, { undoRestoreAvailable: true }));
    expect(w.text()).toContain("Undo last restore");
    await list(w).trigger("keydown", { key: " " });
    commands.sourceRestorePreview!.mockImplementation(() => ok([{ path: "a.ts", reason: "secret" }]));
    commands.sourceRestore!.mockImplementation(() => ok({ restored: ["a.ts"], cannotUndo: [] }));
    commands.sourceUndoRestore!.mockImplementation(() => ok(null));
    await btn(w, "Restore…").trigger("click");
    await flushPromises();
    expect(document.body.textContent).toContain("looks like a secret");
    [...document.querySelectorAll("button")].find((b) => b.textContent === "Restore")!.click();
    await flushPromises();
    const t = useToastStore(pinia).current!;
    expect(t.text).toBe("Restored 1 file");
    t.action!.run();
    await flushPromises();
    expect(commands.sourceUndoRestore).toHaveBeenCalled();
  });
});

describe("run lock, handoff, history", () => {
  const live = (pinia: ReturnType<typeof createPinia>) => {
    useRunsStore(pinia).sessions.p1 = [session({ jobState: "running", mode: "changes" })];
  };

  it("disables Restore while a Make-changes run is live and refreshes when it ends", async () => {
    const { w, pinia } = await setup(status(), live);
    await list(w).trigger("keydown", { key: " " });
    const restore = btn(w, "Restore…");
    expect(restore.attributes("disabled")).toBeDefined();
    expect(restore.attributes("title")).toContain("Claude is changing files");
    const before = commands.sourceStatus!.mock.calls.length;
    useRunsStore(pinia).sessions.p1 = [session({ jobState: "succeeded", mode: "changes" })];
    await flushPromises();
    expect(commands.sourceStatus!.mock.calls.length).toBe(before + 1);
  });

  it("disables Initialize Git while a run is live", async () => {
    const { w } = await setup({ kind: "notARepo" }, live);
    expect(btn(w, "Initialize Git").attributes("disabled")).toBeDefined();
  });

  it("consumes the commit handoff: ticks exactly those paths, drafts the message, clears it", async () => {
    const { w, pinia } = await setup(status(), (p) => {
      useNavStore(p).commitHandoff = { projectId: "p1", paths: ["b.ts", "gone.ts"], message: "Add dark theme" };
    });
    expect(w.findAll('[role="option"]').map((o) => o.attributes("aria-selected"))).toEqual(["false", "true", "false"]);
    expect((w.find("#commit-msg").element as HTMLTextAreaElement).value).toBe("Add dark theme");
    expect(useNavStore(pinia).commitHandoff).toBeNull();
  });

  it("remembers History per project", async () => {
    const { w } = await setup();
    await btn(w, "History").trigger("click");
    expect(localStorage.getItem("source.history.p1")).toBe("open");
    expect(commands.sourceHistory).toHaveBeenCalled();
  });
});
