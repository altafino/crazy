import { ref } from "vue";
import { defineStore } from "pinia";
import { commands } from "../bindings";
import type { CommandError, InstalledApp_Serialize } from "../bindings";
import { call } from "../lib/call";
import { useBuildsStore } from "./builds";
import { useToastStore } from "./toast";

export const useInstalledStore = defineStore("installed", () => {
  const apps = ref<InstalledApp_Serialize[]>([]);
  const loading = ref(false);
  const error = ref<CommandError | null>(null);

  async function refresh() {
    loading.value = true;
    const r = await call(commands.listInstalledApps());
    if (r.ok) {
      apps.value = r.data;
      error.value = null;
    } else error.value = r.error;
    loading.value = false;
  }

  /** Runs one backend action, then refreshes; failures land in `error` (inline callout). */
  async function act(run: () => Promise<{ ok: true } | { ok: false; error: CommandError }>, done?: string) {
    error.value = null;
    const r = await run();
    if (!r.ok) {
      error.value = r.error;
      return false;
    }
    await refresh();
    void useBuildsStore().loadInstalled(); // keeps ProjectCard's "Installed <v>" badge current
    if (done) useToastStore().show(done);
    return true;
  }

  const launch = (id: string) => act(() => call(commands.launchInstalled(id)));
  const repair = (id: string) => act(() => call(commands.repairLauncher(id)), "Launcher repaired.");
  const uninstall = (id: string, name: string) => act(() => call(commands.uninstallApp(id)), `Uninstalled ${name}.`);

  return { apps, loading, error, refresh, launch, repair, uninstall };
});
