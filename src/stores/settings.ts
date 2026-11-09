import { ref } from "vue";
import { defineStore } from "pinia";
import { commands } from "../bindings";
import type { CommandError, Settings } from "../bindings";
import { call } from "../lib/call";
import { useDoctorStore } from "./doctor";

export const useSettingsStore = defineStore("settings", () => {
  const settings = ref<Settings | null>(null);
  const loading = ref(false);
  const saving = ref(false);
  const error = ref<CommandError | null>(null);

  async function load() {
    loading.value = true;
    error.value = null;
    const r = await call(commands.getSettings());
    if (r.ok) settings.value = r.data;
    else error.value = r.error;
    loading.value = false;
    return r;
  }

  /** Saves, then re-runs the doctor (forced) only when the tool overrides changed (R7): they change tool resolution, a folder does not. */
  async function save(next: Settings) {
    saving.value = true;
    error.value = null;
    const key = (o: Record<string, string>) => JSON.stringify(Object.entries(o).sort());
    const before = key(settings.value?.toolOverrides ?? {});
    const r = await call(commands.saveSettings(next));
    saving.value = false;
    if (!r.ok) {
      error.value = r.error;
      return r;
    }
    settings.value = r.data;
    // Not awaited: the save queue must not wait on every tool's version probe.
    if (key(r.data.toolOverrides) !== before) void useDoctorStore().run(true);
    return r;
  }

  return { settings, loading, saving, error, load, save };
});
