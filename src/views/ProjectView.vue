<script setup lang="ts">
import { computed, onMounted, onUnmounted, watch } from "vue";
import ChatTab from "./ChatTab.vue";
import ChangesTab from "./ChangesTab.vue";
import SourceTab from "./SourceTab.vue";
import PreviewTab from "./PreviewTab.vue";
import ReleasesTab from "./ReleasesTab.vue";
import GitHubTab from "./GitHubTab.vue";
import DatabaseTab from "./DatabaseTab.vue";
import PlanTab from "./PlanTab.vue";
import { usePlanStore } from "../stores/plan";
import { useGithubStore } from "../stores/github";
import { useSyncStore } from "../stores/sync";
import { TEMPLATE_LABELS } from "../lib/call";
import { useNavStore, type ProjectTab } from "../stores/nav";
import { useProjectsStore } from "../stores/projects";
import { usePreviewStore } from "../stores/preview";
import { useRunsStore } from "../stores/runs";
import { runLabel } from "../lib/runs";
import AppIcon from "../components/AppIcon.vue";
import { forgetIcon, pickIcon } from "../lib/projectIcons";
import { useToastStore } from "../stores/toast";
import { ref } from "vue";

const nav = useNavStore();
const projects = useProjectsStore();
const runs = useRunsStore();
const preview = usePreviewStore();
const gh = useGithubStore();
const sync = useSyncStore();
const toast = useToastStore();
const plan = usePlanStore();

const project = computed(() => projects.projects.find((p) => p.id === nav.projectId) ?? null);
const list = computed(() => (nav.projectId ? runs.sessions[nav.projectId] ?? [] : []));
const dot = computed(() => (nav.projectId ? preview.indicatorOf(nav.projectId) : null));
const waiting = computed(() => !!nav.projectId && runs.waitingProjects.includes(nav.projectId));

const tab = computed({ get: () => nav.tab, set: (v: ProjectTab) => (nav.tab = v) });
const tabs: { id: ProjectTab; label: string }[] = [
  { id: "chat", label: "Chat" },
  { id: "plan", label: "Plan" },
  { id: "changes", label: "Changes" },
  { id: "source", label: "Source" },
  { id: "preview", label: "Preview" },
  { id: "releases", label: "Releases" },
  { id: "github", label: "GitHub" },
  { id: "database", label: "Database" },
];
// M5-DR1: "↑2 ↓1" after the GitHub label when a remote exists; a waiting dot while signing in.
const syncCount = computed(() => {
  const r = nav.projectId ? sync.status[nav.projectId]?.remote : null;
  if (!r?.upstream || (!r.ahead && !r.behind)) return null;
  const text = [r.ahead && `↑${r.ahead}`, r.behind && `↓${r.behind}`].filter(Boolean).join(" ");
  const name = [r.ahead && `${r.ahead} to push`, r.behind && `${r.behind} to pull`].filter(Boolean).join(", ");
  return { text, name: `GitHub, ${name}` };
});
const planCount = computed(() => (nav.projectId ? plan.progress(nav.projectId) : null));
function tabLabel(id: ProjectTab): string | undefined {
  if (id === "plan" && planCount.value) return `Plan, ${planCount.value.done} of ${planCount.value.total} tasks done`;
  if (id === "preview" && dot.value) return `Preview, ${dot.value.label}`;
  if (id === "github") return gh.signingIn ? "GitHub, waiting for sign-in" : syncCount.value?.name;
}
// Two diff views must not be mistaken for each other (M3-D33).
const captions: Partial<Record<ProjectTab, string>> = {
  changes: "What this run changed",
  source: "Uncommitted changes in your repository",
  releases: "Builds of this app and what GNOME launches",
};
// The run picker shows only where it changes what you see (M3-D34).
const showPicker = computed(() => list.value.length > 0 && (tab.value === "chat" || tab.value === "changes"));

const selectedId = computed({
  get: () => (nav.projectId ? runs.selected[nav.projectId] ?? null : null),
  set: (v) => {
    if (nav.projectId) runs.selected[nav.projectId] = v;
  },
});

// The icon lives in the project's files (src-tauri/icons), so Releases and GNOME use it too.
const iconBusy = ref(false);
async function changeIcon() {
  if (!project.value || iconBusy.value) return;
  iconBusy.value = true;
  try {
    const r = await pickIcon(project.value.id);
    if (r.error) toast.show(r.error);
    else if (r.changed) toast.show("App icon changed. Preview and the next release use it.");
  } finally {
    iconBusy.value = false;
  }
}

async function load() {
  if (nav.projectId) forgetIcon(nav.projectId);
  if (!projects.projects.length) await projects.refresh();
  if (nav.projectId) await runs.loadSessions(nav.projectId);
  if (nav.projectId) await sync.refresh(nav.projectId);
}
onMounted(load);
watch(() => nav.projectId, load);
// The tab count needs the plan before the Plan tab is opened.
const loadPlan = () => (nav.projectId ? plan.load(nav.projectId) : undefined);
onMounted(loadPlan);
watch(() => nav.projectId, loadPlan);
// The count follows commits made in Source and pushes made here.
watch(() => nav.tab, () => nav.projectId && void sync.refresh(nav.projectId));

// Esc returns to the dashboard (D46), unless a dialog or field handles it first.
function onKey(e: KeyboardEvent) {
  if (e.key !== "Escape" || e.defaultPrevented) return;
  const t = e.target;
  if (t instanceof Element && t.closest("dialog, [role='dialog'], textarea, input, select")) return;
  nav.go("projects");
}
onMounted(() => window.addEventListener("keydown", onKey));
onUnmounted(() => window.removeEventListener("keydown", onKey));

function onTabKey(e: KeyboardEvent) {
  if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
  const i = tabs.findIndex((t) => t.id === tab.value);
  const next = tabs[(i + (e.key === "ArrowRight" ? 1 : tabs.length - 1)) % tabs.length]!;
  tab.value = next.id;
  document.getElementById(`tab-${next.id}`)?.focus();
}
</script>

<template>
  <section v-if="project" class="project">
    <nav class="crumbs" aria-label="Breadcrumb">
      <button type="button" class="link" @click="nav.go('projects')">Projects</button>
      <span aria-hidden="true">›</span>
      <span class="muted" aria-current="page">{{ project.name }}</span>
    </nav>

    <header class="head">
      <button type="button" class="icon-btn" title="Change app icon…" :aria-label="`Change app icon of ${project.name}`" :aria-busy="iconBusy" :disabled="iconBusy" @click="changeIcon">
        <AppIcon :project-id="project.id" :name="project.name" :size="48" />
      </button>
      <div class="title">
        <h1>{{ project.name }}</h1>
        <p class="meta">
          <span class="mono muted path" :title="project.workspacePath">{{ project.workspacePath }}</span>
          <span class="badge">{{ TEMPLATE_LABELS[project.templateId] }}</span>
        </p>
      </div>
      <label v-if="showPicker" class="picker">
        <span class="sr-only">Run</span>
        <select v-model="selectedId" class="input">
          <option v-for="s in list" :key="s.id" :value="s.id">{{ runLabel(s) }}</option>
        </select>
      </label>
    </header>

    <div class="tabs" role="tablist" aria-label="Project" @keydown="onTabKey">
      <button
        v-for="t in tabs" :id="`tab-${t.id}`" :key="t.id" type="button" role="tab" class="tab"
        :aria-selected="tab === t.id" :aria-controls="`panel-${t.id}`" :tabindex="tab === t.id ? 0 : -1"
        :aria-label="tabLabel(t.id)"
        @click="tab = t.id"
      >
        {{ t.label }}
        <span v-if="t.id === 'github' && gh.signingIn" class="dot" aria-hidden="true" />
        <span v-else-if="t.id === 'github' && syncCount" class="count" aria-hidden="true">{{ syncCount.text }}</span>
        <span v-if="t.id === 'plan' && planCount" class="count" aria-hidden="true">{{ planCount.done }}/{{ planCount.total }}</span>
        <span v-if="t.id === 'chat' && waiting" class="badge warn">Needs approval</span>
        <span v-if="t.id === 'preview' && dot" class="dot" :class="dot.tone" aria-hidden="true" />
      </button>
    </div>

    <div :id="`panel-${tab}`" class="panel" role="tabpanel" :aria-labelledby="`tab-${tab}`">
      <p v-if="captions[tab]" class="caption muted">{{ captions[tab] }}</p>
      <ChatTab v-if="tab === 'chat'" :key="project.id" :project="project" @review-changes="tab = 'changes'" />
      <PlanTab v-else-if="tab === 'plan'" :project="project" />
      <ChangesTab v-else-if="tab === 'changes'" :project="project" />
      <SourceTab v-else-if="tab === 'source'" :project="project" />
      <PreviewTab v-else-if="tab === 'preview'" :project="project" />
      <ReleasesTab v-else-if="tab === 'releases'" :project="project" />
      <GitHubTab v-else-if="tab === 'github'" :project="project" />
      <DatabaseTab v-else :project="project" />
    </div>
  </section>
  <p v-else class="muted">Project not found. <button type="button" class="link" @click="nav.go('projects')">Back to Projects</button></p>
</template>

<style scoped>
.project { display: flex; flex-direction: column; height: calc(100vh - 48px); min-height: 0; }
.crumbs { display: flex; gap: 6px; align-items: center; font-size: 13px; margin-bottom: 8px; }
.link { font: inherit; background: none; border: 0; padding: 0; color: var(--accent-text); cursor: pointer; }
.link:hover { text-decoration: underline; }
.head { display: flex; justify-content: space-between; align-items: flex-start; gap: var(--space); flex-wrap: wrap; }
.title { flex: 1 1 0; min-width: 0; }
.icon-btn { flex: none; padding: 4px; border: 1px solid transparent; border-radius: 10px; background: none; cursor: pointer; }
.icon-btn:hover { border-color: var(--border); background: var(--card); }
.icon-btn:disabled { cursor: progress; opacity: 0.6; }
.picker { flex: none; }
.meta { display: flex; gap: 8px; align-items: center; margin: 4px 0 0; font-size: 13px; flex-wrap: wrap; }
.path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; max-width: 60ch; }
.picker select { width: auto; max-width: 360px; }
.tabs { display: flex; gap: 4px; border-bottom: 1px solid var(--border); margin-top: var(--space); }
.tab { font: inherit; background: none; border: 0; border-bottom: 2px solid transparent; padding: 8px 12px; color: var(--muted); cursor: pointer; display: flex; gap: 8px; align-items: center; margin-bottom: -1px; }
.tab[aria-selected="true"] { color: var(--accent-text); border-bottom-color: var(--accent); font-weight: 600; }
.dot { width: 8px; height: 8px; border-radius: 50%; background: var(--accent); }
.count { font-size: 13px; color: var(--muted); font-variant-numeric: tabular-nums; font-weight: 400; }
.dot.ok { background: var(--ok); }
.dot.danger { background: var(--danger); }
.caption { margin: 0 0 8px; font-size: 13px; }
.panel { flex: 1; min-height: 0; display: flex; flex-direction: column; padding-top: var(--space); }
</style>
