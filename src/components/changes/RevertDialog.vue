<script setup lang="ts">
import { computed, ref } from "vue";
import { commands } from "../../bindings";
import type { FileChange, RevertReport } from "../../bindings";
import { call } from "../../lib/call";
import Modal from "../Modal.vue";

const props = defineProps<{ sessionId: string; files: FileChange[]; interrupted: boolean }>();
const emit = defineEmits<{ close: []; done: [report: RevertReport] }>();

/** An interrupted run starts unticked: the files may hold work done after it (D37). */
const selected = ref<string[]>(props.interrupted ? [] : props.files.map((f) => f.path));
const busy = ref(false);
const error = ref<string | null>(null);
const count = computed(() => selected.value.length);

async function revert() {
  busy.value = true;
  error.value = null;
  const r = await call(commands.revertRun(props.sessionId, [...selected.value]));
  busy.value = false;
  if (r.ok) emit("done", r.data);
  else error.value = r.error.message;
}
</script>

<template>
  <Modal title="Revert this run?" @close="emit('close')">
    <p>This restores files to how they were before the run. Untick files you changed yourself.</p>
    <ul class="files">
      <li v-for="f in files" :key="f.path">
        <label><input v-model="selected" type="checkbox" :value="f.path" /> <span class="mono">{{ f.path }}</span></label>
      </li>
    </ul>
    <div v-if="error" class="callout danger">
      Couldn't revert.
      <details><summary>Details</summary><pre class="mono">{{ error }}</pre></details>
    </div>
    <div class="actions">
      <button type="button" class="btn" @click="emit('close')">Cancel</button>
      <button type="button" class="btn destructive filled" :disabled="!count || busy" @click="revert">
        Revert <span class="tnum">{{ count }}</span> {{ count === 1 ? "file" : "files" }}
      </button>
    </div>
  </Modal>
</template>

<style scoped>
.files { list-style: none; margin: 0 0 var(--space); padding: 0; border: 1px solid var(--border); border-radius: 8px; max-height: 40vh; overflow: auto; }
.files li { padding: 8px 12px; }
.files li + li { border-top: 1px solid var(--border); }
label { display: flex; gap: 8px; align-items: center; cursor: pointer; }
pre { white-space: pre-wrap; margin: 4px 0 0; }
.actions { display: flex; justify-content: flex-end; gap: 8px; }
</style>
