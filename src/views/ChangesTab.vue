<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { Undo2 } from "lucide-vue-next";
import { commands } from "../bindings";
import type { FileChange, ProjectSummary, RevertReport, RevertSkipReason, SkipReason } from "../bindings";
import { call } from "../lib/call";
import { isActive, useRunsStore } from "../stores/runs";
import FileList from "../components/changes/FileList.vue";
import DiffView from "../components/changes/DiffView.vue";
import RevertDialog from "../components/changes/RevertDialog.vue";

const props = defineProps<{ project: ProjectSummary }>();
const runs = useRunsStore();

const session = computed(() => runs.selectedSession(props.project.id));
const anyActive = computed(() => (runs.sessions[props.project.id] ?? []).some(isActive));
const interrupted = computed(() => !!session.value && !session.value.hasAfter);

const all = ref<FileChange[] | null>(null);
/** Raw backend text when the diff can't be read. */
const loadError = ref<string | null>(null);
const selectedPath = ref<string | null>(null);
const dialog = ref(false);
const report = ref<RevertReport | null>(null);
const reloadKey = ref("");

const files = computed(() => (all.value ?? []).filter((f) => !f.skippedReason));
const skipped = computed(() => (all.value ?? []).filter((f) => f.skippedReason));
const current = computed(() => files.value.find((f) => f.path === selectedPath.value) ?? files.value[0] ?? null);

const SKIP_TEXT: Record<SkipReason, string> = { ignoredDir: "ignored folder", secret: "looks like a secret", tooLarge: "larger than 50 MB" };
const REVERT_TEXT: Record<RevertSkipReason, string> = {
  editedSince: "edited since the run",
  notCaptured: "wasn't captured in the snapshot",
  notSelected: "not selected",
};

async function load() {
  all.value = null;
  loadError.value = null;
  const s = session.value;
  if (!s || !s.hasSnapshot || s.snapshotPruned || anyActive.value) return;
  const r = await call(commands.runDiff(s.id));
  if (s.id !== session.value?.id) return; // stale
  if (r.ok) {
    all.value = r.data;
    selectedPath.value = null;
  } else loadError.value = r.error.message;
}

watch(
  () => [session.value?.id, session.value?.jobState, session.value?.hasSnapshot, session.value?.snapshotPruned, anyActive.value],
  (now, before) => {
    if (now[0] !== before?.[0]) report.value = null;
    void load();
  },
  { immediate: true },
);

async function onReverted(r: RevertReport) {
  dialog.value = false;
  report.value = r;
  reloadKey.value = `${Date.now()}`;
  await load();
}
</script>

<template>
  <div class="changes">
    <p v-if="!session" class="empty muted">No runs yet. Ask for a change in Chat and it shows up here.</p>
    <p v-else-if="anyActive" class="empty muted">Changes appear when the run finishes.</p>
    <p v-else-if="!session.hasSnapshot" class="empty muted">This run has no snapshot, so its changes can't be shown.</p>
    <div v-else-if="session.snapshotPruned" class="empty">
      <p class="muted">Snapshots for this run were cleaned up.</p>
      <button type="button" class="btn destructive" disabled>Revert this run…</button>
    </div>
    <div v-else-if="loadError" class="empty">
      <p class="muted">Couldn't load this run's changes.</p>
      <details><summary>Details</summary><pre class="mono">{{ loadError }}</pre></details>
    </div>
    <p v-else-if="!all" class="empty muted">Loading…</p>
    <template v-else>
      <div class="bar">
        <p v-if="interrupted" class="callout warn banner">
          {{ session.jobState === "interrupted" ? "Studio closed during this run." : "This run didn't finish." }}
          Showing the difference from before the run to now.
        </p>
        <button type="button" class="btn destructive revert" :disabled="!files.length" @click="dialog = true">
          <Undo2 :size="16" aria-hidden="true" /> Revert this run…
        </button>
      </div>

      <div v-if="report" class="callout report" role="status">
        <p><strong class="tnum">{{ report.restored.length }}</strong> {{ report.restored.length === 1 ? "file" : "files" }} restored.</p>
        <ul v-if="report.restored.length" class="mono"><li v-for="p in report.restored" :key="p">{{ p }}</li></ul>
        <template v-if="report.skipped.length">
          <p>Skipped:</p>
          <ul><li v-for="s in report.skipped" :key="s.path"><span class="mono">{{ s.path }}</span>, {{ REVERT_TEXT[s.reason] }}</li></ul>
        </template>
      </div>

      <p v-if="!files.length" class="empty muted">This run didn't change any files.</p>
      <div v-else class="split">
        <aside class="side card"><FileList :files="files" :selected="current?.path ?? null" @select="selectedPath = $event" /></aside>
        <div class="main card"><DiffView v-if="current" :session-id="session.id" :file="current" :reload-key="reloadKey" /></div>
      </div>

      <p v-if="skipped.length" class="foot muted">
        <span class="tnum">{{ skipped.length }}</span> {{ skipped.length === 1 ? "file" : "files" }} skipped:
        <span v-for="(f, i) in skipped" :key="f.path"><span class="mono">{{ f.path }}</span> ({{ SKIP_TEXT[f.skippedReason!] }}){{ i < skipped.length - 1 ? ", " : "" }}</span>
      </p>

      <RevertDialog v-if="dialog" :session-id="session.id" :files="files" :interrupted="interrupted" @close="dialog = false" @done="onReverted" />
    </template>
  </div>
</template>

<style scoped>
.changes { display: flex; flex-direction: column; gap: 12px; flex: 1; min-height: 0; }
.empty { margin: 0; padding: var(--space) 0; }
pre { white-space: pre-wrap; margin: 4px 0 8px; }
.bar { display: flex; justify-content: flex-end; align-items: center; gap: 12px; }
.banner { flex: 1; margin: 0; }
.revert { display: inline-flex; gap: 6px; align-items: center; }
.report p { margin: 0 0 4px; }
.report ul { margin: 0 0 8px; padding-left: 20px; }
.split { display: grid; grid-template-columns: 300px minmax(0, 1fr); gap: 12px; flex: 1; min-height: 0; }
.card { border: 1px solid var(--border); border-radius: var(--radius); background: var(--card); min-height: 0; overflow: hidden; }
.foot { margin: 0; font-size: 0.9em; }
@media (max-width: 899px) {
  .split { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr); }
  .side { border: 0; background: none; overflow: visible; }
}
</style>
