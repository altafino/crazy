import type { BackupView, Cell, DataSet, TableInfo } from "../bindings";

export const SET_LABEL: Record<DataSet, string> = { preview: "Preview data", installed: "Installed app data" };
/** Lower-case form for sentences: "Back up preview data". */
export const setName = (w: DataSet) => (w === "preview" ? "preview data" : "installed app data");

const plural = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString("en-US")} ${n === 1 ? one : many}`;
/** "4 210": a space as thousands separator, as the plan words it. */
export const num = (n: number) => n.toLocaleString("en-US").replace(/,/g, " ");

export const rowsLabel = (t: TableInfo) => (t.manyRows || t.rowCount == null ? "many rows" : plural(t.rowCount, "row"));

/** "42 rows in 3 tables" for the Reset confirm (DR-5). */
export function contents(tables: TableInfo[]): string {
  const many = tables.some((t) => t.manyRows || t.rowCount == null);
  const rows = tables.reduce((n, t) => n + (t.rowCount ?? 0), 0);
  return `${many ? "Many rows" : plural(rows, "row")} in ${plural(tables.length, "table")}`;
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
/** "3 Nov 14:02" (DR-15). */
export function shortDate(iso: string): string {
  const d = new Date(iso);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getDate()} ${MONTHS[d.getMonth()]} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

export const REASON_TEXT: Record<BackupView["reason"], string> = { manual: "Manual", release: "Automatic, before a release switch", restore: "Before a restore" };
/** Automatic release backups follow their release (DR-8); the others may be deleted. */
export const deletable = (b: BackupView) => b.reason !== "release";

export const TOOLTIP_MAX = 2000;

export type Shown = { text: string; kind: "text" | "number" | "null" | "blob"; title?: string };
/** How one cell reads in the table (R7, DR-10). The full text stays in the DOM; the tooltip is capped. */
export function show(c: Cell): Shown {
  switch (c.kind) {
    case "null":
      return { text: "NULL", kind: "null" };
    case "blob":
      return { text: c.size == null ? "BLOB" : `BLOB · ${formatBlob(c.size)}`, kind: "blob" };
    case "integer":
      return { text: c.value, kind: "number" };
    case "real":
      return { text: c.value == null ? "NaN" : String(c.value), kind: "number" };
    default:
      return { text: c.value, kind: "text", title: c.value.slice(0, TOOLTIP_MAX) };
  }
}
function formatBlob(bytes: number): string {
  const units = ["B", "KB", "MB", "GB"];
  let n = bytes;
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return `${i > 0 && n < 10 ? Math.round(n * 10) / 10 : Math.round(n)} ${units[i]}`;
}

// --- prepared Claude requests (T6): they fill the composer; nothing runs until Run ---

/** The convention, in one place for the prepared requests (the template and Claude's rules follow the same). */
const CONVENTION =
  "Follow this convention: one SQLite file in the app's data folder (app.db for the installed app, dev/app.db for debug builds, " +
  "so Preview never touches the installed app's data); numbered migrations in src-tauri/migrations (0001_init.sql, 0002_...), " +
  "applied at startup, with PRAGMA user_version set to the highest number; rusqlite in Rust; " +
  "the frontend calls Tauri commands and never runs SQL.";

export const addDatabasePrompt = () =>
  `Please add a database to this app so it can keep things between runs. ${CONVENTION}\n\nThe data to store: `;

export const askAboutDataPrompt = (w: DataSet, table: string | null) =>
  `About the ${setName(w)}${table ? `, table "${table}"` : ""}: `;

export const fixDatabasePrompt = (w: DataSet, path: string | null, error: string | null) =>
  `Studio can't read this app's ${setName(w)}${path ? ` (${path})` : ""}.${error ? ` SQLite says:\n\n${error}\n` : ""}\n` +
  `Please find out what is wrong and fix it in the app's code or migrations. ${CONVENTION}`;
