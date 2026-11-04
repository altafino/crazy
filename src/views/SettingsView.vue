<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import PreviewLimits from "../components/settings/PreviewLimits.vue";
import AttachmentLimits from "../components/settings/AttachmentLimits.vue";
import AgentProfiles from "../components/settings/AgentProfiles.vue";
import { pickPath } from "../lib/pick";
import { useDoctorStore } from "../stores/doctor";
import { useNavStore } from "../stores/nav";
import { useSettingsStore } from "../stores/settings";

const settings = useSettingsStore();
const doctor = useDoctorStore();
const nav = useNavStore();
const section = ref<"general" | "agents">(nav.settingsSection);
const TABS = [{ id: "general", label: "General" }, { id: "agents", label: "AI Agents" }] as const;
function onTabKey(e: KeyboardEvent) {
  if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
  section.value = section.value === "general" ? "agents" : "general";
  document.getElementById(`set-tab-${section.value}`)?.focus();
}

const dir = ref("");
const overrides = ref<Record<string, string>>({});
const saved = ref(false);

// Fallback until the first doctor report arrives.
const FALLBACK_IDS = ["git", "cargo", "rustc", "node", "pnpm", "cargo-tauri", "claude", "gh", "webkit2gtk", "xdg-open", "libfuse2"];
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

    <div role="tablist" aria-label="Settings sections" class="tabs" @keydown="onTabKey">
      <button
        v-for="t in TABS" :id="`set-tab-${t.id}`" :key="t.id" type="button" role="tab" class="tab"
        :aria-selected="section === t.id" :aria-controls="`set-panel-${t.id}`" :tabindex="section === t.id ? 0 : -1"
        @click="section = t.id"
      >{{ t.label }}</button>
    </div>

    <div v-if="section === 'agents'" id="set-panel-agents" role="tabpanel" aria-labelledby="set-tab-agents"><AgentProfiles /></div>

    <form v-else id="set-panel-general" role="tabpanel" aria-labelledby="set-tab-general" @submit.prevent="save">
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

      <PreviewLimits />
      <AttachmentLimits />

      <div role="alert">
        <p v-if="settings.error" class="callout danger">{{ settings.error.message }}</p>
      </div>
      <p aria-live="polite" class="muted">{{ saved ? "Settings saved." : "" }}</p>

      <button type="submit" class="btn primary" :disabled="settings.saving || !settings.settings" :aria-busy="settings.saving">Save</button>
    </form>
  </section>
</template>

<style scoped>
.tabs { display: flex; gap: 16px; border-bottom: 1px solid var(--border); margin-bottom: var(--space); }
.tab { background: none; border: 0; border-bottom: 2px solid transparent; padding: 6px 2px; font: inherit; color: var(--muted); cursor: pointer; }
.tab[aria-selected="true"] { border-bottom-color: var(--accent); color: var(--accent-text); }
.card { background: var(--card); border: 1px solid var(--border); border-radius: var(--radius); padding: var(--space); margin-bottom: var(--space); }
.row { display: flex; gap: 8px; }
.tool { margin-bottom: 10px; }
label { display: block; font-weight: 600; margin-bottom: 4px; }
.sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
</style>
