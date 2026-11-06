<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { commands } from "../../bindings";
import { call } from "../../lib/call";
import { formatSize } from "../../lib/format";
import { useToastStore } from "../../stores/toast";
import ConfirmDialog from "../ConfirmDialog.vue";

const toast = useToastStore();
/** undefined while loading, null when Docker's volumes can't be read. */
const bytes = ref<number | null | undefined>(undefined);
const sizeError = ref<string | null>(null);
const accepted = ref<string | null | undefined>(undefined);
const confirming = ref(false);

async function loadSize() {
  const r = await call(commands.exportCacheSize());
  sizeError.value = r.ok ? null : r.error.message;
  bytes.value = r.ok ? (r.data.bytes ?? 0) : null;
}
async function loadLicense() {
  const r = await call(commands.exportReadiness());
  accepted.value = r.ok ? r.data.licenseAcceptedAt : undefined;
}
onMounted(() => Promise.all([loadSize(), loadLicense()]));

const size = computed(() => (bytes.value ? formatSize(bytes.value) : ""));
const none = computed(() => bytes.value === 0);
const date = computed(() => (accepted.value ? new Date(accepted.value).toLocaleDateString(undefined, { dateStyle: "medium" }) : ""));

async function clear() {
  confirming.value = false;
  const r = await call(commands.clearExportCaches());
  if (!r.ok) return void toast.show(r.error.message); // refused while an export runs: "Wait until the export finishes"
  toast.show("Export caches cleared");
  await loadSize();
}
async function withdraw() {
  const r = await call(commands.withdrawExportLicense());
  if (!r.ok) return void toast.show(r.error.message);
  await loadLicense();
}
</script>

<template>
  <div class="card" aria-labelledby="ex-h" role="group">
    <h2 id="ex-h">Exports</h2>
    <p class="muted">Exporting an app for other systems keeps a build environment in Docker so later exports start faster.</p>

    <div class="line">
      <div class="what">
        <p v-if="bytes === undefined" class="size muted">Calculating…</p>
        <template v-else-if="bytes === null">
          <p class="size">Couldn't read Docker volumes</p>
          <details v-if="sizeError" class="details"><summary>Details</summary><pre class="mono">{{ sizeError }}</pre></details>
        </template>
        <p v-else-if="none" class="size muted">No export caches</p>
        <p v-else class="size tnum">Export caches use {{ size }}</p>
      </div>
      <button type="button" class="btn" :disabled="bytes === undefined || bytes === null || none" @click="confirming = true">Clear export caches…</button>
    </div>

    <p v-if="accepted !== undefined" class="license">
      Microsoft license for Windows builds:
      <template v-if="accepted">accepted {{ date }} <button type="button" class="link" @click="withdraw">Withdraw</button></template>
      <template v-else>not accepted</template>
    </p>

    <ConfirmDialog v-if="confirming" title="Clear export caches?" :confirm-label="`Clear ${size}`" @confirm="clear" @cancel="confirming = false">
      <p>This removes the Docker volumes and the build image used for exports.</p>
      <p class="muted">Your exported files are kept. The next export downloads the environment again (about 3 GB).</p>
    </ConfirmDialog>
  </div>
</template>

<style scoped>
.card { background: var(--card); border: 1px solid var(--border); border-radius: var(--radius); padding: var(--space); margin-bottom: var(--space); }
.line { display: flex; gap: 12px; align-items: center; justify-content: space-between; flex-wrap: wrap; }
.what { min-width: 0; }
.size { margin: 0; }
.details pre { margin: 4px 0 0; white-space: pre-wrap; overflow-wrap: anywhere; }
.license { margin: 12px 0 0; }
.link { font: inherit; background: none; border: 0; padding: 0; min-height: 32px; color: var(--accent-text); cursor: pointer; }
.link:hover { text-decoration: underline; }
</style>
