import { getVersion } from "@tauri-apps/api/app";
import { commands } from "../bindings";
import { call } from "./call";
import { saveNotifyFinish, setNotifyFinish } from "./notify";
import { useNavStore } from "../stores/nav";
import { useToastStore } from "../stores/toast";
import { useUpdateStore } from "../stores/update";

/** Where 0.2.0 kept the finish-notification switch (moved to the backend, D8). */
const OLD_NOTIFY_KEY = "studio.notifyFinish";

/**
 * Startup state from the backend (D8, D9, eng D3): the finish-notification switch, the one-time copy
 * of the 0.2.0 localStorage switch, and the "Updated to X" toast once per new version.
 */
export async function startUiState(): Promise<void> {
  const r = await call(commands.getUiState());
  if (!r.ok) {
    console.warn("Couldn't read the UI state:", r.error.message);
    return;
  }
  setNotifyFinish(r.data.notifyFinish);
  useUpdateStore().setAuto(r.data.checkUpdates !== false); // a newer release: now, then every 24 hours
  await copyOldNotifySwitch(r.data.notifyFinish);
  await whatsNew(r.data.lastSeenVersion ?? null);
}

/** A 0.2.0 "off" is stored once; the key is removed only after that worked, so a failure retries next start. */
async function copyOldNotifySwitch(on: boolean) {
  let old: string | null;
  try {
    old = localStorage.getItem(OLD_NOTIFY_KEY);
  } catch {
    return;
  }
  if (old === null) return;
  if (old === "off" && on && !(await saveNotifyFinish(false)).ok) return;
  try {
    localStorage.removeItem(OLD_NOTIFY_KEY);
  } catch {
    /* storage unavailable */
  }
}

/**
 * Missing (a fresh install; migration 0010 marks databases from 0.2.0): store the version quietly.
 * Different: store it, then offer What's new. A failed store shows nothing, so the toast never repeats every start.
 */
async function whatsNew(lastSeen: string | null) {
  const version = await getVersion().catch(() => null);
  if (!version || lastSeen === version) return;
  if (!(await call(commands.setLastSeenVersion(version))).ok) return;
  if (lastSeen === null) return;
  useToastStore().show(`Updated to ${version}`, { label: "What's new", run: () => useNavStore().openReleaseNotes() });
}
