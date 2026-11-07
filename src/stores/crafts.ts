import { ref } from "vue";
import { defineStore } from "pinia";
import type { CraftAppsView } from "../bindings";

/** App names from the backend's `craftApps` view, remembered so the timeline, approval cards and
 * notifications can say "PhotoCraft" without a second name list (design D24). */
export const useCraftsStore = defineStore("crafts", () => {
  const names = ref<Record<string, string>>({});
  /** Per project: ticked AppImage apps the next run unpacks first ("Preparing", design D22). */
  const firstUse = ref<Record<string, string[]>>({});
  function remember(v: CraftAppsView, projectId?: string) {
    for (const a of [...v.apps, ...v.notInstalled]) names.value[a.id] = a.name;
    if (projectId) firstUse.value[projectId] = v.enabled ? v.apps.filter((a) => a.ticked && a.firstUse && !a.missing).map((a) => a.name) : [];
  }
  // ponytail: cleared even when the unpack failed, so the retry on the next run shows no
  // "Preparing" strip (the run's "didn't start" warning still names the app); re-read the view
  // after each run if that matters.
  const unpacked = (projectId: string) => delete firstUse.value[projectId];
  /** Unknown app (uninstalled, or the view not loaded yet): the id, capitalised. */
  const nameOf = (id: string) => names.value[id] ?? id.charAt(0).toUpperCase() + id.slice(1);
  return { names, firstUse, remember, unpacked, nameOf };
});
