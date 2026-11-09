<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue";
import { commands } from "../../bindings";
import type { CommandError, SchemaChange } from "../../bindings";
import { useBuildsStore } from "../../stores/builds";
import { useNavStore } from "../../stores/nav";
import Modal from "../Modal.vue";
import { bumps } from "./helpers";

const props = defineProps<{ projectId: string; highest: string | null }>();
const emit = defineEmits<{ close: [] }>();
const builds = useBuildsStore();
const nav = useNavStore();

const version = ref("");
const notes = ref("");
/** "From 2 runs since 1.1.0. Edit freely." (D9); absent when there were no runs. */
const notesFrom = ref("");
const dirty = ref<string[]>([]);
const schema = ref<SchemaChange | null>(null);
const failure = ref<CommandError | null>(null);
const busy = ref(false);
const versionInput = ref<HTMLInputElement | null>(null);

// D10: ticked by default, remembered per project.
const INSTALL_KEY = `build-install:${props.projectId}`;
const install = ref(true);
try {
  install.value = localStorage.getItem(INSTALL_KEY) !== "0";
} catch {
  /* no storage: default */
}
function remember() {
  try {
    localStorage.setItem(INSTALL_KEY, install.value ? "1" : "0");
  } catch {
    /* no storage */
  }
}

onMounted(async () => {
  // D11: back from "Commit first…" with the fields as they were.
  const kept = nav.buildDraft?.projectId === props.projectId ? nav.buildDraft : null;
  nav.buildDraft = null;
  if (kept) {
    version.value = kept.version;
    notes.value = kept.notes;
    install.value = kept.install;
  }
  const [v, n, sc, st] = await Promise.all([
    builds.nextVersion(props.projectId),
    kept ? null : builds.notesDraft(props.projectId),
    builds.schemaChange(props.projectId),
    commands.sourceStatus(props.projectId, []).catch(() => null),
  ]);
  if (v.ok && !version.value) version.value = v.data;
  if (n?.ok && n.data.runs) {
    notes.value = n.data.text;
    notesFrom.value = `From ${n.data.runs} ${n.data.runs === 1 ? "run" : "runs"} since ${n.data.sinceVersion ?? "the project started"}. Edit freely.`;
  }
  if (sc.ok) schema.value = sc.data;
  if (st && st.status === "ok") dirty.value = st.data.entries.map((e) => e.path);
  await nextTick();
  versionInput.value?.select(); // D8: typing replaces the suggestion
});

const bump = computed(() => bumps(props.highest ?? ""));
const versionError = () => failure.value?.fieldErrors.find((f) => f.field === "version")?.message ?? null;
const files = computed(() => `${dirty.value.length} ${dirty.value.length === 1 ? "file isn't" : "files aren't"} committed yet. ${dirty.value.length === 1 ? "It'll" : "They'll"} be in this build.`);

/** D11: Source opens with every changed file ticked and the notes as the message; the dialog's fields wait in `nav`. */
function commitFirst() {
  const v = version.value.trim();
  nav.buildDraft = { projectId: props.projectId, version: v, notes: notes.value, install: install.value };
  nav.commitFiles({ projectId: props.projectId, paths: dirty.value, message: notes.value.trim() || `Release ${v}` });
  emit("close");
}

async function submit() {
  if (busy.value || !version.value.trim()) return;
  busy.value = true;
  failure.value = null;
  remember();
  const r = await builds.start(props.projectId, version.value.trim(), notes.value.trim() || null, install.value);
  busy.value = false;
  if (r.ok) emit("close");
  else failure.value = r.error;
}
</script>

<template>
  <Modal title="Build new release" @close="emit('close')">
    <form class="form" @submit.prevent="submit">
      <label class="field">
        <span>Version</span>
        <input ref="versionInput" v-model="version" class="input mono" autocomplete="off" :aria-invalid="!!versionError()" aria-describedby="bd-hint" />
      </label>
      <p id="bd-hint" class="muted hint">
        <template v-if="highest">
          Highest so far: <span class="mono">{{ highest }}</span>
          <template v-if="bump">
            · or <button type="button" class="link mono" :aria-label="`Use version ${bump.minor}`" @click="version = bump.minor">{{ bump.minor }}</button>
            · <button type="button" class="link mono" :aria-label="`Use version ${bump.major}`" @click="version = bump.major">{{ bump.major }}</button>
          </template>
        </template>
        <template v-else>This is the first release.</template>
      </p>
      <p v-if="versionError()" class="err" role="alert">{{ versionError() }}</p>
      <label class="field">
        <span>Notes <span class="muted">(optional)</span></span>
        <textarea v-model="notes" class="input" rows="4" placeholder="What changed in this build" />
      </label>
      <p v-if="notesFrom" class="muted hint">{{ notesFrom }}</p>
      <label class="check">
        <input v-model="install" type="checkbox" />
        <span>Install in GNOME when the test launch passes</span>
      </label>
      <p v-if="install && schema" class="muted hint">Database changes: schema {{ schema.from }} → {{ schema.to }}. Studio backs up the app's data before installing.</p>
      <div v-if="dirty.length" class="callout warn row">
        <p>{{ files }}</p>
        <button type="button" class="btn small" @click="commitFirst">Commit first…</button>
      </div>
      <p v-if="failure && !versionError()" class="callout danger" role="alert">{{ failure.message }}</p>
      <div class="actions">
        <button type="button" class="btn" @click="emit('close')">Cancel</button>
        <button type="submit" class="btn primary" :disabled="busy || !version.trim()">{{ install ? "Build and install" : "Build" }} {{ version }}</button>
      </div>
    </form>
  </Modal>
</template>

<style scoped>
.form { display: flex; flex-direction: column; gap: 8px; min-width: min(420px, 100%); }
.field { display: flex; flex-direction: column; gap: 4px; margin: 0; }
.hint { margin: -4px 0 0; font-size: 13px; }
.err { color: var(--danger); margin: 0; font-size: 13px; }
.check { display: flex; align-items: center; gap: 8px; margin: 0; }
.link { font: inherit; background: none; border: 0; padding: 0; color: var(--accent-text); cursor: pointer; }
.link:hover { text-decoration: underline; }
.callout { margin: 0; }
.callout.row { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
.callout.row p { margin: 0; }
.actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 4px; }
</style>
