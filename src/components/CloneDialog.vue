<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { commands } from "../bindings";
import type { CloneError } from "../bindings";
import { pickPath } from "../lib/pick";
import { useGithubStore } from "../stores/github";
import { useNavStore } from "../stores/nav";
import { useProjectsStore } from "../stores/projects";
import { useSettingsStore } from "../stores/settings";
import Modal from "./Modal.vue";
import SignInCode from "./github/SignInCode.vue";
import { ghErrorText } from "./source/ghText";

/** M5-DR12: pick or type a repository, choose where to save it, clone, open the project. */
const emit = defineEmits<{ close: [] }>();
const gh = useGithubStore();
const nav = useNavStore();
const projects = useProjectsStore();
const settings = useSettingsStore();

const spec = ref("");
const parent = ref(settings.settings?.defaultProjectsDir ?? "");
/** Only a folder the user picked is sent; otherwise the backend uses the default. */
const parentPicked = ref(false);
const cloning = ref<string | null>(null);
const elapsed = ref(0);
const failure = ref<CloneError | null>(null);
let timer: ReturnType<typeof setInterval> | undefined;

const signedIn = computed(() => gh.auth?.kind === "signedIn");
const signedOut = computed(() => gh.auth?.kind === "signedOut");
const repos = computed(() => {
  const q = spec.value.trim().toLowerCase();
  const all = [...(gh.repos ?? [])].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));
  return q ? all.filter((r) => r.nameWithOwner.toLowerCase().includes(q)) : all;
});
const repoName = computed(() => spec.value.trim().replace(/\/+$/, "").replace(/\.git$/, "").split(/[/:]/).pop() ?? "");
const finalPath = computed(() => (repoName.value && parent.value ? `${parent.value.replace(/\/+$/, "")}/${repoName.value}` : ""));
const errorText = computed(() => {
  const f = failure.value;
  if (!f) return "";
  if (f.kind === "project") return f.message;
  return f.error.kind === "stopped" ? "" : ghErrorText(f.error);
});
const targetExists = computed(() => failure.value?.kind === "gh" && failure.value.error.kind === "targetExists");

async function load() {
  await gh.check();
  if (gh.auth?.kind === "signedIn") await gh.loadRepos();
}
onMounted(load);
onBeforeUnmount(() => clearInterval(timer));

async function signIn() {
  await gh.signIn();
  if (gh.auth?.kind === "signedIn") await gh.loadRepos();
}

async function browse() {
  const p = await pickPath(true, parent.value);
  if (p) {
    parent.value = p;
    parentPicked.value = true;
    failure.value = null;
  }
}

async function clone() {
  const s = spec.value.trim();
  if (!s || cloning.value) return;
  failure.value = null;
  cloning.value = s;
  elapsed.value = 0;
  timer = setInterval(() => elapsed.value++, 1000);
  try {
    const r = await commands.cloneProject(s, parentPicked.value ? parent.value : null);
    if (r.status === "error") return void (failure.value = r.error);
    const o = r.data;
    if (o.kind === "imported" || o.kind === "alreadyInLibrary") {
      await projects.refresh();
      emit("close");
      nav.openProject(o.project.id);
    } else {
      // The Dashboard owns the relink / copy confirmations.
      projects.pendingImport = o;
      emit("close");
    }
  } catch (e) {
    failure.value = { kind: "project", message: e instanceof Error ? e.message : String(e) };
  } finally {
    clearInterval(timer);
    cloning.value = null;
  }
}
const stop = () => cloning.value && void commands.cloneProjectStop(cloning.value).catch(() => {});
</script>

<template>
  <Modal title="Clone from GitHub" @close="emit('close')">
    <form novalidate @submit.prevent="clone">
      <div v-if="signedOut || gh.signingIn" class="field">
        <p class="muted">Sign in to see your repositories. You can still type any public owner/name or URL below.</p>
        <SignInCode v-if="gh.signingIn" />
        <button v-else type="button" class="btn primary" @click="signIn">Sign in</button>
      </div>

      <div class="field">
        <label for="cl-spec">Repository</label>
        <input
          id="cl-spec" v-model="spec" class="input mono" autocomplete="off" :disabled="!!cloning"
          placeholder="stefan/quicknotes or a GitHub URL" aria-describedby="cl-eg"
        />
        <p id="cl-eg" class="muted hint">Example: stefan/quicknotes or a GitHub URL</p>
      </div>

      <div v-if="signedIn" class="field">
        <span class="label" id="cl-list-h">Your repositories</span>
        <p v-if="gh.reposLoading" class="muted">Loading your repositories…</p>
        <p v-else-if="gh.reposError" class="muted" role="alert">
          {{ ghErrorText(gh.reposError) }} <button type="button" class="btn small" @click="gh.loadRepos()">Try again</button>
        </p>
        <p v-else-if="!(gh.repos ?? []).length" class="muted">You have no repositories yet. Type an owner/name or URL above.</p>
        <p v-else-if="!repos.length" class="muted">No repositories match "{{ spec }}". Press Clone to try it as typed.</p>
        <ul v-else class="repos" aria-labelledby="cl-list-h">
          <li v-for="r in repos" :key="r.nameWithOwner">
            <button type="button" class="repo" :disabled="!!cloning" :aria-label="`Use ${r.nameWithOwner}`" @click="spec = r.nameWithOwner">
              <span class="mono">{{ r.nameWithOwner }}</span>
              <span v-if="r.private" class="badge">private</span>
              <span v-if="r.description" class="muted desc">{{ r.description }}</span>
            </button>
          </li>
        </ul>
      </div>

      <div class="field">
        <label for="cl-parent">Save in</label>
        <div class="row">
          <input id="cl-parent" v-model="parent" class="input mono" autocomplete="off" :disabled="!!cloning" aria-describedby="cl-target" @input="parentPicked = true" />
          <button type="button" class="btn" :disabled="!!cloning" @click="browse">Change…</button>
        </div>
        <p id="cl-target" class="muted mono hint">{{ finalPath ? `Will clone to: ${finalPath}` : "Pick a repository to see the folder." }}</p>
      </div>

      <div role="status" aria-live="polite" class="status">
        <p v-if="cloning" class="muted">Cloning {{ cloning }} · {{ elapsed }} s <button type="button" class="btn small" @click="stop">Stop</button></p>
      </div>
      <div role="alert" class="status">
        <p v-if="errorText" class="field-error">
          {{ errorText }}
          <button v-if="targetExists" type="button" class="btn small" @click="browse">Change folder</button>
        </p>
      </div>

      <div class="actions">
        <button type="button" class="btn" @click="emit('close')">Cancel</button>
        <button type="submit" class="btn primary" :disabled="!spec.trim() || !!cloning" :aria-busy="!!cloning" :aria-describedby="!spec.trim() ? 'cl-why' : undefined">Clone</button>
      </div>
      <p v-if="!spec.trim()" id="cl-why" class="sr-only">Enter a repository first.</p>
    </form>
  </Modal>
</template>

<style scoped>
.field { margin-bottom: 12px; }
label, .label { display: block; font-weight: 600; margin-bottom: 4px; }
.row { display: flex; gap: 8px; }
.hint { margin: 4px 0 0; font-size: 13px; overflow-wrap: anywhere; }
.repos { list-style: none; margin: 0; padding: 0; max-height: 200px; overflow: auto; border: 1px solid var(--border); border-radius: var(--radius); }
.repo { display: flex; gap: 8px; align-items: baseline; width: 100%; text-align: left; background: none; border: 0; padding: 6px 10px; cursor: pointer; color: inherit; font: inherit; }
.repo:hover { background: var(--sidebar); }
.desc { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
.status { min-height: 1.4em; }
.status p { margin: 0; }
.actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 8px; }
</style>
