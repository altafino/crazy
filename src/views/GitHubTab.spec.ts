import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const { commands, openUrl } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>>, openUrl: vi.fn(() => Promise.resolve()) }));
vi.mock("../bindings", () => ({ commands, events: {} }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }));

import type { AuthState, RemoteInfo, RepoStatus, SourceError } from "../bindings";
import { mockCommands, ok, project } from "../test-helpers";
import { useGithubStore } from "../stores/github";
import { useNavStore } from "../stores/nav";
import { useBuildsStore } from "../stores/builds";
import GitHubTab from "./GitHubTab.vue";

const fail = (e: SourceError) => Promise.resolve({ status: "error" as const, error: e });
const commit = (n: number) => ({ hash: `h${n}`, short: `abc${n}`, subject: `Change ${n}`, author: "Me", time: 1 });
const remote = (over: Partial<RemoteInfo> = {}): RemoteInfo =>
  ({ name: "origin", url: "https://github.com/stefan/notes", upstream: "origin/main", ahead: 0, behind: 0, fetchedAt: null, github: { owner: "stefan", name: "notes" }, ...over }) as RemoteInfo;
const status = (over: Partial<RepoStatus> = {}): RepoStatus => ({
  location: { top: "/p", prefix: "", commonDir: "/p/.git", isSubfolder: false, isLinked: false },
  head: { kind: "branch", name: "main" }, entries: [], total: 0, truncated: false, conflicts: 0, stagedOutside: 0,
  lastCommit: commit(1), undoRestoreAvailable: false, remote: remote(), merging: false, ...over,
} as RepoStatus);
const signedIn: AuthState = { kind: "signedIn", login: "stefan", gitProtocol: "https" };
const outcome = (operation: string, commits: number, conflicted = false) => ({ operation, commits, conflicted, message: "" });

type W = ReturnType<typeof mount>;
async function setup(st: RepoStatus | SourceError = status(), auth: AuthState = signedIn) {
  commands.sourceStatus!.mockImplementation(() => ("head" in st ? ok(st) : fail(st)));
  commands.githubAuthStatus!.mockImplementation(() => ok(auth));
  const pinia = createPinia();
  setActivePinia(pinia);
  const w = mount(GitHubTab, { props: { project: project() }, global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return { w, pinia };
}
const btn = (w: W, text: string) => w.findAll("button").find((b) => b.text() === text)!;
const names = (w: W) => w.findAll(".buttons button").map((b) => b.text());
const sentence = (w: W) => w.find(".sentence").text();

beforeEach(() => {
  document.body.innerHTML = "";
  openUrl.mockClear();
  Object.assign(commands, mockCommands(), Object.fromEntries(
    ["sourceStatus", "sourceSyncCommits", "sourceFetch", "sourcePull", "sourceMergeUpstream", "sourcePush", "sourceAbortMerge", "sourceStopSync",
      "sourceLastSync", "githubAuthStatus", "githubCreateRepo", "githubSignIn", "githubSignInStop"].map((k) => [k, vi.fn()]),
  ));
  commands.sourceSyncCommits!.mockImplementation(() => ok({ outgoing: [commit(3), commit(2)], incoming: [commit(9)] }));
  commands.sourceLastSync!.mockImplementation(() => ok(null));
});

describe("GitHubTab state rows (M5-DR3)", () => {
  it("gh missing: Open Doctor only", async () => {
    const { w, pinia } = await setup(status(), { kind: "ghMissing" });
    expect(sentence(w)).toBe("GitHub CLI is not installed");
    await btn(w, "Open Doctor").trigger("click");
    expect(useNavStore(pinia).view).toBe("doctor");
  });

  it("signed out: Sign in with the no-token caption", async () => {
    const { w } = await setup(status(), { kind: "signedOut" });
    expect(sentence(w)).toBe("Sign in to GitHub");
    expect(w.text()).toContain("stores no token");
    expect(names(w)).toEqual(["Sign in"]);
  });

  it("signing in shows the code row", async () => {
    const { w, pinia } = await setup(status(), { kind: "signedOut" });
    const gh = useGithubStore(pinia);
    gh.signingIn = true;
    gh.signInCode = { code: "ABCD-1234", url: "https://github.com/login/device" } as never;
    gh.signInStartedAt = Date.now();
    await flushPromises();
    expect(w.text()).toContain("Enter this code on GitHub");
    expect(w.find('output[aria-label="Sign-in code"]').text()).toBe("ABCD-1234");
  });

  it("offline: Check again re-checks auth", async () => {
    const { w } = await setup(status(), { kind: "unreachable", message: "dial tcp" });
    expect(sentence(w)).toBe("GitHub can't be reached");
    expect(w.find("details").text()).toContain("dial tcp");
    await btn(w, "Check again").trigger("click");
    expect(commands.githubAuthStatus).toHaveBeenCalledTimes(2);
  });

  it("not a repo and no commit: Open Source tab", async () => {
    const a = await setup({ kind: "notARepo" });
    expect(sentence(a.w)).toBe("This project isn't using Git yet");
    expect(names(a.w)).toEqual(["Open Source tab"]);
    const b = await setup(status({ head: { kind: "unborn", name: "main" }, remote: null }));
    expect(sentence(b.w)).toBe("Commit first");
  });

  it("inside a larger repo: no actions", async () => {
    const { w } = await setup({ kind: "alreadyInRepo", top: "/big" });
    expect(sentence(w)).toBe("This project is inside a larger repository");
    expect(w.text()).toContain("/big");
    expect(names(w)).toEqual([]);
  });

  it("no remote: Create GitHub repository opens the dialog", async () => {
    const { w } = await setup(status({ remote: null }));
    expect(sentence(w)).toBe("Not on GitHub yet");
    expect(names(w)).toEqual(["Create GitHub repository…"]);
    await btn(w, "Create GitHub repository…").trigger("click");
    expect(document.body.textContent).toContain("Create repository");
  });

  it("branch never pushed: Push branch and Fetch", async () => {
    const { w } = await setup(status({ remote: remote({ upstream: null }) }));
    expect(sentence(w)).toBe("Branch main is not on GitHub yet");
    expect(names(w)).toEqual(["Push branch", "Fetch"]);
    commands.sourcePush!.mockImplementation(() => ok(outcome("push", 1)));
    await btn(w, "Push branch").trigger("click");
    expect(commands.sourcePush).toHaveBeenCalledWith("p1");
  });

  it("synced: Fetch primary, Open on GitHub opens the https URL", async () => {
    const { w } = await setup();
    expect(sentence(w)).toBe("Up to date with GitHub");
    expect(w.text()).toContain("stefan/notes · branch main · never fetched");
    expect(btn(w, "Fetch").classes()).toContain("primary");
    expect(w.text()).toContain("Signed in as stefan");
    await btn(w, "Open on GitHub").trigger("click");
    expect(openUrl).toHaveBeenCalledWith("https://github.com/stefan/notes");
  });

  it("ahead: lists commits not on GitHub and pushes without a dialog", async () => {
    const { w } = await setup(status({ remote: remote({ ahead: 2 }) }));
    expect(sentence(w)).toBe("2 commits to push to GitHub");
    expect(w.find("section[aria-labelledby]").text()).toContain("Not on GitHub yet");
    expect(w.findAll("li").length).toBe(2);
    expect(btn(w, "Push 2 commits").classes()).toContain("primary");
    commands.sourcePush!.mockImplementation(() => ok(outcome("push", 2)));
    await btn(w, "Push 2 commits").trigger("click");
    await flushPromises();
    expect(commands.sourcePush).toHaveBeenCalledWith("p1");
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(w.find('[aria-live="polite"]').text()).toBe("Pushed 2 commits");
  });

  it("behind: Pull N commits with the incoming list", async () => {
    const { w } = await setup(status({ remote: remote({ behind: 1 }) }));
    expect(sentence(w)).toBe("GitHub has 1 commit you don't have");
    expect(w.text()).toContain("Incoming from GitHub");
    commands.sourcePull!.mockImplementation(() => ok(outcome("pull", 1)));
    await btn(w, "Pull 1 commit").trigger("click");
    expect(commands.sourcePull).toHaveBeenCalledWith("p1");
  });

  it("diverged: Merge GitHub's changes", async () => {
    const { w } = await setup(status({ remote: remote({ ahead: 1, behind: 1 }) }));
    expect(sentence(w)).toBe("Your project and GitHub both have new commits");
    commands.sourceMergeUpstream!.mockImplementation(() => ok(outcome("merge", 1)));
    await btn(w, "Merge GitHub's changes").trigger("click");
    expect(commands.sourceMergeUpstream).toHaveBeenCalledWith("p1");
  });

  it("merging: Open Source tab primary, Abort merge", async () => {
    const { w, pinia } = await setup(status({ merging: true, remote: remote({ ahead: 1, behind: 1 }) }));
    expect(sentence(w)).toBe("Merge in progress");
    expect(names(w).slice(0, 2)).toEqual(["Open Source tab", "Abort merge"]);
    expect(w.find("section[aria-labelledby]").exists()).toBe(false);
    commands.sourceAbortMerge!.mockImplementation(() => ok(null));
    await btn(w, "Abort merge").trigger("click");
    expect(commands.sourceAbortMerge).toHaveBeenCalledWith("p1");
    await btn(w, "Open Source tab").trigger("click");
    expect(useNavStore(pinia).tab).toBe("source");
  });

  it("fetch calls sourceFetch", async () => {
    const { w } = await setup();
    commands.sourceFetch!.mockImplementation(() => ok(outcome("fetch", 0)));
    await btn(w, "Fetch").trigger("click");
    await flushPromises();
    expect(commands.sourceFetch).toHaveBeenCalledWith("p1");
    expect(w.find('[aria-live="polite"]').text()).toBe("Nothing new on GitHub");
  });

  it("running: elapsed seconds, Stop is the only enabled button", async () => {
    const { w } = await setup(status({ remote: remote({ ahead: 2 }) }));
    let done!: (v: unknown) => void;
    commands.sourcePush!.mockImplementation(() => new Promise((r) => (done = r)));
    await btn(w, "Push 2 commits").trigger("click");
    await flushPromises();
    expect(sentence(w)).toMatch(/^Pushing… \d+ s$/);
    const enabled = w.findAll(".buttons button").filter((b) => b.attributes("disabled") === undefined).map((b) => b.text());
    expect(enabled).toEqual(["Stop"]);
    commands.sourceStopSync!.mockImplementation(() => ok(true));
    await btn(w, "Stop").trigger("click");
    expect(commands.sourceStopSync).toHaveBeenCalledWith("p1");
    done({ status: "ok", data: outcome("push", 2) });
    await flushPromises();
  });

  it("blocked: a dirty tree disables Pull with its reason tied by aria-describedby", async () => {
    const entry = { path: "a.ts", oldPath: null, kind: "modified", staged: false, unstaged: true, submodule: false, nonUtf8: false, binary: false, additions: 1, deletions: 0, secret: false };
    const { w } = await setup(status({ remote: remote({ behind: 1 }), entries: [entry] as never }));
    const pull = btn(w, "Pull 1 commit");
    expect(pull.attributes("disabled")).toBeDefined();
    const why = document.getElementById(pull.attributes("aria-describedby")!);
    expect(why?.textContent).toBe("Commit or restore your changes first.");
  });

  it("blocked: a live release build disables Merge with its reason", async () => {
    const { w } = await setup(status({ remote: remote({ ahead: 1, behind: 1 }) }));
    useBuildsStore().status["p1"] = { live: true } as never;
    await flushPromises();
    const merge = btn(w, "Merge GitHub's changes");
    expect(merge.attributes("disabled")).toBeDefined();
    expect(document.getElementById(merge.attributes("aria-describedby")!)?.textContent).toBe("Wait for the release build to finish.");
  });

  it("non-GitHub remote: shows the URL, no Open on GitHub, no sign-in gate", async () => {
    const { w } = await setup(status({ remote: remote({ github: null, url: "https://example.com/r.git" }) }), { kind: "signedOut" });
    expect(sentence(w)).toBe("Up to date with GitHub");
    expect(w.text()).toContain("https://example.com/r.git");
    expect(names(w)).toEqual(["Fetch"]);
  });
});

describe("GitHubTab failures (M5-DR10)", () => {
  it("rejected push: reason, Pull fix, raw text not shown", async () => {
    const { w } = await setup(status({ remote: remote({ ahead: 1 }) }));
    commands.sourcePush!.mockImplementation(() => fail({ kind: "rejected" }));
    await btn(w, "Push 1 commit").trigger("click");
    await flushPromises();
    const callout = w.find(".callout.danger");
    expect(callout.text()).toContain("GitHub has commits you don't have yet.");
    commands.sourcePull!.mockImplementation(() => ok(outcome("pull", 1)));
    await callout.findAll("button").find((b) => b.text() === "Pull")!.trigger("click");
    expect(commands.sourcePull).toHaveBeenCalled();
  });

  it("auth failure: sign-in wording and an auth re-check", async () => {
    const { w } = await setup(status({ remote: remote({ ahead: 1 }) }));
    commands.sourcePush!.mockImplementation(() => fail({ kind: "authRequired" }));
    await btn(w, "Push 1 commit").trigger("click");
    await flushPromises();
    expect(w.find(".callout.danger").text()).toContain("GitHub needs you to sign in again.");
    expect(commands.githubAuthStatus).toHaveBeenCalledTimes(2);
  });

  it("ssh key rejected: ssh wording and Try again, not Sign in", async () => {
    const { w } = await setup(status({ remote: remote({ ahead: 1 }) }));
    commands.sourcePush!.mockImplementation(() => fail({ kind: "sshKeyRejected" }));
    await btn(w, "Push 1 commit").trigger("click");
    await flushPromises();
    const callout = w.find(".callout.danger");
    expect(callout.text()).toContain("GitHub didn't accept your SSH key.");
    expect(callout.text()).not.toContain("sign in again");
    expect(callout.findAll("button").map((b) => b.text())).toContain("Try again");
    expect(callout.findAll("button").map((b) => b.text())).not.toContain("Sign in");
  });

  it("failed last fetch: no 'Up to date' claim, caption says the fetch failed", async () => {
    commands.sourceLastSync!.mockImplementation(() => ok({ operation: "fetch", ok: false, message: "x", finishedAt: new Date().toISOString() }));
    const { w } = await setup();
    expect(sentence(w)).toBe("Couldn't check GitHub for new commits");
    expect(w.text()).toContain("stefan/notes · branch main · last fetch failed");
  });

  it("unknown git error: generic wording, raw text under Details", async () => {
    const { w } = await setup(status({ remote: remote({ ahead: 1 }) }));
    commands.sourcePush!.mockImplementation(() => fail({ kind: "git", message: "fatal: boom" }));
    await btn(w, "Push 1 commit").trigger("click");
    await flushPromises();
    expect(w.find(".callout.danger").text()).toContain("Git reported a problem.");
    expect(w.find(".callout details").text()).toContain("fatal: boom");
  });

  it("run or build live: reason, no fix button", async () => {
    const { w } = await setup(status({ remote: remote({ behind: 1 }) }));
    commands.sourcePull!.mockImplementation(() => fail({ kind: "buildRunning" }));
    await btn(w, "Pull 1 commit").trigger("click");
    await flushPromises();
    const c = w.find(".callout.danger");
    expect(c.text()).toContain("Wait for the Make-changes run / release build to finish.");
    expect(c.findAll("button").length).toBe(0);
  });
});

describe("Create repository dialog (M5-DR6)", () => {
  async function open() {
    const t = await setup(status({ remote: null }));
    await btn(t.w, "Create GitHub repository…").trigger("click");
    const dlg = document.querySelector('[role="dialog"]') as HTMLElement;
    const q = (s: string) => dlg.querySelector(s) as HTMLInputElement;
    const create = () => [...dlg.querySelectorAll("button")].find((b) => /^Create/.test(b.textContent!) && b.type === "submit")!;
    return { ...t, dlg, q, create };
  }

  it("Private by default, name pre-filled, plain button", async () => {
    const { dlg, q, create } = await open();
    expect(q('input[value="private"]').checked).toBe(true);
    expect(q("#" + dlg.querySelector("label")!.getAttribute("for")).value).toBe("notes");
    expect(create().textContent).toBe("Create repository");
    expect(dlg.textContent).not.toContain("Anyone on the internet");
  });

  it("Public shows the warning, needs the checkbox, button renamed", async () => {
    const { dlg, q, create } = await open();
    q('input[value="public"]').click();
    await flushPromises();
    expect(dlg.textContent).toContain("Anyone on the internet can see this code");
    expect(create().textContent).toBe("Create public repository");
    expect(create().disabled).toBe(true);
    const box = q('input[type="checkbox"]');
    box.click();
    await flushPromises();
    expect(create().disabled).toBe(false);
  });

  it("creates privately and refreshes", async () => {
    const { w, create } = await open();
    commands.githubCreateRepo!.mockImplementation(() => ok({ nameWithOwner: "stefan/notes", url: "u", visibility: "private" }));
    commands.sourceStatus!.mockImplementation(() => ok(status()));
    create().click();
    await flushPromises();
    expect(commands.githubCreateRepo).toHaveBeenCalledWith("p1", "notes", true, null);
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(w.text()).toContain("Up to date with GitHub");
    expect(w.text()).toContain("private");
  });

  it("name taken shows the wording", async () => {
    const { create } = await open();
    commands.githubCreateRepo!.mockImplementation(() => Promise.resolve({ status: "error", error: { kind: "nameTaken" } }));
    create().click();
    await flushPromises();
    expect(document.querySelector('[role="alert"]')!.textContent).toBe("You already have a repository named notes.");
  });
});

describe("GitHub tab label (M5-DR1)", () => {
  it("shows the sync count with a spelled-out name, and the waiting dot while signing in", async () => {
    const { default: ProjectView } = await import("./ProjectView.vue");
    commands.listProjects!.mockImplementation(() => ok([project()]));
    commands.listSessions!.mockImplementation(() => ok([]));
    commands.sourceStatus!.mockImplementation(() => ok(status({ remote: remote({ ahead: 2, behind: 1 }) })));
    const pinia = createPinia();
    setActivePinia(pinia);
    useNavStore().openProject("p1");
    const w = mount(ProjectView, { global: { plugins: [pinia], stubs: { ChatTab: true } }, attachTo: document.body });
    await flushPromises();
    const tab = w.find("#tab-github");
    expect(tab.attributes("aria-label")).toBe("GitHub, 2 to push, 1 to pull");
    expect(tab.text()).toBe("GitHub ↑2 ↓1");
    useGithubStore().signingIn = true;
    await flushPromises();
    expect(w.find("#tab-github").attributes("aria-label")).toBe("GitHub, waiting for sign-in");
    expect(w.find("#tab-github .dot").exists()).toBe(true);
  });
});
