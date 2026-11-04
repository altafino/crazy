<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { FieldError, NewProjectRequest_Deserialize, ProjectSummary } from "../bindings";
import { TEMPLATE_LABELS } from "../lib/call";
import { pickPath } from "../lib/pick";
import { useGithubStore } from "../stores/github";
import { useNavStore } from "../stores/nav";
import { useProjectsStore } from "../stores/projects";
import { useToastStore } from "../stores/toast";
import { commands } from "../bindings";
import { ghErrorText } from "./source/ghText";
import { useSettingsStore } from "../stores/settings";
import Modal from "./Modal.vue";

const emit = defineEmits<{ close: []; created: [ProjectSummary] }>();
const projects = useProjectsStore();
const settings = useSettingsStore();
const gh = useGithubStore();
const nav = useNavStore();
const toast = useToastStore();

const name = ref("");
const slug = ref("");
const slugEdited = ref(false);
const parentDir = ref(settings.settings?.defaultProjectsDir ?? "");
const identifier = ref("");
const initGit = ref(true);
const createGh = ref(false);
const ghOn = computed(() => initGit.value && createGh.value);
const gh_hint = computed(() => {
  if (!initGit.value) return "Needs Git";
  if (signedIn.value) return `as ${gh.login()}`;
  return createGh.value ? "You'll sign in on the project's GitHub tab" : "";
});
const signedIn = computed(() => gh.auth?.kind === "signedIn");
watch(initGit, (g) => {
  if (!g) createGh.value = false;
});
onMounted(() => {
  if (!gh.auth) void gh.check();
});

const errors = ref<FieldError[]>([]);
const message = ref("");
const validating = ref(false);
const busy = ref(false);

/** Lowercase ASCII slug; the backend stays the authority and re-validates. */
function slugify(s: string): string {
  return s.normalize("NFKD").replace(/[̀-ͯ]/g, "").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
}

const FIELDS = ["name", "slug", "identifier", "parentDir"];
const errFor = (f: string) => errors.value.filter((e) => e.field === f);
const otherErrors = computed(() => errors.value.filter((e) => !FIELDS.includes(e.field)));
const describedBy = (f: string, hint?: string) => [hint, errFor(f).length ? `${f}-err` : ""].filter(Boolean).join(" ") || undefined;

const target = computed(() => `${parentDir.value.replace(/\/+$/, "")}/${slug.value}`);
const canCreate = computed(
  () => !busy.value && !validating.value && errors.value.length === 0 && !!name.value.trim() && !!slug.value && !!parentDir.value.trim(),
);

function request(): NewProjectRequest_Deserialize {
  return {
    name: name.value,
    slug: slugEdited.value ? slug.value : null,
    parentDir: parentDir.value,
    identifier: identifier.value.trim() || null,
    initGit: initGit.value,
    createGithub: ghOn.value ? { name: slug.value, private: true, description: null } : null,
  };
}

watch(name, (n) => {
  if (!slugEdited.value) slug.value = slugify(n);
});
watch(
  () => settings.settings?.defaultProjectsDir,
  (d) => {
    if (d && !parentDir.value) parentDir.value = d;
  },
);

let timer: ReturnType<typeof setTimeout> | undefined;
let seq = 0;

watch([name, slug, parentDir, identifier], () => {
  clearTimeout(timer);
  seq++;
  message.value = "";
  if (!name.value.trim()) {
    errors.value = [];
    validating.value = false;
    return;
  }
  validating.value = true;
  timer = setTimeout(validate, 300);
});
onBeforeUnmount(() => clearTimeout(timer));

async function validate() {
  const mine = ++seq;
  const r = await projects.validate(request());
  if (mine !== seq) return; // input changed while in flight
  validating.value = false;
  errors.value = r.ok ? r.data : r.error.fieldErrors;
  message.value = r.ok ? "" : r.error.message;
}

async function browse() {
  const p = await pickPath(true, parentDir.value);
  if (p) parentDir.value = p;
}

/** Never rolls back the local project (M5-DR13). True when the project should open on its GitHub tab. */
async function createRepo(p: ProjectSummary): Promise<boolean> {
  if (!signedIn.value) return true;
  try {
    const r = await commands.githubCreateRepo(p.id, slug.value || p.slug, true, null);
    if (r.status === "ok") {
      toast.show("Created a private GitHub repository");
      return false;
    }
    toast.show(ghErrorText(r.error, slug.value || p.slug));
  } catch {
    toast.show(ghErrorText({ kind: "gh", message: "" }));
  }
  return true;
}

async function submit() {
  if (!canCreate.value) return; // also stops a double click / Enter while in flight
  busy.value = true;
  const r = await projects.create(request());
  busy.value = false;
  if (r.ok) {
    const goGithub = ghOn.value ? await createRepo(r.data) : false;
    emit("created", r.data);
    emit("close");
    if (goGithub) {
      nav.openProject(r.data.id);
      nav.tab = "github";
    }
  } else {
    errors.value = r.error.fieldErrors;
    message.value = r.error.message;
  }
}
</script>

<template>
  <Modal title="New project" @close="emit('close')">
    <form novalidate @submit.prevent="submit">
      <div class="field">
        <label for="np-name">Name</label>
        <input id="np-name" v-model="name" class="input" autocomplete="off" required :aria-invalid="errFor('name').length > 0" :aria-describedby="describedBy('name')" />
        <p v-for="e in errFor('name')" id="name-err" :key="e.message" class="field-error">{{ e.message }}</p>
      </div>

      <div class="field">
        <label for="np-slug">Slug</label>
        <input
          id="np-slug" v-model="slug" class="input mono" autocomplete="off"
          :aria-invalid="errFor('slug').length > 0" :aria-describedby="describedBy('slug')"
          @input="slugEdited = slug !== ''"
        />
        <p v-for="e in errFor('slug')" id="slug-err" :key="e.message" class="field-error">{{ e.message }}</p>
      </div>

      <div class="field">
        <label for="np-parent">Parent folder</label>
        <div class="row">
          <input id="np-parent" v-model="parentDir" class="input mono" autocomplete="off" :aria-invalid="errFor('parentDir').length > 0" :aria-describedby="describedBy('parentDir', 'np-target')" />
          <button type="button" class="btn" @click="browse">Browse…</button>
        </div>
        <p v-for="e in errFor('parentDir')" id="parentDir-err" :key="e.message" class="field-error">{{ e.message }}</p>
      </div>
      <p id="np-target" class="muted mono target">Will create: {{ target }}</p>

      <div class="field">
        <label for="np-ident">Bundle identifier (optional)</label>
        <input id="np-ident" v-model="identifier" class="input mono" autocomplete="off" placeholder="derived from slug" :aria-invalid="errFor('identifier').length > 0" :aria-describedby="describedBy('identifier')" />
        <p v-for="e in errFor('identifier')" id="identifier-err" :key="e.message" class="field-error">{{ e.message }}</p>
      </div>

      <div class="field">
        <span class="label">Template</span>
        <p class="muted tpl">{{ TEMPLATE_LABELS["tauri-vue"] }}</p>
      </div>

      <label class="check"><input v-model="initGit" type="checkbox" /> Initialize Git repository</label>
      <div class="gh">
        <label class="check"><input v-model="createGh" type="checkbox" :disabled="!initGit" :aria-describedby="gh_hint ? 'np-gh-hint' : undefined" /> Also create a private GitHub repository</label>
        <p v-if="gh_hint" id="np-gh-hint" class="muted hint">{{ gh_hint }}</p>
      </div>

      <div role="alert" class="alert">
        <p v-for="e in otherErrors" :key="e.message" class="field-error">{{ e.message }}</p>
        <p v-if="message" class="field-error">{{ message }}</p>
      </div>

      <div class="actions">
        <button type="button" class="btn" @click="emit('close')">Cancel</button>
        <button type="submit" class="btn primary" :disabled="!canCreate" :aria-busy="busy">Create</button>
      </div>
    </form>
  </Modal>
</template>

<style scoped>
.field { margin-bottom: 12px; }
label, .label { display: block; font-weight: 600; margin-bottom: 4px; }
.row { display: flex; gap: 8px; }
.target { margin: -6px 0 12px; overflow-wrap: anywhere; }
.tpl { margin: 0; }
.check { display: flex; align-items: center; gap: 8px; font-weight: 400; margin-bottom: 12px; }
.gh .check { margin-bottom: 4px; }
.hint { margin: 0 0 12px 24px; font-size: 13px; }
.alert { min-height: 1.4em; }
.alert p { margin: 0; }
.actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 8px; }
</style>
