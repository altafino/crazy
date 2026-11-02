<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { CircleAlert } from "lucide-vue-next";
import type { ProjectSummary } from "../bindings";
import { endedText, fixPrompt, installSkipped } from "../lib/preview";
import { useNavStore } from "../stores/nav";
import { usePreviewStore } from "../stores/preview";
import LogView from "../components/preview/LogView.vue";
import PhaseStepper from "../components/preview/PhaseStepper.vue";

const props = defineProps<{ project: ProjectSummary }>();
const nav = useNavStore();
const preview = usePreviewStore();

const id = computed(() => props.project.id);
const loaded = ref(false);
async function load() {
  loaded.value = false;
  await preview.load(id.value);
  loaded.value = true;
}
onMounted(load);
watch(id, load);

const status = computed(() => preview.status[id.value] ?? null);
const log = computed(() => preview.logs[id.value] ?? null);
const lines = computed(() => log.value?.lines ?? []);
const error = computed(() => preview.error[id.value] ?? null);
const live = computed(() => !!status.value?.live);
/** Ended in this Studio session (M3-D40); after a restart only the start screen with "Last preview" shows (M3-D41). */
const ended = computed(() => !!status.value && !live.value && !!preview.thisSession[id.value]);
const last = computed(() => (status.value && !live.value && !ended.value && status.value.hasLog ? status.value : null));
const viewingLog = ref(false);
watch(id, () => (viewingLog.value = false));

const phase = computed(() => (status.value?.live ? status.value.phase : null));
const failed = computed(() => (phase.value?.phase === "buildFailed" ? phase.value : null));
const lastWhen = computed(() => {
  const t = last.value?.completedAt ?? last.value?.startedAt;
  return t ? new Date(t).toLocaleString() : "";
});

const busy = ref(false);
async function run(fn: (p: string) => Promise<unknown>) {
  busy.value = true;
  await fn(id.value);
  busy.value = false;
}
const start = () => run(preview.start);
const restart = () => run(preview.restart);
const stop = () => run(preview.stop);
async function viewLog() {
  viewingLog.value = true;
  if (last.value && log.value?.jobId !== last.value.jobId) await preview.loadLog(id.value, last.value.jobId);
}
const askFix = () => nav.askClaude({ projectId: id.value, text: fixPrompt(failed.value?.firstError, lines.value) });
const showLog = computed(() => live.value || ended.value || viewingLog.value);
</script>

<template>
  <div class="preview">
    <p v-if="error" class="callout danger" role="alert">{{ error.message }}</p>

    <!-- Not available: imported folders (M3-D39). -->
    <div v-if="!project.supportsOperations" class="start">
      <h2>Preview isn't available for this project</h2>
      <p class="muted">{{ project.name }} is an imported folder. Studio can only run projects it created from the Tauri template, because it needs to know how to build and start them.</p>
    </div>

    <template v-else-if="loaded">
      <!-- Running -->
      <template v-if="live && status">
        <div class="head">
          <PhaseStepper :phase="phase" :install-skipped="installSkipped(lines)" />
          <span v-if="status.port" class="muted tnum">Port {{ status.port }}</span>
          <span class="spacer" />
          <button type="button" class="btn small" :disabled="busy" @click="restart">Restart</button>
          <button type="button" class="btn small" :disabled="busy" @click="stop">Stop</button>
        </div>
        <p v-if="installSkipped(lines)" class="muted note">Dependencies weren't refreshed because Claude is changing files. Restart the preview after the run if it added packages.</p>
        <p v-if="phase?.phase === 'waitingForRustBuild'" class="muted note">Waiting for another Rust build to finish. Preview continues by itself.</p>
        <div v-if="failed" class="callout danger build" role="alert">
          <CircleAlert :size="18" :stroke-width="1.75" aria-hidden="true" />
          <code class="mono msg">{{ failed.firstError || "The build failed. See the log for details." }}</code>
          <button type="button" class="btn small" @click="askFix">Ask Claude to fix</button>
        </div>
      </template>

      <!-- Ended in this session (M3-D40) -->
      <div v-else-if="ended && status" class="head">
        <p class="outcome"><strong>{{ endedText(status) }}</strong></p>
        <span class="spacer" />
        <button type="button" class="btn small primary" :disabled="busy" @click="start">Start again</button>
      </div>

      <!-- Start screen -->
      <div v-else class="start">
        <h2>Run {{ project.name }} as a separate window</h2>
        <p class="muted">Studio builds the app and opens it next to this window. The first build compiles Rust and can take several minutes; later starts are much faster.</p>
        <p v-if="last" class="muted">
          Last preview {{ lastWhen }} · {{ endedText(last) }}
          · <button v-if="!viewingLog" type="button" class="link" @click="viewLog">View log</button>
          <button v-else type="button" class="link" @click="viewingLog = false">Hide log</button>
        </p>
        <button type="button" class="btn primary" :disabled="busy" @click="start">Start preview</button>
      </div>

      <LogView v-if="showLog && log" :lines="lines" :first-seq="log.firstSeq" :truncated="log.truncated" />
    </template>
  </div>
</template>

<style scoped>
.preview { display: flex; flex-direction: column; gap: 12px; flex: 1; min-height: 0; }
.start { max-width: 640px; }
.start h2 { margin: 0 0 8px; }
.start p { margin: 0 0 12px; }
.head { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; }
.spacer { flex: 1; }
.outcome { margin: 0; }
.note { margin: 0; font-size: 13px; }
.link { font: inherit; background: none; border: 0; padding: 0; color: var(--accent-text); cursor: pointer; }
.link:hover { text-decoration: underline; }
/* M3-D44: a full border on all sides, no left stripe. */
.callout.build { display: flex; gap: 10px; align-items: center; border: 1px solid var(--danger); margin: 0; }
.msg { flex: 1; min-width: 0; overflow-wrap: anywhere; white-space: pre-wrap; }
</style>
