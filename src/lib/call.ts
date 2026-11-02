import type { CommandError } from "../bindings";

/** Result of a backend call, flattened so stores never touch `status`. */
export type Outcome<T> = { ok: true; data: T } | { ok: false; error: CommandError };

export type Raw<T> = { status: "ok"; data: T } | { status: "error"; error: CommandError };

/** Unwraps a bindings result; thrown runtime errors become a CommandError too. */
export async function call<T>(p: Promise<Raw<T>>): Promise<Outcome<T>> {
  try {
    const r = await p;
    return r.status === "ok" ? { ok: true, data: r.data } : { ok: false, error: r.error };
  } catch (e) {
    return { ok: false, error: { message: e instanceof Error ? e.message : String(e), fieldErrors: [] } };
  }
}

export const TEMPLATE_LABELS = {
  "tauri-vue": "Rust + Tauri 2 + Vue 3 + TypeScript",
  existing: "Imported folder",
} as const;
