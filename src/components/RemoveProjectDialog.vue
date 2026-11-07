<script setup lang="ts">
import { ref } from "vue";
import Modal from "./Modal.vue";
import type { ProjectSummary } from "../bindings";
import { useProjectsStore } from "../stores/projects";

/** Remove from Studio (PRD F01): separate from deleting the source and from uninstalling. */
const props = defineProps<{ project: ProjectSummary }>();
const emit = defineEmits<{ removed: [trashed: boolean]; cancel: [] }>();

const projects = useProjectsStore();
const trash = ref(false);
const busy = ref(false);
const error = ref<string | null>(null);
const folderMissing = props.project.state.kind === "missingWorkspace";

async function remove() {
  busy.value = true;
  error.value = null;
  const r = await projects.remove(props.project.id, trash.value && !folderMissing);
  busy.value = false;
  if (r.ok) emit("removed", trash.value && !folderMissing);
  else error.value = r.error.message;
}
</script>

<template>
  <Modal :title="`Remove ${project.name} from Studio?`" @close="busy || emit('cancel')">
    <p>Studio forgets this project and deletes what it stored for it: runs and their snapshots, attachments, releases and exports, data backups and logs.</p>
    <label v-if="!folderMissing" class="check">
      <input v-model="trash" type="checkbox" :disabled="busy" />
      <span>
        Also move the project folder to the Trash
        <span class="mono muted path" :title="project.workspacePath">{{ project.workspacePath }}</span>
      </span>
    </label>
    <p v-if="!trash || folderMissing" class="muted">The project folder stays where it is. You can import it again later.</p>
    <p v-else class="muted">You can restore it from the Trash in Files.</p>
    <p v-if="error" class="callout danger" role="alert">{{ error }}</p>
    <div class="actions">
      <button type="button" class="btn" :disabled="busy" @click="emit('cancel')">Cancel</button>
      <button type="button" class="btn destructive filled" :disabled="busy" :aria-busy="busy" @click="remove">
        {{ busy ? "Removing…" : "Remove" }}
      </button>
    </div>
  </Modal>
</template>

<style scoped>
p { margin: 0 0 12px; }
.check { display: flex; gap: 8px; align-items: flex-start; margin: 0 0 12px; cursor: pointer; }
.check input { margin-top: 3px; }
.path { display: block; font-size: 0.9em; overflow-wrap: anywhere; }
.actions { display: flex; justify-content: flex-end; gap: 8px; }
</style>
