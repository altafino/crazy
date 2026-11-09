import { ref } from "vue";
import { defineStore } from "pinia";
import type { SessionMode } from "../bindings";

export type View = "projects" | "installed" | "doctor" | "settings" | "help" | "about" | "project";
/** The one list of Settings tabs; deep links (`settingsSection`) and the tab row both read it. */
export const SETTINGS_TABS = [
  { id: "general", label: "General" },
  { id: "agents", label: "AI Agents" },
  { id: "approvals", label: "Approvals" },
  { id: "limits", label: "Limits" },
  { id: "tools", label: "Tools" },
] as const;
export type SettingsSection = (typeof SETTINGS_TABS)[number]["id"];
export type ProjectTab = "chat" | "plan" | "changes" | "source" | "preview" | "releases" | "github" | "database";

/** Chat end row "Commit…" (M3-D42): Source pre-ticks these files and drafts the message. */
export type CommitHandoff = { projectId: string; paths: string[]; message: string };
/** "Ask Claude to fix" (M3-D43): Chat fills its composer, in Make changes unless `mode` says otherwise; nothing runs until Run. */
export type ComposerHandoff = { projectId: string; text: string; mode?: SessionMode };

/** Build dialog "Commit first…" (D11): the fields it keeps while Source commits; the dialog takes them back when it opens. */
export type BuildDraft = { projectId: string; version: string; notes: string; install: boolean };

export const useNavStore = defineStore("nav", () => {
  const view = ref<View>("projects");
  /** The open project while `view === "project"` (D46). */
  const projectId = ref<string | null>(null);
  /** The open project tab; any component may switch it. */
  const tab = ref<ProjectTab>("chat");
  /** Settings section to show on arrival ("Set up Claude first" lands on AI Agents, D52). */
  const settingsSection = ref<SettingsSection>("general");
  /** One-shot handoffs between tabs; the receiving tab clears them once applied. */
  const commitHandoff = ref<CommitHandoff | null>(null);
  const composerHandoff = ref<ComposerHandoff | null>(null);
  /** Chat end row "Build release…" (M4-DR13): the Releases tab opens the build dialog for this project. */
  const buildDialogProject = ref<string | null>(null);
  const buildDraft = ref<BuildDraft | null>(null);

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
    view, projectId, tab, settingsSection, commitHandoff, composerHandoff, buildDialogProject, buildDraft,
    go, openProject, openAgentSettings, commitFiles, askClaude, openBuildDialog,
  };
});
