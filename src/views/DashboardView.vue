<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import type { ProjectSummary } from "../bindings";
import CloneDialog from "../components/CloneDialog.vue";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import NewProjectWizard from "../components/NewProjectWizard.vue";
import ProjectCard from "../components/ProjectCard.vue";
import { pickPath } from "../lib/pick";
import { useNavStore } from "../stores/nav";
import { useProjectsStore } from "../stores/projects";
import { useRunsStore } from "../stores/runs";

const projects = useProjectsStore();
const nav = useNavStore();
const runs = useRunsStore();
const root = ref<HTMLElement | null>(null);
const query = ref("");
const showWizard = ref(false);
const showClone = ref(false);

onMounted(() => projects.refresh());

const visible = computed(() => {
  const q = query.value.trim().toLowerCase();
  return q ? projects.projects.filter((p) => p.name.toLowerCase().includes(q) || p.slug.toLowerCase().includes(q)) : projects.projects;
});

async function importFolder() {
  const p = await pickPath(true);
  if (p) await projects.startImport(p);
}

async function relink(p: ProjectSummary) {
  const path = await pickPath(true);
  if (path) await projects.relink(p.id, path);
}

async function openFolder(p: ProjectSummary) {
  try {
    // opener:default allows reveal, not arbitrary openPath (capabilities stay minimal).
    await revealItemInDir(p.workspacePath);
  } catch (e) {
    projects.error = { message: `Could not open folder: ${e instanceof Error ? e.message : String(e)}`, fieldErrors: [] };
  }
}

async function copyPath(p: ProjectSummary) {
  try {
    await navigator.clipboard.writeText(p.workspacePath);
    projects.notice = "Path copied to clipboard.";
  } catch {
    projects.error = { message: "Could not copy the path.", fieldErrors: [] };
  }
}

async function onCreated(p: ProjectSummary) {
  showWizard.value = false;
  await nextTick();
  root.value?.querySelector<HTMLElement>(`[data-project-id="${CSS.escape(p.id)}"]`)?.focus();
}
</script>

<template>
  <section ref="root" aria-labelledby="dash-h">
    <div class="view-head">
      <h1 id="dash-h">Projects</h1>
      <div v-if="projects.projects.length" class="toolbar">
        <input v-model="query" type="search" class="input search" aria-label="Search projects by name or slug" placeholder="Search" />
        <button type="button" class="btn" @click="importFolder">Import folder</button>
        <button type="button" class="btn" @click="showClone = true">Clone from GitHub</button>
        <button type="button" class="btn primary" @click="showWizard = true">New project</button>
      </div>
    </div>

    <div aria-live="polite" class="status">
      <p v-if="projects.notice" class="callout">{{ projects.notice }}</p>
    </div>
    <div role="alert">
      <p v-if="projects.error" class="callout danger">{{ projects.error.message }}</p>
    </div>

    <p v-if="projects.loading && !projects.projects.length" class="muted">Loading…</p>

    <div v-else-if="!projects.projects.length" class="empty">
      <h2>No apps yet</h2>
      <p class="muted">Create an app from the template, bring in an existing folder, or clone a GitHub repository.</p>
      <div class="empty-actions">
        <button type="button" class="btn primary" @click="showWizard = true">Create Your First App</button>
        <button type="button" class="btn" @click="importFolder">Import a folder</button>
        <button type="button" class="btn" @click="showClone = true">Clone from GitHub</button>
      </div>
    </div>

    <template v-else>
      <p v-if="!visible.length" class="muted">No projects match "{{ query }}".</p>
      <div class="grid">
        <ProjectCard v-for="p in visible" :key="p.id" :project="p" @relink="relink" @enter="nav.openProject($event.id)" @reveal="openFolder" @copy="copyPath"
          :waiting="runs.waitingProjects.includes(p.id)" />
      </div>
    </template>

    <CloneDialog v-if="showClone" @close="showClone = false" />
    <NewProjectWizard v-if="showWizard" @close="showWizard = false" @created="onCreated" />

    <ConfirmDialog
      v-if="projects.pendingImport?.kind === 'needsRelink'"
      title="Relink project"
      confirm-label="Relink"
      @confirm="projects.confirmImport()"
      @cancel="projects.cancelImport()"
    >
      <p>
        This folder contains <strong>{{ projects.pendingImport.existing.name }}</strong>, which the library lists at
        <code>{{ projects.pendingImport.existing.workspacePath }}</code> (missing). Relink it here?
      </p>
      <p class="mono">{{ projects.pendingImport.newPath }}</p>
    </ConfirmDialog>

    <ConfirmDialog
      v-if="projects.pendingImport?.kind === 'needsCopyConfirm'"
      title="Import as copy"
      confirm-label="Import as copy"
      @confirm="projects.confirmImport()"
      @cancel="projects.cancelImport()"
    >
      <p>
        This folder looks like a copy of <strong>{{ projects.pendingImport.existing.name }}</strong>, which is still at
        <code>{{ projects.pendingImport.existing.workspacePath }}</code>.
      </p>
      <p>
        Importing it gives the copy a new project ID and a new app ID / bundle identifier. Studio rewrites
        project.json and tauri.conf.json in that folder.
      </p>
      <p class="mono">{{ projects.pendingImport.newPath }}</p>
    </ConfirmDialog>
  </section>
</template>

<style scoped>
.toolbar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.search { width: 220px; }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: var(--space); }
.empty { text-align: center; padding: 64px 16px; }
.empty-actions { display: flex; justify-content: center; gap: 8px; margin-top: var(--space); }
.status:empty, .status:has(:empty) { margin: 0; }
</style>
