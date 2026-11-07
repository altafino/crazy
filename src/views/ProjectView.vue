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
import RemoveProjectDialog from "../components/RemoveProjectDialog.vue";
import UninstallDialog from "../components/UninstallDialog.vue";
import { kept } from "../components/releases/helpers";
import { forgetIcon, pickIcon } from "../lib/projectIcons";
import { useBuildsStore } from "../stores/builds";
import { useToastStore } from "../stores/toast";
import { nextTick, ref } from "vue";
import { MoreVertical } from "lucide-vue-next";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";

const nav = useNavStore();
const projects = useProjectsStore();
const runs = useRunsStore();
const preview = usePreviewStore();
const gh = useGithubStore();
const sync = useSyncStore();
const toast = useToastStore();
const plan = usePlanStore();
const builds = useBuildsStore();

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

// ⋮ menu: project actions that don't belong to one tab (PRD F01 quick actions).
const menuOpen = ref(false);
const menuBtn = ref<HTMLButtonElement | null>(null);
const menuEl = ref<HTMLElement | null>(null);
const installedVersion = computed(() => (nav.projectId ? builds.installedVersion(nav.projectId) : null));
const githubUrl = computed(() => {
  const g = nav.projectId ? sync.status[nav.projectId]?.remote?.github : null;
  return g ? `https://github.com/${g.owner}/${g.name}` : null;
});
const folderMissing = computed(() => project.value?.state.kind === "missingWorkspace");
const removing = ref(false);
const uninstalling = ref(false);
const keptCount = computed(() => kept(nav.projectId ? builds.releases[nav.projectId] ?? [] : []).length);

async function toggleMenu() {
  menuOpen.value = !menuOpen.value;
  if (!menuOpen.value) return;
  await nextTick();
  menuEl.value?.querySelector<HTMLElement>("[role='menuitem']")?.focus();
}
function closeMenu(refocus = false) {
  menuOpen.value = false;
  if (refocus) menuBtn.value?.focus();
}
function onMenuKey(e: KeyboardEvent) {
  const items = [...(menuEl.value?.querySelectorAll<HTMLElement>("[role='menuitem']") ?? [])];
  const i = items.indexOf(document.activeElement as HTMLElement);
  if (e.key === "Escape") {
    e.preventDefault(); // not "back to Projects" (onKey)
    closeMenu(true);
  } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    items[(i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length]?.focus();
  } else if (e.key === "Tab") {
    closeMenu();
  }
}
function onOutside(e: MouseEvent) {
  const t = e.target as Node;
  if (menuOpen.value && !menuEl.value?.contains(t) && !menuBtn.value?.contains(t)) closeMenu();
}
/** Runs a menu action after closing the menu; failures become a toast. */
async function act(f: () => Promise<unknown>) {
  closeMenu();
  try {
    await f();
  } catch (e) {
    toast.show(e instanceof Error ? e.message : String(e));
  }
}
const reveal = () => act(() => revealItemInDir(project.value!.workspacePath));
const copyPath = () =>
  act(async () => {
    await navigator.clipboard.writeText(project.value!.workspacePath);
    toast.show("Path copied to clipboard.");
  });
const openGithub = () => act(() => openUrl(githubUrl.value!));
const launch = () =>
  act(async () => {
    const r = await builds.launch(project.value!.id);
    if (!r.ok) toast.show(r.error.message);
  });
const askUninstall = () =>
  act(async () => {
    await builds.loadReleases(project.value!.id);
    uninstalling.value = true;
  });
async function confirmUninstall() {
  uninstalling.value = false;
  const p = project.value!;
  const r = await builds.uninstall(p.id);
  toast.show(r.ok ? `Uninstalled ${p.name} from GNOME.` : r.error.message);
}
function askRemove() {
  closeMenu();
  removing.value = true;
}
function onRemoved(trashed: boolean) {
  const p = project.value!;
  removing.value = false;
  forgetIcon(p.id);
  toast.show(`Removed ${p.name} from Studio.${trashed ? " The folder is in the Trash." : ""}`);
  nav.go("projects");
  projects.forget(p.id);
}

async function load() {
  if (nav.projectId) forgetIcon(nav.projectId);
  if (!builds.installedLoaded) void builds.loadInstalled();
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
onMounted(() => {
  window.addEventListener("keydown", onKey);
  window.addEventListener("mousedown", onOutside);
});
onUnmounted(() => {
  window.removeEventListener("keydown", onKey);
  window.removeEventListener("mousedown", onOutside);
});

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
      <div class="menu-wrap">
        <button
          ref="menuBtn" type="button" class="btn small menu-btn" :aria-label="`More actions for ${project.name}`"
          aria-haspopup="menu" :aria-expanded="menuOpen" @click="toggleMenu"
        >
          <MoreVertical :size="18" aria-hidden="true" />
        </button>
        <div v-if="menuOpen" ref="menuEl" class="menu" role="menu" :aria-label="`${project.name} actions`" @keydown="onMenuKey">
          <button type="button" role="menuitem" :disabled="folderMissing" @click="reveal">Show in Files</button>
          <button type="button" role="menuitem" @click="copyPath">Copy path</button>
          <button type="button" role="menuitem" :disabled="folderMissing || iconBusy" @click="act(changeIcon)">Change app icon…</button>
          <button v-if="githubUrl" type="button" role="menuitem" @click="openGithub">Open on GitHub</button>
          <button v-if="installedVersion" type="button" role="menuitem" @click="launch">Launch {{ project.name }}</button>
          <hr role="separator" />
          <button v-if="installedVersion" type="button" role="menuitem" @click="askUninstall">Uninstall from GNOME…</button>
          <button type="button" role="menuitem" class="danger" @click="askRemove">Remove from Studio…</button>
        </div>
      </div>
    </header>

    <RemoveProjectDialog v-if="removing" :project="project" @removed="onRemoved" @cancel="removing = false" />
    <UninstallDialog
      v-if="uninstalling" :app-name="project.name" :version="installedVersion" :releases-kept="keptCount"
      @confirm="confirmUninstall" @cancel="uninstalling = false"
    />

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
.menu-wrap { position: relative; flex: none; }
.menu-btn { display: inline-flex; align-items: center; min-height: 36px; min-width: 36px; justify-content: center; }
.menu { position: absolute; right: 0; top: calc(100% + 4px); z-index: 10; min-width: 220px; background: var(--card); border: 1px solid var(--border); border-radius: 8px; padding: 4px; display: flex; flex-direction: column; box-shadow: 0 4px 16px rgb(0 0 0 / 0.2); }
.menu button { font: inherit; text-align: left; background: none; border: 0; border-radius: 6px; padding: 8px 12px; color: var(--text); cursor: pointer; min-height: 36px; }
/* :focus, not :focus-visible: WebKit skips the latter for the focus() the arrow keys move. */
.menu button:hover:not(:disabled), .menu button:focus { background: rgb(128 128 128 / 0.15); outline: none; }
.menu button:disabled { color: var(--muted); cursor: default; }
.menu .danger { color: var(--danger); }
.menu hr { border: 0; border-top: 1px solid var(--border); margin: 4px 0; }
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
