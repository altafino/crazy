import { ref } from "vue";
import { defineStore } from "pinia";
import type { CraftAppsView } from "../bindings";

/** App names from the backend's `craftApps` view, remembered so the timeline, approval cards and
 * notifications can say "PhotoCraft" without a second name list (design D24). */
export const useCraftsStore = defineStore("crafts", () => {
  const names = ref<Record<string, string>>({});
  function remember(v: CraftAppsView) {
    for (const a of [...v.apps, ...v.notInstalled]) names.value[a.id] = a.name;
  }
  /** Unknown app (uninstalled, or the view not loaded yet): the id, capitalised. */
  const nameOf = (id: string) => names.value[id] ?? id.charAt(0).toUpperCase() + id.slice(1);
  return { names, remember, nameOf };
});
