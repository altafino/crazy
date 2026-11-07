<script setup lang="ts">
import type { ProjectSummary } from "../bindings";
import { computed, onMounted } from "vue";
import { TEMPLATE_LABELS } from "../lib/call";
import AppIcon from "./AppIcon.vue";
import ProjectMenu from "./ProjectMenu.vue";
import { useBuildsStore } from "../stores/builds";
import { usePreviewStore } from "../stores/preview";

const props = defineProps<{ project: ProjectSummary; waiting?: boolean }>();
const previewing = computed(() => usePreviewStore().indicatorOf(props.project.id));
const builds = useBuildsStore();
const installedVersion = computed(() => builds.installedVersion(props.project.id));
onMounted(() => {
  void builds.loadStatus(props.project.id);
  if (!builds.installedLoaded) void builds.loadInstalled();
});
const emit = defineEmits<{ relink: [ProjectSummary]; enter: [ProjectSummary]; reveal: [ProjectSummary]; copy: [ProjectSummary] }>();

/** The whole card opens the project (D46); its own buttons don't. */
function onCardClick(e: MouseEvent, p: ProjectSummary) {
  if ((e.target as HTMLElement).closest("button, a, [role='menu']")) return;
  if (p.state.kind !== "missingWorkspace") emit("enter", p);
}
</script>

<template>
  <article class="card" tabindex="-1" @click="onCardClick($event, project)" :data-project-id="project.id" :aria-label="project.name">
    <header>
      <AppIcon :project-id="project.id" :name="project.name" :size="32" />
      <h2>{{ project.name }}</h2>
      <span class="badges">
      <span v-if="waiting" class="badge warn">Needs approval</span>
      <span v-else-if="project.state.kind === 'ok'" class="badge ok">OK</span>
      <span v-else-if="project.state.kind === 'missingWorkspace'" class="badge danger">Folder missing</span>
      <span v-else class="badge warn">Needs attention</span>
      <span v-if="builds.isBuilding(project.id)" class="badge warn">Building…</span>
      <span v-else-if="installedVersion" class="badge">Installed {{ installedVersion }}</span>
      <span v-if="previewing?.tone === 'danger'" class="badge danger">Preview: build failed</span>
      <span v-else-if="previewing" class="badge ok">Previewing</span>
      </span>
    </header>
    <p class="muted slug">{{ project.slug }}</p>
    <p class="muted">{{ TEMPLATE_LABELS[project.templateId] }}</p>
    <p class="mono path" :title="project.workspacePath">{{ project.workspacePath }}</p>
    <p v-if="project.state.kind === 'metadataMismatch'" class="note warn">{{ project.state.detail }}</p>
    <p v-if="!project.supportsOperations" class="muted note">
      Imported folder: build and install are not available for this project type
    </p>
    <footer>
      <button v-if="project.state.kind === 'missingWorkspace'" type="button" class="btn small primary" @click="emit('relink', project)">Relink</button>
      <button v-else type="button" class="btn small primary" @click="emit('enter', project)">Open</button>
      <button type="button" class="btn small" :disabled="project.state.kind === 'missingWorkspace'" @click="emit('reveal', project)">Show in Files</button>
      <button type="button" class="btn small" @click="emit('copy', project)">Copy path</button>
      <ProjectMenu class="more" :project="project" />
    </footer>
  </article>
</template>

<style scoped>
.card { cursor: pointer; background: var(--card); border: 1px solid var(--border); border-radius: var(--radius); padding: var(--space); display: flex; flex-direction: column; gap: 4px; min-width: 0; }
header { display: flex; justify-content: space-between; align-items: center; gap: 8px; }
.badges { display: flex; gap: 6px; flex-wrap: wrap; justify-content: flex-end; }
h2 { margin: 0; overflow-wrap: anywhere; flex: 1; min-width: 0; }
p { margin: 0; }
.slug { font-family: var(--mono); font-size: 0.85em; }
.path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.note { font-size: 0.9em; }
.warn { color: var(--warn); }
footer { display: flex; gap: 8px; margin-top: 10px; flex-wrap: wrap; }
.more { margin-left: auto; }
</style>
