<script setup lang="ts">
import { computed, onMounted, onUnmounted, watch } from "vue";
import ChatTab from "./ChatTab.vue";
import ChangesTab from "./ChangesTab.vue";
import SourceTab from "./SourceTab.vue";
import PreviewTab from "./PreviewTab.vue";
import ReleasesTab from "./ReleasesTab.vue";
import { TEMPLATE_LABELS } from "../lib/call";
import { useNavStore, type ProjectTab } from "../stores/nav";
import { useProjectsStore } from "../stores/projects";
import { usePreviewStore } from "../stores/preview";
import { useRunsStore } from "../stores/runs";
import { runLabel } from "../lib/runs";

const nav = useNavStore();
const projects = useProjectsStore();
const runs = useRunsStore();
const preview = usePreviewStore();

const project = computed(() => projects.projects.find((p) => p.id === nav.projectId) ?? null);
const list = computed(() => (nav.projectId ? runs.sessions[nav.projectId] ?? [] : []));
const dot = computed(() => (nav.projectId ? preview.indicatorOf(nav.projectId) : null));
const waiting = computed(() => !!nav.projectId && runs.waitingProjects.includes(nav.projectId));

const tab = computed({ get: () => nav.tab, set: (v: ProjectTab) => (nav.tab = v) });
const tabs: { id: ProjectTab; label: string }[] = [
  { id: "chat", label: "Chat" },
  { id: "changes", label: "Changes" },
  { id: "source", label: "Source" },
  { id: "preview", label: "Preview" },
  { id: "releases", label: "Releases" },
];
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

async function load() {
  if (!projects.projects.length) await projects.refresh();
  if (nav.projectId) await runs.loadSessions(nav.projectId);
}
onMounted(load);
watch(() => nav.projectId, load);

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
        :aria-label="t.id === 'preview' && dot ? `Preview, ${dot.label}` : undefined"
        @click="tab = t.id"
      >
        {{ t.label }}
        <span v-if="t.id === 'chat' && waiting" class="badge warn">Needs approval</span>
        <span v-if="t.id === 'preview' && dot" class="dot" :class="dot.tone" aria-hidden="true" />
      </button>
    </div>

    <div :id="`panel-${tab}`" class="panel" role="tabpanel" :aria-labelledby="`tab-${tab}`">
      <p v-if="captions[tab]" class="caption muted">{{ captions[tab] }}</p>
      <ChatTab v-if="tab === 'chat'" :project="project" @review-changes="tab = 'changes'" />
      <ChangesTab v-else-if="tab === 'changes'" :project="project" />
      <SourceTab v-else-if="tab === 'source'" :project="project" />
      <PreviewTab v-else-if="tab === 'preview'" :project="project" />
      <ReleasesTab v-else :project="project" />
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
.picker { flex: none; }
.meta { display: flex; gap: 8px; align-items: center; margin: 4px 0 0; font-size: 13px; flex-wrap: wrap; }
.path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; max-width: 60ch; }
.picker select { width: auto; max-width: 360px; }
.tabs { display: flex; gap: 4px; border-bottom: 1px solid var(--border); margin-top: var(--space); }
.tab { font: inherit; background: none; border: 0; border-bottom: 2px solid transparent; padding: 8px 12px; color: var(--muted); cursor: pointer; display: flex; gap: 8px; align-items: center; margin-bottom: -1px; }
.tab[aria-selected="true"] { color: var(--accent-text); border-bottom-color: var(--accent); font-weight: 600; }
.dot { width: 8px; height: 8px; border-radius: 50%; background: var(--accent); }
.dot.ok { background: var(--ok); }
.dot.danger { background: var(--danger); }
.caption { margin: 0 0 8px; font-size: 13px; }
.panel { flex: 1; min-height: 0; display: flex; flex-direction: column; padding-top: var(--space); }
</style>
