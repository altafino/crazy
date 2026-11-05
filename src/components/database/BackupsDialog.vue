<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { commands } from "../../bindings";
import type { BackupView, DataGuard, DataSet } from "../../bindings";
import { call } from "../../lib/call";
import { formatSize } from "../../lib/format";
import { REASON_TEXT, SET_LABEL, deletable, setName, shortDate } from "../../lib/appdb";
import { useToastStore } from "../../stores/toast";
import Modal from "../Modal.vue";
import ConfirmDialog from "../ConfirmDialog.vue";

const props = defineProps<{ projectId: string; which: DataSet; guard: DataGuard | null; canBackUp: boolean }>();
const emit = defineEmits<{ close: []; changed: [announce: string] }>();
const toast = useToastStore();

const backups = ref<BackupView[]>([]);
const loaded = ref(false);
const error = ref<string | null>(null);
const busy = ref<"backup" | "restore" | "delete" | null>(null);
const pending = ref<{ kind: "restore" | "delete"; b: BackupView } | null>(null);

async function load() {
  const r = await call(commands.appdbListBackups(props.projectId, props.which));
  if (r.ok) backups.value = r.data;
  else error.value = r.error.message;
  loaded.value = true;
}
onMounted(load);

const when = (b: BackupView) => shortDate(b.createdAt);
const title = (b: BackupView) => b.label ?? `Backup from ${when(b)}`;
/** Restore replaces a file the app may hold open (DR-15). */
const blocked = (b: BackupView) =>
  b.which === "preview" ? (props.guard?.previewRunning ? "Stop Preview first" : null) : props.guard?.installedRunning ? `Quit ${props.guard.appName} first` : null;
const restoreReason = computed(() => backups.value.map(blocked).find(Boolean) ?? null);

async function backUp() {
  busy.value = "backup";
  error.value = null;
  const r = await call(commands.appdbBackup(props.projectId, props.which));
  busy.value = null;
  if (!r.ok) return void (error.value = r.error.message);
  toast.show(`Backed up ${setName(props.which)}`);
  emit("changed", `Backed up ${setName(props.which)}`);
  await load();
}

async function doRestore(b: BackupView) {
  pending.value = null;
  busy.value = "restore";
  const r = await call(commands.appdbRestoreBackup(props.projectId, b.id));
  busy.value = null;
  if (!r.ok) return void (error.value = r.error.message);
  const text = `${SET_LABEL[b.which].replace(/^./, (c) => c.toUpperCase())} replaced`;
  toast.show(text);
  emit("changed", text);
  emit("close");
}

async function doDelete(b: BackupView) {
  pending.value = null;
  busy.value = "delete";
  const r = await call(commands.appdbDeleteBackup(props.projectId, b.id));
  busy.value = null;
  if (!r.ok) return void (error.value = r.error.message);
  emit("changed", "Backup deleted");
  await load();
}
</script>

<template>
  <Modal v-if="!pending" :title="`Backups of ${SET_LABEL[which]}`" @close="emit('close')">
    <div class="top">
      <button type="button" class="btn primary" :disabled="!!busy || !canBackUp" @click="backUp">{{ busy === "backup" ? "Backing up…" : "Back up now" }}</button>
      <p v-if="!canBackUp" class="caption muted">There is no data to back up yet.</p>
    </div>
    <p v-if="error" class="callout danger" role="alert">{{ error }}</p>
    <p v-if="loaded && !backups.length" class="muted">No backups yet. Back up now keeps a copy you can restore later.</p>
    <ul v-if="backups.length" class="list" aria-label="Backups">
      <li v-for="b in backups" :key="b.id" class="item">
        <div class="what">
          <p class="name">{{ title(b) }}</p>
          <p class="caption muted tnum">{{ REASON_TEXT[b.reason] }} · {{ when(b) }}<template v-if="b.sizeBytes != null"> · {{ formatSize(b.sizeBytes) }}</template></p>
          <p v-if="!b.fileExists" class="caption muted">The backup file is gone.</p>
        </div>
        <button
          type="button" class="btn small" :disabled="!!busy || !!blocked(b) || !b.fileExists" :aria-label="`Restore ${title(b)}…`"
          :aria-describedby="blocked(b) ? 'bk-why' : undefined" @click="pending = { kind: 'restore', b }"
        >Restore…</button>
        <button v-if="deletable(b)" type="button" class="btn small destructive" :disabled="!!busy" :aria-label="`Delete ${title(b)}…`" @click="pending = { kind: 'delete', b }">Delete…</button>
      </li>
    </ul>
    <p v-if="restoreReason" id="bk-why" class="caption muted">{{ restoreReason }}</p>
    <div class="actions"><button type="button" class="btn" @click="emit('close')">Close</button></div>
  </Modal>

  <ConfirmDialog
    v-else-if="pending.kind === 'restore'" :title="`Replace ${setName(pending.b.which)} with the backup from ${when(pending.b)}?`" confirm-label="Replace data"
    @confirm="doRestore(pending.b)" @cancel="pending = null"
  >
    <p>Studio first saves the current data as a "before restore" backup. You can restore that one too.</p>
  </ConfirmDialog>
  <ConfirmDialog v-else :title="`Delete the backup from ${when(pending.b)}?`" confirm-label="Delete backup" @confirm="doDelete(pending.b)" @cancel="pending = null">
    <p>This removes the copy ({{ pending.b.sizeBytes != null ? formatSize(pending.b.sizeBytes) : "size unknown" }}). Your app's data is not touched.</p>
  </ConfirmDialog>
</template>

<style scoped>
.top { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; margin-bottom: 12px; }
.caption { margin: 0; font-size: 13px; }
.list { list-style: none; margin: 0 0 12px; padding: 0; }
.item { display: flex; gap: 8px; align-items: center; padding: 8px 0; border-bottom: 1px solid var(--border); }
.what { flex: 1; min-width: 0; }
.name { margin: 0; font-weight: 600; }
.actions { display: flex; justify-content: flex-end; }
.callout { margin-bottom: 12px; }
</style>
