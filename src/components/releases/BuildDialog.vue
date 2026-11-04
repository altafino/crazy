<script setup lang="ts">
import { onMounted, ref } from "vue";
import { commands } from "../../bindings";
import type { CommandError } from "../../bindings";
import { useBuildsStore } from "../../stores/builds";
import Modal from "../Modal.vue";

const props = defineProps<{ projectId: string; highest: string | null }>();
const emit = defineEmits<{ close: [] }>();
const builds = useBuildsStore();

const version = ref("");
const notes = ref("");
const dirty = ref(false);
const failure = ref<CommandError | null>(null);
const busy = ref(false);

onMounted(async () => {
  const [v, st] = await Promise.all([builds.nextVersion(props.projectId), commands.sourceStatus(props.projectId, []).catch(() => null)]);
  if (v.ok && !version.value) version.value = v.data;
  if (st && st.status === "ok") dirty.value = st.data.entries.length > 0;
});

const versionError = () => failure.value?.fieldErrors.find((f) => f.field === "version")?.message ?? null;

async function submit() {
  if (busy.value || !version.value.trim()) return;
  busy.value = true;
  failure.value = null;
  const r = await builds.start(props.projectId, version.value.trim(), notes.value.trim() || null);
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
        <input v-model="version" class="input mono" autocomplete="off" :aria-invalid="!!versionError()" aria-describedby="bd-hint" />
      </label>
      <p id="bd-hint" class="muted hint">
        <template v-if="highest">Highest so far: <span class="mono">{{ highest }}</span></template>
        <template v-else>This is the first release.</template>
      </p>
      <p v-if="versionError()" class="err" role="alert">{{ versionError() }}</p>
      <label class="field">
        <span>Notes <span class="muted">(optional)</span></span>
        <textarea v-model="notes" class="input" rows="2" placeholder="What changed in this build" />
      </label>
      <p v-if="dirty" class="callout warn">Uncommitted changes in your project will be included in this build.</p>
      <p v-if="failure && !versionError()" class="callout danger" role="alert">{{ failure.message }}</p>
      <div class="actions">
        <button type="button" class="btn" @click="emit('close')">Cancel</button>
        <button type="submit" class="btn primary" :disabled="busy || !version.trim()">Build {{ version }}</button>
      </div>
    </form>
  </Modal>
</template>

<style scoped>
.form { display: flex; flex-direction: column; gap: 8px; min-width: min(420px, 100%); }
.field { display: flex; flex-direction: column; gap: 4px; margin: 0; }
.hint { margin: -4px 0 0; font-size: 13px; }
.err { color: var(--danger); margin: 0; font-size: 13px; }
.callout { margin: 0; }
.actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 4px; }
</style>
