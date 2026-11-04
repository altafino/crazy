<script setup lang="ts">
import { computed, ref, useId } from "vue";
import { commands } from "../../bindings";
import type { GhError, RemoteCreated } from "../../bindings";
import Modal from "../Modal.vue";

/** M5-DR6: Create GitHub Repository. Private is the default; Public needs an explicit tick. */
const props = defineProps<{ projectId: string; defaultName: string }>();
const emit = defineEmits<{ close: []; created: [r: RemoteCreated]; authLost: [] }>();

const name = ref(props.defaultName);
const description = ref("");
const visibility = ref<"private" | "public">("private");
const understood = ref(false);
const busy = ref(false);
const error = ref<GhError | null>(null);
const nameEl = ref<HTMLInputElement | null>(null);
const nameId = useId();
const errId = useId();

// GitHub allows letters, digits, "-", "_" and "."; checked live so the button explains itself.
const nameProblem = computed(() => {
  const n = name.value.trim();
  if (!n) return "Enter a name.";
  if (!/^[A-Za-z0-9._-]+$/.test(n)) return "Use letters, numbers, - _ and . only.";
  if (n === "." || n === "..") return "That isn't a valid name.";
  return "";
});
const isPublic = computed(() => visibility.value === "public");
const canCreate = computed(() => !busy.value && !nameProblem.value && (!isPublic.value || understood.value));

const errorText = computed(() => {
  const e = error.value;
  if (!e) return "";
  switch (e.kind) {
    case "nameTaken": return `You already have a repository named ${name.value.trim()}.`;
    case "insideLargerRepo": return `This project is inside the repository at ${e.top}; create it there instead.`;
    case "signedOut": case "tokenInvalid": return "GitHub needs you to sign in again.";
    case "unreachable": return "GitHub can't be reached.";
    case "invalidName": case "gh": return e.message;
    case "originExists": return `This project already has a remote: ${e.url}`;
    case "noCommits": return "Commit first, then create the repository.";
    default: return "Git reported a problem.";
  }
});

async function create() {
  if (!canCreate.value) return;
  busy.value = true;
  error.value = null;
  const r = await commands
    .githubCreateRepo(props.projectId, name.value.trim(), !isPublic.value, description.value.trim() || null)
    .catch((e) => ({ status: "error" as const, error: { kind: "gh" as const, message: String(e) } }));
  busy.value = false;
  if (r.status === "ok") return emit("created", r.data);
  error.value = r.error;
  if (r.error.kind === "nameTaken") nameEl.value?.focus();
  if (r.error.kind === "signedOut" || r.error.kind === "tokenInvalid") emit("authLost");
}
</script>

<template>
  <Modal title="Create GitHub repository" @close="!busy && emit('close')">
    <form class="form" @submit.prevent="create">
      <label :for="nameId">Name</label>
      <input :id="nameId" ref="nameEl" v-model="name" class="input" :aria-invalid="!!nameProblem || error?.kind === 'nameTaken'" :aria-describedby="errId" autocomplete="off" />
      <p :id="errId" class="muted note" role="status">{{ error?.kind === "nameTaken" ? "" : nameProblem }}</p>

      <label for="repo-desc">Description (optional)</label>
      <input id="repo-desc" v-model="description" class="input" autocomplete="off" />

      <fieldset>
        <legend>Visibility</legend>
        <label class="radio"><input v-model="visibility" type="radio" name="vis" value="private" /> Private</label>
        <label class="radio"><input v-model="visibility" type="radio" name="vis" value="public" /> Public</label>
      </fieldset>

      <template v-if="isPublic">
        <p class="callout warn">Anyone on the internet can see this code</p>
        <label class="radio"><input v-model="understood" type="checkbox" /> I understand this repository will be public</label>
      </template>

      <p v-if="errorText" class="callout danger" role="alert">{{ errorText }}</p>

      <div class="actions">
        <button type="button" class="btn" :disabled="busy" @click="emit('close')">Cancel</button>
        <button type="submit" class="btn primary" :disabled="!canCreate">{{ busy ? "Creating…" : isPublic ? "Create public repository" : "Create repository" }}</button>
      </div>
    </form>
  </Modal>
</template>

<style scoped>
.form { display: flex; flex-direction: column; gap: 6px; }
.note { margin: 0; font-size: 13px; min-height: 1.2em; }
fieldset { border: 0; padding: 0; margin: 6px 0 0; display: flex; flex-direction: column; gap: 4px; }
legend { padding: 0; font-weight: 600; margin-bottom: 2px; }
.radio { display: flex; gap: 8px; align-items: center; }
.callout { margin: 0; }
.actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 8px; }
</style>
