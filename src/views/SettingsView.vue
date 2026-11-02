<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { pickPath } from "../lib/pick";
import { useDoctorStore } from "../stores/doctor";
import { useSettingsStore } from "../stores/settings";

const settings = useSettingsStore();
const doctor = useDoctorStore();

const dir = ref("");
const overrides = ref<Record<string, string>>({});
const saved = ref(false);

// Fallback until the first doctor report arrives.
const FALLBACK_IDS = ["git", "cargo", "rustc", "node", "pnpm", "cargo-tauri", "claude", "gh", "webkit2gtk", "libfuse2"];
const tools = computed(() => (doctor.report ? doctor.report.tools.map((t) => ({ id: t.id, name: t.name })) : FALLBACK_IDS.map((id) => ({ id, name: id }))));

onMounted(async () => {
  if (!settings.settings) await settings.load();
  if (!doctor.report) doctor.run(false);
});

watch(
  () => settings.settings,
  (s) => {
    if (!s) return;
    dir.value = s.defaultProjectsDir;
    overrides.value = { ...s.toolOverrides };
  },
  { immediate: true },
);

async function browseDir() {
  const p = await pickPath(true, dir.value);
  if (p) dir.value = p;
}
async function browseTool(id: string) {
  const p = await pickPath(false);
  if (p) overrides.value[id] = p;
}

async function save() {
  saved.value = false;
  const toolOverrides: Record<string, string> = {};
  for (const [k, v] of Object.entries(overrides.value)) if (v.trim()) toolOverrides[k] = v.trim(); // empty = auto-detect
  const r = await settings.save({ defaultProjectsDir: dir.value.trim(), toolOverrides });
  saved.value = r.ok;
}
const fieldErr = (f: string) => settings.error?.fieldErrors.filter((e) => e.field === f) ?? [];
</script>

<template>
  <section aria-labelledby="set-h">
    <div class="view-head"><h1 id="set-h">Settings</h1></div>

    <form @submit.prevent="save">
      <div class="card">
        <h2>Default projects folder</h2>
        <label for="set-dir" class="sr-only">Default projects folder</label>
        <div class="row">
          <input id="set-dir" v-model="dir" class="input mono" autocomplete="off" aria-describedby="set-dir-err" />
          <button type="button" class="btn" @click="browseDir">Browse…</button>
        </div>
        <p id="set-dir-err" class="field-error" v-for="e in fieldErr('defaultProjectsDir')" :key="e.message">{{ e.message }}</p>
      </div>

      <div class="card">
        <h2>Tool paths</h2>
        <p class="muted">Leave a field empty to auto-detect the tool.</p>
        <div v-for="t in tools" :key="t.id" class="tool">
          <label :for="`tool-${t.id}`">{{ t.name }} <code class="muted">{{ t.id }}</code></label>
          <div class="row">
            <input :id="`tool-${t.id}`" v-model="overrides[t.id]" class="input mono" autocomplete="off" placeholder="auto-detect" />
            <button type="button" class="btn" :aria-label="`Browse for ${t.name}`" @click="browseTool(t.id)">Browse…</button>
          </div>
        </div>
      </div>

      <div role="alert">
        <p v-if="settings.error" class="callout danger">{{ settings.error.message }}</p>
      </div>
      <p aria-live="polite" class="muted">{{ saved ? "Settings saved." : "" }}</p>

      <button type="submit" class="btn primary" :disabled="settings.saving || !settings.settings" :aria-busy="settings.saving">Save</button>
    </form>
  </section>
</template>

<style scoped>
.card { background: var(--card); border: 1px solid var(--border); border-radius: var(--radius); padding: var(--space); margin-bottom: var(--space); }
.row { display: flex; gap: 8px; }
.tool { margin-bottom: 10px; }
label { display: block; font-weight: 600; margin-bottom: 4px; }
.sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
</style>
