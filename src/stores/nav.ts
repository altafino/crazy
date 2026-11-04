import { ref } from "vue";
import { defineStore } from "pinia";

export type View = "projects" | "installed" | "doctor" | "settings" | "project";
export type ProjectTab = "chat" | "changes" | "source" | "preview" | "releases" | "github";

/** Chat end row "Commit…" (M3-D42): Source pre-ticks these files and drafts the message. */
export type CommitHandoff = { projectId: string; paths: string[]; message: string };
/** "Ask Claude to fix" (M3-D43): Chat fills its composer in Make-changes mode; nothing runs until Run. */
export type ComposerHandoff = { projectId: string; text: string };

export const useNavStore = defineStore("nav", () => {
  const view = ref<View>("projects");
  /** The open project while `view === "project"` (D46). */
  const projectId = ref<string | null>(null);
  /** The open project tab; any component may switch it. */
  const tab = ref<ProjectTab>("chat");
  /** Settings section to show on arrival ("Set up Claude first" lands on AI Agents, D52). */
  const settingsSection = ref<"general" | "agents" | "approvals">("general");
  /** One-shot handoffs between tabs; the receiving tab clears them once applied. */
  const commitHandoff = ref<CommitHandoff | null>(null);
  const composerHandoff = ref<ComposerHandoff | null>(null);
  /** Chat end row "Build release…" (M4-DR13): the Releases tab opens the build dialog for this project. */
  const buildDialogProject = ref<string | null>(null);

  function go(v: View) {
    view.value = v;
    if (v !== "project") projectId.value = null;
  }
  function openProject(id: string) {
    if (projectId.value !== id) tab.value = "chat";
    projectId.value = id;
    view.value = "project";
  }
  function openAgentSettings() {
    settingsSection.value = "agents";
    go("settings");
  }
  function commitFiles(h: CommitHandoff) {
    commitHandoff.value = h;
    tab.value = "source";
  }
  function askClaude(h: ComposerHandoff) {
    composerHandoff.value = h;
    tab.value = "chat";
  }
  function openBuildDialog(id: string) {
    buildDialogProject.value = id;
    tab.value = "releases";
  }
  return {
    view, projectId, tab, settingsSection, commitHandoff, composerHandoff, buildDialogProject,
    go, openProject, openAgentSettings, commitFiles, askClaude, openBuildDialog,
  };
});
