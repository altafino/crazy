<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref } from "vue";
import { MoreVertical } from "lucide-vue-next";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import type { ProjectSummary } from "../bindings";
import RemoveProjectDialog from "./RemoveProjectDialog.vue";
import UninstallDialog from "./UninstallDialog.vue";
import { kept } from "./releases/helpers";
import { forgetIcon, pickIcon } from "../lib/projectIcons";
import { useBuildsStore } from "../stores/builds";
import { useProjectsStore } from "../stores/projects";
import { useSyncStore } from "../stores/sync";
import { useToastStore } from "../stores/toast";

/** ⋮ menu: project actions that don't belong to one tab (PRD F01 quick actions). Project page and cards. */
const props = defineProps<{ project: ProjectSummary }>();
const emit = defineEmits<{ removed: [] }>();

const builds = useBuildsStore();
const projects = useProjectsStore();
const sync = useSyncStore();
const toast = useToastStore();

const open = ref(false);
const btn = ref<HTMLButtonElement | null>(null);
const menu = ref<HTMLElement | null>(null);
const id = computed(() => props.project.id);
const installedVersion = computed(() => builds.installedVersion(id.value));
const githubUrl = computed(() => {
  const g = sync.status[id.value]?.remote?.github;
  return g ? `https://github.com/${g.owner}/${g.name}` : null;
});
const folderMissing = computed(() => props.project.state.kind === "missingWorkspace");
const removing = ref(false);
const uninstalling = ref(false);
const keptCount = computed(() => kept(builds.releases[id.value] ?? []).length);

async function toggle() {
  open.value = !open.value;
  if (!open.value) return;
  // Cards don't load the repo status; Open on GitHub shows up once it is here.
  if (!(id.value in sync.status) && !folderMissing.value) void sync.refresh(id.value);
  await nextTick();
  menu.value?.querySelector<HTMLElement>("[role='menuitem']")?.focus();
}
function close(refocus = false) {
  open.value = false;
  if (refocus) btn.value?.focus();
}
function onKey(e: KeyboardEvent) {
  const items = [...(menu.value?.querySelectorAll<HTMLElement>("[role='menuitem']") ?? [])];
  const i = items.indexOf(document.activeElement as HTMLElement);
  if (e.key === "Escape") {
    e.preventDefault(); // not "back to Projects" (ProjectView)
    close(true);
  } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    items[(i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length]?.focus();
  } else if (e.key === "Tab") {
    close();
  }
}
function onOutside(e: MouseEvent) {
  const t = e.target as Node;
  if (open.value && !menu.value?.contains(t) && !btn.value?.contains(t)) close();
}
onMounted(() => window.addEventListener("mousedown", onOutside));
onUnmounted(() => window.removeEventListener("mousedown", onOutside));

/** Runs a menu action after closing the menu; failures become a toast. */
async function act(f: () => Promise<unknown>) {
  close();
  try {
    await f();
  } catch (e) {
    toast.show(e instanceof Error ? e.message : String(e));
  }
}
const reveal = () => act(() => revealItemInDir(props.project.workspacePath));
const copyPath = () =>
  act(async () => {
    await navigator.clipboard.writeText(props.project.workspacePath);
    toast.show("Path copied to clipboard.");
  });
const changeIcon = () =>
  act(async () => {
    const r = await pickIcon(id.value);
    if (r.error) toast.show(r.error);
    else if (r.changed) toast.show("App icon changed. Preview and the next release use it.");
  });
const openGithub = () => act(() => openUrl(githubUrl.value!));
const launch = () =>
  act(async () => {
    const r = await builds.launch(id.value);
    if (!r.ok) toast.show(r.error.message);
  });
const askUninstall = () =>
  act(async () => {
    await builds.loadReleases(id.value);
    uninstalling.value = true;
  });
async function confirmUninstall() {
  uninstalling.value = false;
  const p = props.project;
  const r = await builds.uninstall(p.id);
  toast.show(r.ok ? `Uninstalled ${p.name} from GNOME.` : r.error.message);
}
function askRemove() {
  close();
  removing.value = true;
}
function onRemoved(trashed: boolean) {
  const p = props.project;
  removing.value = false;
  forgetIcon(p.id);
  toast.show(`Removed ${p.name} from Studio.${trashed ? " The folder is in the Trash." : ""}`);
  emit("removed"); // ProjectView leaves the page before the list drops the project
  projects.forget(p.id);
}
</script>

<template>
  <div class="menu-wrap">
    <button
      ref="btn" type="button" class="btn small menu-btn" :aria-label="`More actions for ${project.name}`"
      aria-haspopup="menu" :aria-expanded="open" @click="toggle"
    >
      <MoreVertical :size="18" aria-hidden="true" />
    </button>
    <div v-if="open" ref="menu" class="menu" role="menu" :aria-label="`${project.name} actions`" @keydown="onKey">
      <button type="button" role="menuitem" :disabled="folderMissing" @click="reveal">Show in Files</button>
      <button type="button" role="menuitem" @click="copyPath">Copy path</button>
      <button type="button" role="menuitem" :disabled="folderMissing" @click="changeIcon">Change app icon…</button>
      <button v-if="githubUrl" type="button" role="menuitem" @click="openGithub">Open on GitHub</button>
      <button v-if="installedVersion" type="button" role="menuitem" @click="launch">Launch {{ project.name }}</button>
      <hr role="separator" />
      <button v-if="installedVersion" type="button" role="menuitem" @click="askUninstall">Uninstall from GNOME…</button>
      <button type="button" role="menuitem" class="danger" @click="askRemove">Remove from Studio…</button>
    </div>
    <!-- Out of the card, so clicks in a dialog don't open the project. -->
    <Teleport to="body">
      <RemoveProjectDialog v-if="removing" :project="project" @removed="onRemoved" @cancel="removing = false" />
      <UninstallDialog
        v-if="uninstalling" :app-name="project.name" :version="installedVersion" :releases-kept="keptCount"
        @confirm="confirmUninstall" @cancel="uninstalling = false"
      />
    </Teleport>
  </div>
</template>

<style scoped>
.menu-wrap { position: relative; flex: none; }
.menu-btn { display: inline-flex; align-items: center; min-height: 36px; min-width: 36px; justify-content: center; }
.menu { position: absolute; right: 0; top: calc(100% + 4px); z-index: 10; min-width: 220px; background: var(--card); border: 1px solid var(--border); border-radius: 8px; padding: 4px; display: flex; flex-direction: column; box-shadow: 0 4px 16px rgb(0 0 0 / 0.2); }
.menu button { font: inherit; text-align: left; background: none; border: 0; border-radius: 6px; padding: 8px 12px; color: var(--text); cursor: pointer; min-height: 36px; }
/* :focus, not :focus-visible: WebKit skips the latter for the focus() the arrow keys move. */
.menu button:hover:not(:disabled), .menu button:focus { background: rgb(128 128 128 / 0.15); outline: none; }
.menu button:disabled { color: var(--muted); cursor: default; }
.menu .danger { color: var(--danger); }
.menu hr { border: 0; border-top: 1px solid var(--border); margin: 4px 0; }
</style>
