import { open } from "@tauri-apps/plugin-dialog";

/** Opens a native folder or file picker; resolves to null when cancelled. */
export async function pickPath(directory: boolean, defaultPath?: string): Promise<string | null> {
  const r = await open({ directory, multiple: false, defaultPath: defaultPath || undefined });
  return typeof r === "string" ? r : null;
}
