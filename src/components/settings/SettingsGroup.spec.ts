import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defineComponent, h } from "vue";
import { mount } from "@vue/test-utils";
import type { Outcome } from "../../lib/call";
import SettingsGroup from "./SettingsGroup.vue";

type Doc = { a: number; b: number };
type Api = { commit: (f: string, v: unknown) => Promise<boolean>; commitMany: (p: Record<string, unknown>) => Promise<boolean>; doc: Doc; draft: Doc };

const good = <T,>(data: T): Outcome<T> => ({ ok: true, data });
const bad = (message: string, fieldErrors: { field: string; message: string }[] = []): Outcome<Doc> => ({ ok: false, error: { message, fieldErrors } });
/** A save the test settles by hand. */
function deferred() {
  const calls: { doc: Doc; settle: (r: Outcome<Doc>) => void }[] = [];
  const save = vi.fn((doc: Doc) => new Promise<Outcome<Doc>>((resolve) => calls.push({ doc, settle: resolve })));
  return { save, calls };
}
const tick = () => vi.advanceTimersByTimeAsync(0);

function mountGroup(o: { load?: () => Promise<Outcome<Doc>>; save: (d: Doc) => Promise<Outcome<Doc>>; validate?: (f: string, v: unknown) => string | null }) {
  const api = {} as Api;
  const Host = defineComponent({
    setup: () => () =>
      h(SettingsGroup as never, { title: "Test", load: o.load ?? (() => Promise.resolve(good({ a: 1, b: 2 }))), save: o.save, validate: o.validate }, {
        default: (p: Api & { errorFor: (f: string) => string | undefined }) => {
          Object.assign(api, p);
          return [
            h("input", { id: "a", "data-field": "a", value: p.draft.a, onInput: (e: Event) => ((p.draft as Doc).a = Number((e.target as HTMLInputElement).value)) }),
            p.errorFor("a") ? h("p", { class: "field-error" }, p.errorFor("a")) : null,
          ];
        },
      }),
  });
  return { w: mount(Host, { attachTo: document.body }), api };
}

beforeEach(() => {
  vi.useFakeTimers();
  document.body.innerHTML = "";
});
afterEach(() => vi.useRealTimers());

describe("SettingsGroup", () => {
  it("is disabled with 'Loading settings…' until the first load, and a failed load offers Retry", async () => {
    const load = vi.fn().mockResolvedValueOnce(bad("db locked")).mockResolvedValueOnce(good({ a: 1, b: 2 }));
    const { w } = mountGroup({ load, save: vi.fn() });
    expect(w.text()).toContain("Loading settings…");
    expect(w.find("input").exists()).toBe(false);
    await tick();
    expect(w.text()).toContain("Couldn't load settings");
    expect(w.find("details pre").text()).toBe("db locked");
    await w.findAll("button").find((b) => b.text() === "Retry")!.trigger("click");
    await tick();
    expect(load).toHaveBeenCalledTimes(2);
    expect((w.find("#a").element as HTMLInputElement).value).toBe("1");
  });

  it("a valid commit saves the latest saved document with only that field changed", async () => {
    const save = vi.fn((d: Doc) => Promise.resolve(good(d)));
    const { w, api } = mountGroup({ save });
    await tick();
    api.draft.b = 99; // an unrelated edit in progress is never sent
    await api.commit("a", 5);
    expect(save).toHaveBeenCalledWith({ a: 5, b: 2 });
    expect(w.find(".card").attributes("role")).toBe("group");
  });

  it("an invalid commit sends nothing and shows the message under the field", async () => {
    const save = vi.fn();
    const { w, api } = mountGroup({ save, validate: (_f, v) => ((v as number) > 10 ? "Too big" : null) });
    await tick();
    expect(await api.commit("a", 50)).toBe(false);
    await tick();
    expect(save).not.toHaveBeenCalled();
    expect(w.find(".field-error").text()).toBe("Too big");
  });

  it("an unchanged value sends nothing", async () => {
    const save = vi.fn();
    const { api } = mountGroup({ save });
    await tick();
    await api.commit("a", 1);
    expect(save).not.toHaveBeenCalled();
  });

  it("fieldErrors from the backend land under the field and keep the value", async () => {
    const { w, api } = mountGroup({ save: () => Promise.resolve(bad("Invalid", [{ field: "a", message: "Must be even" }])) });
    await tick();
    await api.commit("a", 3);
    await tick();
    expect(w.find(".field-error").text()).toBe("Must be even");
    expect(w.find(".callout").exists()).toBe(false);
    expect((w.find("#a").element as HTMLInputElement).value).toBe("3");
  });

  it("any other error shows a callout, Retry resends the same change, and success clears it", async () => {
    const save = vi.fn().mockResolvedValueOnce(bad("disk full")).mockImplementation((d: Doc) => Promise.resolve(good(d)));
    const { w, api } = mountGroup({ save });
    await tick();
    await api.commit("a", 7);
    await tick();
    expect(w.find(".callout.danger").text()).toContain("Couldn't save Test settings");
    expect(w.find("details pre").text()).toBe("disk full");
    expect((w.find("#a").element as HTMLInputElement).value).toBe("7");
    await w.findAll("button").find((b) => b.text() === "Retry")!.trigger("click");
    await tick();
    expect(save).toHaveBeenCalledTimes(2);
    expect(save).toHaveBeenLastCalledWith({ a: 7, b: 2 });
    expect(w.find(".callout").exists()).toBe(false);
  });

  it("Esc in a field puts the saved value back and clears its error", async () => {
    const { w, api } = mountGroup({ save: vi.fn(), validate: () => "Nope" });
    await tick();
    await w.find("#a").setValue("9");
    await api.commit("a", 9);
    await tick();
    expect(w.find(".field-error").exists()).toBe(true);
    await w.find("#a").trigger("keydown", { key: "Escape" });
    expect((w.find("#a").element as HTMLInputElement).value).toBe("1");
    expect(w.find(".field-error").exists()).toBe(false);
  });

  it("two commits without awaiting are saved one after the other and both are stored (A1)", async () => {
    const { save, calls } = deferred();
    const { api } = mountGroup({ save });
    await tick();
    const p1 = api.commit("a", 5);
    const p2 = api.commit("b", 6);
    expect(save).toHaveBeenCalledTimes(1); // the second waits for the first
    expect(calls[0]!.doc).toEqual({ a: 5, b: 2 });
    calls[0]!.settle(good(calls[0]!.doc));
    await tick();
    expect(save).toHaveBeenCalledTimes(2);
    expect(calls[1]!.doc).toEqual({ a: 5, b: 6 }); // merged into the latest saved document
    calls[1]!.settle(good(calls[1]!.doc));
    expect(await Promise.all([p1, p2])).toEqual([true, true]);
    expect(api.doc).toEqual({ a: 5, b: 6 });
  });

  it("says Saving… only after 400 ms, then Saved for 2 s, in a polite live region", async () => {
    const { save, calls } = deferred();
    const { w, api } = mountGroup({ save });
    await tick();
    expect(w.find(".caption").attributes("aria-live")).toBe("polite");
    void api.commit("a", 5);
    await vi.advanceTimersByTimeAsync(399);
    expect(w.find(".caption").text()).toBe("");
    await vi.advanceTimersByTimeAsync(1);
    expect(w.find(".caption").text()).toBe("Saving…");
    calls[0]!.settle(good(calls[0]!.doc));
    await tick();
    expect(w.find(".caption").text()).toBe("Saved");
    await vi.advanceTimersByTimeAsync(1999);
    expect(w.find(".caption").text()).toBe("Saved");
    await vi.advanceTimersByTimeAsync(1);
    expect(w.find(".caption").text()).toBe("");
  });

  it("a quick save never shows Saving…", async () => {
    const save = vi.fn((d: Doc) => Promise.resolve(good(d)));
    const { w, api } = mountGroup({ save });
    await tick();
    await api.commit("a", 5);
    expect(w.find(".caption").text()).toBe("Saved");
    await vi.advanceTimersByTimeAsync(1000);
    expect(w.find(".caption").text()).toBe("Saved"); // the 400 ms timer was cleared, nothing flips to Saving…
  });
});
