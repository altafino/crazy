import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import type { AppDbOverview, BackupView, Cell, DataGuard, PreviewStatus, RowsPage } from "../bindings";

const { commands } = vi.hoisted(() => ({ commands: {} as Record<string, ReturnType<typeof vi.fn>> }));
vi.mock("../bindings", () => ({
  commands,
  events: { previewStatusEvent: { listen: vi.fn(async () => () => {}) }, previewLogEvent: { listen: vi.fn(async () => () => {}) } },
}));

import { ok, err, project } from "../test-helpers";
import { useNavStore } from "../stores/nav";
import { usePreviewStore } from "../stores/preview";
import { useToastStore } from "../stores/toast";
import DatabaseTab from "./DatabaseTab.vue";

const T = (name: string, rowCount: number | null, manyRows = false) => ({ name, rowCount, manyRows });
const ov = (over: Partial<AppDbOverview> = {}): AppDbOverview => ({
  which: "preview", state: "ok", error: null, path: "/home/u/.local/share/dev.studio.notes/dev/app.db", sizeBytes: 151_552, schemaVersion: 4,
  modifiedAt: new Date(Date.now() - 2 * 60_000).toISOString(), tables: [T("scores", 42), T("settings", 3), T("levels", 12)], ...over,
});
const text = (value: string): Cell => ({ kind: "text", value });
const int = (value: string): Cell => ({ kind: "integer", value });
const pg = (over: Partial<RowsPage> = {}): RowsPage => ({
  table: "scores", columns: ["id", "player", "points"], rows: [[int("1"), text("Ada"), int("12400")], [int("2"), text("Grace"), { kind: "null" }]],
  offset: 0, pageSize: 50, total: 42, totalOverLimit: false, timedOut: false, ...over,
});
const guard = (over: Partial<DataGuard> = {}): DataGuard => ({ previewRunning: false, installedRunning: false, appName: "Notes", ...over });
const bk = (over: Partial<BackupView> = {}): BackupView => ({
  id: "b1", which: "preview", reason: "manual", releaseId: null, label: null, sizeBytes: 150_000, schemaVersion: 4,
  createdAt: new Date(2026, 10, 3, 14, 2).toISOString(), fileExists: true, ...over,
});

async function setup(o: { overview?: AppDbOverview | ((w: string) => AppDbOverview); rows?: RowsPage | ((table: string, filter: string | null) => RowsPage); guard?: DataGuard; backups?: BackupView[] } = {}) {
  commands.appdbOverview!.mockImplementation((_p: string, w: string) => ok(typeof o.overview === "function" ? o.overview(w) : o.overview ?? ov()));
  commands.appdbRows!.mockImplementation((_p: string, _w: string, t: string, off: number, f: string | null) =>
    ok(typeof o.rows === "function" ? o.rows(t, f) : { ...(o.rows ?? pg()), table: t, offset: off }));
  commands.appdbGuard!.mockImplementation(() => ok(o.guard ?? guard()));
  commands.appdbListBackups!.mockImplementation(() => ok(o.backups ?? []));
  const pinia = createPinia();
  setActivePinia(pinia);
  const nav = useNavStore();
  nav.openProject("p1");
  nav.tab = "database";
  const w = mount(DatabaseTab, { props: { project: project() }, global: { plugins: [pinia] }, attachTo: document.body });
  await flushPromises();
  return { w, nav, toast: useToastStore(), preview: usePreviewStore() };
}
type W = Awaited<ReturnType<typeof setup>>["w"];
const btn = (w: W, label: string) => w.findAll("button").find((b) => b.text() === label);
const dlg = (label: string) => [...document.querySelectorAll<HTMLButtonElement>("[role='dialog'] button")].find((b) => b.textContent?.trim() === label);
const dlgText = () => document.querySelector("[role='dialog']")?.textContent ?? "";

beforeEach(() => {
  document.body.innerHTML = "";
  Object.assign(commands, {
    appdbOverview: vi.fn(), appdbRows: vi.fn(), appdbGuard: vi.fn(), appdbListBackups: vi.fn(), appdbBackup: vi.fn(), appdbDeleteBackup: vi.fn(),
    appdbRestoreBackup: vi.fn(), appdbCopyInstalledToPreview: vi.fn(() => ok(null)), appdbResetPreview: vi.fn(() => ok(null)),
    getPreviewSettings: vi.fn(() => ok({ maxLines: 8000, maxLogMb: 20, keepLogs: 7 })), getPreviewStatus: vi.fn(() => ok(null)),
    launchInstalled: vi.fn(() => ok(null)), startRun: vi.fn(),
  });
});
afterEach(() => vi.useRealTimers());

describe("states (DR-6)", () => {
  it("no database: the one-line explanation and Ask Claude to add a database, which only fills the composer", async () => {
    const { w, nav } = await setup({ overview: ov({ state: "noDatabase", path: null, tables: [] }) });
    expect(w.text()).toContain("This app doesn't store data yet. A database keeps things like scores, notes or settings between runs.");
    expect(w.find("[role='radiogroup']").exists()).toBe(false);
    await btn(w, "Ask Claude to add a database")!.trigger("click");
    const h = nav.composerHandoff!;
    expect(h.projectId).toBe("p1");
    expect(h.text).toContain("src-tauri/migrations");
    expect(h.text).toContain("rusqlite");
    expect(h.text).toContain("one SQLite file");
    expect(nav.tab).toBe("chat");
    expect(commands.startRun).not.toHaveBeenCalled();
  });

  it("Preview never ran: Open Preview", async () => {
    const { w, nav } = await setup({ overview: ov({ state: "noFile", tables: [] }) });
    expect(w.text()).toContain("No preview data yet. Run the app in Preview to create it.");
    await btn(w, "Open Preview")!.trigger("click");
    expect(nav.tab).toBe("preview");
  });

  it("not installed: Open Releases", async () => {
    const { w, nav } = await setup({ overview: (which) => ov(which === "installed" ? { which: "installed", state: "notInstalled", tables: [] } : {}) });
    await w.findAll("input[name='db-which']")[1]!.setValue(true);
    await flushPromises();
    expect(w.text()).toContain("Not installed. Install a release to see its data.");
    await btn(w, "Open Releases")!.trigger("click");
    expect(nav.tab).toBe("releases");
  });

  it("installed, never opened: Launch", async () => {
    const { w } = await setup({ overview: (which) => ov({ which: which as "preview", state: which === "installed" ? "noFile" : "ok" }) });
    await w.findAll("input[name='db-which']")[1]!.setValue(true);
    await flushPromises();
    expect(w.text()).toContain("The installed app hasn't created its data yet. Open it once.");
    await btn(w, "Launch")!.trigger("click");
    expect(commands.launchInstalled).toHaveBeenCalledWith("p1");
  });

  it("damaged: danger callout, raw error under Details, Ask Claude to fix fills the composer", async () => {
    const { w, nav } = await setup({ overview: ov({ state: "damaged", error: "file is not a database", tables: [] }) });
    const c = w.find(".callout.danger");
    expect(c.text()).toContain("Studio can't read this database");
    expect(c.find("details summary").text()).toBe("Details");
    expect(c.find("details pre").text()).toBe("file is not a database");
    await btn(w, "Ask Claude to fix")!.trigger("click");
    expect(nav.composerHandoff!.text).toContain("file is not a database");
    expect(commands.startRun).not.toHaveBeenCalled();
  });

  it("table with 0 rows", async () => {
    const { w } = await setup({ overview: ov({ tables: [T("scores", 0)] }), rows: pg({ rows: [], total: 0 }) });
    expect(w.text()).toContain("scores has no rows yet.");
    expect(w.find("table").exists()).toBe(false);
  });

  it("loading: text first, spinner only after 400 ms, layout kept", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"] });
    let done: (v: unknown) => void = () => {};
    commands.appdbOverview!.mockImplementation(() => new Promise((r) => (done = r)));
    commands.appdbGuard!.mockImplementation(() => ok(guard()));
    const pinia = createPinia();
    setActivePinia(pinia);
    const w = mount(DatabaseTab, { props: { project: project() }, global: { plugins: [pinia] }, attachTo: document.body });
    await flushPromises();
    expect(w.text()).toContain("Reading preview data…");
    expect(w.find(".spin").exists()).toBe(false);
    expect(w.find("[role='listbox']").exists()).toBe(true);
    await vi.advanceTimersByTimeAsync(400);
    expect(w.find(".spin").exists()).toBe(true);
    done(await ok(ov()));
    commands.appdbRows!.mockImplementation(() => ok(pg()));
    await flushPromises();
    expect(w.find(".spin").exists()).toBe(false);
    expect(w.text()).toContain("Preview data · SQLite · 3 tables · 148 KB");
  });
});

describe("status row and tables", () => {
  it("shows the sentence, caption and a Refresh icon button that re-reads", async () => {
    const { w } = await setup();
    expect(w.find(".sentence").text()).toBe("Preview data · SQLite · 3 tables · 148 KB");
    const cap = w.find(".caption").text();
    expect(cap).toContain("/home/u/.local/share/dev.studio.notes/dev/app.db");
    expect(cap).toContain("schema version 4");
    expect(cap).toContain("updated 2 minutes ago");
    expect(w.find("[aria-label='Refresh data']").exists()).toBe(true);
    commands.appdbOverview!.mockClear();
    await w.find("[aria-label='Refresh data']").trigger("click");
    await flushPromises();
    expect(commands.appdbOverview).toHaveBeenCalledWith("p1", "preview");
    expect(w.find("[role='status']").text()).toBe("Data refreshed");
  });

  it("re-reads when Preview stops", async () => {
    const { preview } = await setup();
    const st = (live: boolean) => ({ projectId: "p1", live }) as unknown as PreviewStatus;
    preview.status.p1 = st(true);
    await flushPromises();
    commands.appdbOverview!.mockClear();
    preview.status.p1 = st(false);
    await flushPromises();
    expect(commands.appdbOverview).toHaveBeenCalledTimes(1);
  });

  it("tables listbox: options are named, one Tab stop, arrows move and show the table", async () => {
    const { w } = await setup({ overview: ov({ tables: [T("scores", 42), T("settings", 1), T("events", null, true)] }) });
    const list = w.find("[role='listbox']");
    expect(list.attributes("aria-label")).toBe("Tables");
    expect(list.attributes("tabindex")).toBe("0");
    const opts = w.findAll("[role='option']");
    expect(opts.map((o) => o.attributes("aria-label"))).toEqual(["scores, 42 rows", "settings, 1 row", "events, many rows"]);
    expect(opts[0]!.attributes("aria-selected")).toBe("true");
    expect(opts.every((o) => o.attributes("tabindex") === undefined)).toBe(true);
    commands.appdbRows!.mockClear();
    await list.trigger("keydown", { key: "ArrowDown" });
    await flushPromises();
    expect(commands.appdbRows).toHaveBeenLastCalledWith("p1", "preview", "settings", 0, null);
    expect(list.attributes("aria-activedescendant")).toBe(opts[1]!.attributes("id"));
    expect(w.findAll("[role='option']")[1]!.attributes("aria-selected")).toBe("true");
    await list.trigger("keydown", { key: "ArrowUp" });
    await list.trigger("keydown", { key: "ArrowUp" });
    await flushPromises();
    expect(commands.appdbRows).toHaveBeenLastCalledWith("p1", "preview", "scores", 0, null);
  });

  it("narrow layout: a Table select reads 'scores · 42 rows' and switches the table", async () => {
    const { w } = await setup();
    const sel = w.find("select[aria-label='Table']");
    expect(sel.findAll("option").map((o) => o.text())).toEqual(["scores · 42 rows", "settings · 3 rows", "levels · 12 rows"]);
    await sel.setValue("levels");
    await flushPromises();
    expect(commands.appdbRows).toHaveBeenLastCalledWith("p1", "preview", "levels", 0, null);
  });
});

describe("rows (DR-10, DR-13, R7)", () => {
  it("is a semantic table with a caption, column headers, NULL and BLOB placeholders, right-aligned numbers", async () => {
    const long = "x".repeat(2500);
    const { w } = await setup({
      rows: pg({ columns: ["id", "note", "data", "x"], rows: [[int("1"), text(long), { kind: "blob", size: 2150 }, { kind: "null" }]] }),
    });
    expect(w.find("table caption").text()).toBe("scores, 42 rows, showing 1 to 1");
    const ths = w.findAll("th");
    expect(ths.map((t) => t.text())).toEqual(["id", "note", "data", "x"]);
    expect(ths.every((t) => t.attributes("scope") === "col")).toBe(true);
    const tds = w.findAll("td");
    expect(tds[0]!.classes()).toContain("number");
    expect(tds[1]!.text()).toBe(long);
    expect(tds[1]!.attributes("title")).toHaveLength(2000);
    expect(tds[2]!.text()).toBe("BLOB · 2.1 KB");
    expect(tds[2]!.classes()).toContain("blob");
    expect(tds[3]!.text()).toBe("NULL");
    expect(tds[3]!.classes()).toContain("null");
  });

  it("pages 50 at a time with named pager buttons and a visible range", async () => {
    const { w } = await setup({ overview: ov({ tables: [T("scores", 4210)] }), rows: pg({ total: 4210, rows: Array.from({ length: 50 }, (_, i) => [int(String(i + 1))]), columns: ["id"] }) });
    expect(w.find(".pager").text()).toContain("Rows 1–50 of 4 210");
    expect((w.find("button:disabled").element as HTMLElement).textContent).toBe("Previous page");
    const next = w.find("[aria-label='Next page, rows 51 to 100']");
    expect(next.exists()).toBe(true);
    await next.trigger("click");
    await flushPromises();
    expect(commands.appdbRows).toHaveBeenLastCalledWith("p1", "preview", "scores", 50, null);
    expect(w.find(".pager").text()).toContain("Rows 51–100 of 4 210");
    expect(w.findAll("button").some((b) => b.text() === "Previous page" && !b.attributes("disabled"))).toBe(true);
  });

  it("a count over the limit reads 'many rows'", async () => {
    const { w } = await setup({ overview: ov({ tables: [T("big", null, true)] }), rows: pg({ table: "big", total: null, totalOverLimit: true }) });
    expect(w.find("table caption").text()).toContain("many rows");
    expect(w.find(".pager").text()).toContain("of many");
    expect(w.find("[role='option']").attributes("aria-label")).toBe("big, many rows");
  });

  it("filter: named 'Filter rows', debounced 300 ms, searches with the text", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const { w } = await setup();
    const input = w.find("input[type='search']");
    expect(w.text()).toContain("Filter rows");
    commands.appdbRows!.mockClear();
    await input.setValue("ada");
    vi.advanceTimersByTime(299);
    expect(commands.appdbRows).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    await flushPromises();
    expect(commands.appdbRows).toHaveBeenCalledWith("p1", "preview", "scores", 0, "ada");
  });

  it("no match: 'No rows match' with Clear filter", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const { w } = await setup({ rows: (t, f) => (f ? pg({ table: t, rows: [], total: 0 }) : pg({ table: t })) });
    await w.find("input[type='search']").setValue("ada");
    vi.advanceTimersByTime(300);
    await flushPromises();
    expect(w.text()).toContain('No rows match "ada".');
    expect(w.text()).not.toContain("has no rows yet");
    await btn(w, "Clear filter")!.trigger("click");
    await flushPromises();
    expect(commands.appdbRows).toHaveBeenLastCalledWith("p1", "preview", "scores", 0, null);
    expect((w.find("input[type='search']").element as HTMLInputElement).value).toBe("");
    expect(w.find("table").exists()).toBe(true);
  });

  it("a filter over the time limit asks to narrow it", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    const { w } = await setup({ rows: (t, f) => (f ? pg({ table: t, rows: [], total: null, timedOut: true }) : pg({ table: t })) });
    await w.find("input[type='search']").setValue("a");
    vi.advanceTimersByTime(300);
    await flushPromises();
    expect(w.text()).toContain("Filter took too long; narrow it");
  });

  it("a table over the time limit links to Settings › Limits", async () => {
    const { w } = await setup({ rows: (t) => pg({ table: t, rows: [], total: null, timedOut: true }) });
    expect(w.text()).toContain("took too long");
    await w.get("button.limits").trigger("click");
    const nav = useNavStore();
    expect(nav.view).toBe("settings");
    expect(nav.settingsSection).toBe("limits");
  });
});

describe("switch, menu and actions (DR-3, DR-5, DR-11, DR-15)", () => {
  it("defaults to Preview, switches to Installed with an announcement, and hides Reset", async () => {
    const { w } = await setup({ overview: (which) => ov({ which: which as "preview" }) });
    const radios = w.findAll("input[name='db-which']");
    expect(w.find("[role='radiogroup']").attributes("aria-label")).toBe("Data");
    expect((radios[0]!.element as HTMLInputElement).checked).toBe(true);
    expect(w.text()).toContain("Reset preview data…");
    await radios[1]!.setValue(true);
    await flushPromises();
    expect(commands.appdbOverview).toHaveBeenLastCalledWith("p1", "installed");
    expect(w.find(".sentence").text()).toContain("Installed app data · SQLite");
    expect(w.find("[role='status']").text()).toBe("Showing installed app data");
    expect(w.text()).not.toContain("Reset preview data…");
    expect(w.text()).toContain("Copy installed data to Preview…");
  });

  it("Back up… is the primary; ⋯ holds Copy and Reset", async () => {
    const { w } = await setup();
    expect(btn(w, "Back up…")!.classes()).toContain("primary");
    expect(w.find("summary[aria-label='More data actions']").exists()).toBe(true);
    expect(w.findAll(".pop button").map((b) => b.text())).toEqual(["Copy installed data to Preview…", "Reset preview data…"]);
    expect(w.find(".pop .danger").text()).toBe("Reset preview data…");
  });

  it("while Preview runs, Copy and Reset are disabled with 'Stop Preview first' tied by aria-describedby", async () => {
    const { w } = await setup({ guard: guard({ previewRunning: true }) });
    const items = w.findAll(".pop button");
    expect(items.every((b) => b.attributes("disabled") !== undefined)).toBe(true);
    const why = w.find("#db-why");
    expect(why.text()).toBe("Stop Preview first");
    expect(items.every((b) => b.attributes("aria-describedby") === "db-why")).toBe(true);
  });

  it("while the installed app runs, its data is blocked with 'Quit Notes first'", async () => {
    const { w } = await setup({ guard: guard({ installedRunning: true }), overview: (which) => ov({ which: which as "preview" }) });
    await w.findAll("input[name='db-which']")[1]!.setValue(true);
    await flushPromises();
    expect(w.find("#db-why").text()).toBe("Quit Notes first");
  });

  it("Reset: confirm names what is deleted, then toasts and re-reads", async () => {
    const { w, toast } = await setup();
    await btn(w, "Reset preview data…")!.trigger("click");
    expect(dlgText()).toContain("Delete preview data?");
    expect(dlgText()).toContain("57 rows in 3 tables");
    await dlg("Cancel")!.click();
    await flushPromises();
    expect(commands.appdbResetPreview).not.toHaveBeenCalled();
    await btn(w, "Reset preview data…")!.trigger("click");
    await dlg("Delete preview data")!.click();
    await flushPromises();
    expect(commands.appdbResetPreview).toHaveBeenCalledWith("p1");
    expect(toast.current!.text).toBe("Preview data deleted");
    expect(w.find("[role='status']").text()).toBe("Preview data deleted");
  });

  it("Copy: confirm, then 'Preview data replaced'", async () => {
    const { w, toast } = await setup();
    await btn(w, "Copy installed data to Preview…")!.trigger("click");
    expect(dlgText()).toContain("Preview data becomes a copy of the installed app's data");
    await dlg("Replace preview data")!.click();
    await flushPromises();
    expect(commands.appdbCopyInstalledToPreview).toHaveBeenCalledWith("p1");
    expect(toast.current!.text).toBe("Preview data replaced");
  });

  it("a refused action shows the backend message and changes nothing", async () => {
    commands.appdbResetPreview!.mockImplementation(() => err("Stop Preview first."));
    const { w, toast } = await setup();
    await btn(w, "Reset preview data…")!.trigger("click");
    await dlg("Delete preview data")!.click();
    await flushPromises();
    expect(toast.current!.text).toBe("Stop Preview first.");
  });

  it("Back up…: empty list wording, Back up now toasts 'Backed up preview data'", async () => {
    commands.appdbBackup!.mockImplementation(() => ok(bk()));
    const { w, toast } = await setup();
    await btn(w, "Back up…")!.trigger("click");
    await flushPromises();
    expect(dlgText()).toContain("Backups of Preview data");
    expect(dlgText()).toContain("No backups yet.");
    await dlg("Back up now")!.click();
    await flushPromises();
    expect(commands.appdbBackup).toHaveBeenCalledWith("p1", "preview");
    expect(toast.current!.text).toBe("Backed up preview data");
    expect(w.find("[role='status']").text()).toBe("Backed up preview data");
  });

  it("backup list: label or date, reason, size; Delete… only for manual and restore backups", async () => {
    const list = [
      bk({ id: "m", reason: "manual" }),
      bk({ id: "r", reason: "release", label: "before 1.1.0", which: "installed" }),
      bk({ id: "s", reason: "restore", label: "before restore" }),
    ];
    const { w } = await setup({ backups: list });
    await btn(w, "Back up…")!.trigger("click");
    await flushPromises();
    const items = [...document.querySelectorAll("[role='dialog'] li")];
    expect(items).toHaveLength(3);
    expect(items[0]!.textContent).toContain("Backup from 3 Nov 14:02");
    expect(items[0]!.textContent).toContain("Manual");
    expect(items[0]!.textContent).toContain("146 KB");
    expect(items[1]!.textContent).toContain("before 1.1.0");
    expect(items.map((li) => !!li.querySelector("button[aria-label^='Delete']"))).toEqual([true, false, true]);
  });

  it("Restore: confirm text per DR-15, button 'Replace data', then 'before restore' note and a toast", async () => {
    commands.appdbRestoreBackup!.mockImplementation(() => ok(bk({ id: "x", reason: "restore" })));
    const { w, toast } = await setup({
      overview: (which) => ov({ which: which as "preview" }),
      backups: [bk({ which: "installed" })],
    });
    await w.findAll("input[name='db-which']")[1]!.setValue(true);
    await flushPromises();
    await btn(w, "Back up…")!.trigger("click");
    await flushPromises();
    (document.querySelector("button[aria-label^='Restore']") as HTMLButtonElement).click();
    await flushPromises();
    expect(dlgText()).toContain("Replace installed app data with the backup from 3 Nov 14:02?");
    expect(dlgText()).toContain("before restore");
    await dlg("Replace data")!.click();
    await flushPromises();
    expect(commands.appdbRestoreBackup).toHaveBeenCalledWith("p1", "b1");
    expect(toast.current!.text).toBe("Installed app data replaced");
    expect(document.querySelector("[role='dialog']")).toBeNull();
  });

  it("Restore is disabled while the app using that file runs, with the caption tied to it", async () => {
    const { w } = await setup({ guard: guard({ previewRunning: true }), backups: [bk()] });
    await btn(w, "Back up…")!.trigger("click");
    await flushPromises();
    const r = document.querySelector("button[aria-label^='Restore']") as HTMLButtonElement;
    expect(r.disabled).toBe(true);
    expect(r.getAttribute("aria-describedby")).toBe("bk-why");
    expect(document.getElementById("bk-why")!.textContent).toBe("Stop Preview first");
  });

  it("Delete… confirms first and removes only after confirming", async () => {
    commands.appdbDeleteBackup!.mockImplementation(() => ok(null));
    const { w } = await setup({ backups: [bk()] });
    await btn(w, "Back up…")!.trigger("click");
    await flushPromises();
    (document.querySelector("button[aria-label^='Delete']") as HTMLButtonElement).click();
    await flushPromises();
    expect(commands.appdbDeleteBackup).not.toHaveBeenCalled();
    await dlg("Delete backup")!.click();
    await flushPromises();
    expect(commands.appdbDeleteBackup).toHaveBeenCalledWith("p1", "b1");
  });

  it("Ask Claude about this data fills the composer and runs nothing", async () => {
    const { w, nav } = await setup();
    await btn(w, "Ask Claude about this data")!.trigger("click");
    expect(nav.composerHandoff).toEqual({ projectId: "p1", text: 'About the preview data, table "scores": ' });
    expect(nav.tab).toBe("chat");
    expect(commands.startRun).not.toHaveBeenCalled();
  });
});
