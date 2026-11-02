<script setup lang="ts">
import type { ProjectSummary } from "../bindings";
import { TEMPLATE_LABELS } from "../lib/call";

defineProps<{ project: ProjectSummary }>();
const emit = defineEmits<{ relink: [ProjectSummary]; open: [ProjectSummary]; copy: [ProjectSummary] }>();
</script>

<template>
  <article class="card" tabindex="-1" :data-project-id="project.id" :aria-label="project.name">
    <header>
      <h2>{{ project.name }}</h2>
      <span v-if="project.state.kind === 'ok'" class="badge ok">OK</span>
      <span v-else-if="project.state.kind === 'missingWorkspace'" class="badge danger">Folder missing</span>
      <span v-else class="badge warn">Needs attention</span>
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
      <button type="button" class="btn small" :disabled="project.state.kind === 'missingWorkspace'" @click="emit('open', project)">Show in Files</button>
      <button type="button" class="btn small" @click="emit('copy', project)">Copy path</button>
    </footer>
  </article>
</template>

<style scoped>
.card { background: var(--card); border: 1px solid var(--border); border-radius: var(--radius); padding: var(--space); display: flex; flex-direction: column; gap: 4px; min-width: 0; }
header { display: flex; justify-content: space-between; align-items: center; gap: 8px; }
h2 { margin: 0; overflow-wrap: anywhere; }
p { margin: 0; }
.slug { font-family: var(--mono); font-size: 0.85em; }
.path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.note { font-size: 0.9em; }
.warn { color: var(--warn); }
footer { display: flex; gap: 8px; margin-top: 10px; flex-wrap: wrap; }
</style>
