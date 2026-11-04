import { beforeEach, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";

const { commands, ev } = vi.hoisted(() => ({
  commands: {} as Record<string, ReturnType<typeof vi.fn>>,
  ev: { code: null as null | ((e: unknown) => void) },
}));
vi.mock("../bindings", () => ({
  commands,
  events: { githubSignInCodeEvent: { listen: vi.fn(async (fn) => ((ev.code = fn), () => (ev.code = null))) } },
}));

import { ok } from "../test-helpers";
import { useGithubStore } from "./github";

const fail = (error: unknown) => Promise.resolve({ status: "error" as const, error });

beforeEach(() => {
  setActivePinia(createPinia());
  for (const k of Object.keys(commands)) delete commands[k];
});

it("shows the code while signing in, then flips to signed in", async () => {
  let finish!: (v: unknown) => void;
  commands.githubSignIn = vi.fn(() => new Promise((r) => (finish = r)));
  const gh = useGithubStore();
  const done = gh.signIn();
  await vi.waitFor(() => expect(ev.code).not.toBeNull());
  ev.code!({ payload: { code: "ABCD-1234", url: "https://github.com/login/device" } });
  expect(gh.signingIn).toBe(true);
  expect(gh.signInCode?.code).toBe("ABCD-1234");
  finish({ status: "ok", data: { kind: "signedIn", login: "octo", gitProtocol: "https" } });
  await done;
  expect(gh.login()).toBe("octo");
  expect(gh.signingIn).toBe(false);
  expect(gh.signInCode).toBeNull();
  expect(ev.code).toBeNull();
});

it("Stop is not an error; ghMissing becomes an auth state", async () => {
  commands.githubSignIn = vi.fn(() => fail({ kind: "stopped" }));
  commands.githubAuthStatus = vi.fn(() => fail({ kind: "ghMissing" }));
  const gh = useGithubStore();
  await gh.signIn();
  expect(gh.error).toBeNull();
  expect((await gh.check())?.kind).toBe("ghMissing");
  commands.githubAuthStatus = vi.fn(() => ok({ kind: "unreachable", message: "offline" }));
  expect((await gh.check())?.kind).toBe("unreachable");
});
