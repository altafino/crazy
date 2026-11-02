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

  /** Saves, then re-runs the doctor (forced) because overrides change tool resolution. */
  async function save(next: Settings) {
    saving.value = true;
    error.value = null;
    const r = await call(commands.saveSettings(next));
    saving.value = false;
    if (!r.ok) {
      error.value = r.error;
      return r;
    }
    settings.value = r.data;
    await useDoctorStore().run(true);
    return r;
  }

  return { settings, loading, saving, error, load, save };
});
