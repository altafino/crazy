import { reactive } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { commands } from "../bindings";
import { call } from "./call";

/** Project id -> app icon as a data URL (null: none). Shared, so a change shows everywhere at once. */
const icons = reactive<Record<string, string | null>>({});
const loading = new Set<string>();

/** The cached icon; the first call for a project loads it. */
export function iconOf(id: string): string | null {
  if (!(id in icons) && !loading.has(id)) {
    loading.add(id);
    void call(commands.projectIcon(id)).then((r) => {
      icons[id] = r.ok ? r.data : null;
      loading.delete(id);
    });
  }
  return icons[id] ?? null;
}

/** Reloads on next use (Claude may have changed the files). */
export function forgetIcon(id: string) {
  delete icons[id];
}

export const ICON_EXTENSIONS = ["png", "jpg", "jpeg", "webp", "svg"];

/** The file picker for an icon picture; null when cancelled. */
export async function pickPicture(): Promise<string | null> {
  const path = await open({
    multiple: false,
    directory: false,
    title: "Choose an app icon",
    filters: [{ name: "Images", extensions: ICON_EXTENSIONS }],
  });
  return typeof path === "string" ? path : null;
}

/** Opens the picker and replaces the icon. `changed: false` without error means the user cancelled. */
export async function pickIcon(id: string): Promise<{ changed: boolean; error?: string }> {
  const path = await pickPicture();
  if (!path) return { changed: false };
  const r = await call(commands.setProjectIcon(id, path));
  if (!r.ok) return { changed: false, error: r.error.message };
  icons[id] = r.data;
  return { changed: true };
}
