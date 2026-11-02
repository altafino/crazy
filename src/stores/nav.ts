import { ref } from "vue";
import { defineStore } from "pinia";

export type View = "projects" | "doctor" | "settings" | "project";

export const useNavStore = defineStore("nav", () => {
  const view = ref<View>("projects");
  /** The open project while `view === "project"` (D46). */
  const projectId = ref<string | null>(null);
  /** Settings section to show on arrival ("Set up Claude first" lands on AI Agents, D52). */
  const settingsSection = ref<"general" | "agents">("general");

  function go(v: View) {
    view.value = v;
    if (v !== "project") projectId.value = null;
  }
  function openProject(id: string) {
    projectId.value = id;
    view.value = "project";
  }
  function openAgentSettings() {
    settingsSection.value = "agents";
    go("settings");
  }
  return { view, projectId, settingsSection, go, openProject, openAgentSettings };
});
