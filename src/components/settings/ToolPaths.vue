<script setup lang="ts">
import { computed } from "vue";
import type { Settings } from "../../bindings";
import { pickPath } from "../../lib/pick";
import { useDoctorStore } from "../../stores/doctor";
import { useSettingsStore } from "../../stores/settings";
import SettingsGroup from "./SettingsGroup.vue";

// Saves through the settings store: it is the one owner of the document (A3).
const settings = useSettingsStore();
const doctor = useDoctorStore();
const load = () => (settings.settings ? Promise.resolve({ ok: true as const, data: settings.settings }) : settings.load());
const save = (d: Settings) => settings.save({ ...d, toolOverrides: Object.fromEntries(Object.entries(d.toolOverrides).filter(([, v]) => v.trim())) }); // empty = auto-detect

// Fallback until the first doctor report arrives.
const FALLBACK_IDS = ["git", "cargo", "rustc", "node", "pnpm", "cargo-tauri", "claude", "gh", "webkit2gtk", "xdg-open", "libfuse2"];
const tools = computed(() => (doctor.report ? doctor.report.tools.map((t) => ({ id: t.id, name: t.name })) : FALLBACK_IDS.map((id) => ({ id, name: id }))));

async function browse(id: string, draft: Settings, commit: (f: string, v: unknown) => Promise<boolean>) {
  const p = await pickPath(false);
  if (!p) return;
  draft.toolOverrides[id] = p;
  void commit(`toolOverrides.${id}`, p);
}
</script>

<template>
  <SettingsGroup v-slot="{ draft, commit, errorFor }" title="Tool paths" :load="load" :save="save">
    <p class="muted">Leave a field empty to auto-detect the tool.</p>
    <div v-for="t in tools" :key="t.id" class="tool">
      <label :for="`tool-${t.id}`">{{ t.name }} <code class="muted">{{ t.id }}</code></label>
      <div class="row">
        <input
          :id="`tool-${t.id}`" v-model="draft.toolOverrides[t.id]" class="input mono" autocomplete="off" placeholder="auto-detect" :data-field="`toolOverrides.${t.id}`"
          :aria-invalid="!!errorFor(`toolOverrides.${t.id}`)" :aria-describedby="`tool-${t.id}-cap`"
          @blur="commit(`toolOverrides.${t.id}`, (draft.toolOverrides[t.id] ?? '').trim())" @keydown.enter.prevent="commit(`toolOverrides.${t.id}`, (draft.toolOverrides[t.id] ?? '').trim())"
        />
        <button type="button" class="btn" :aria-label="`Browse for ${t.name}`" @click="browse(t.id, draft, commit)">Browse…</button>
      </div>
      <div :id="`tool-${t.id}-cap`">
        <p v-if="errorFor(`toolOverrides.${t.id}`)" class="field-error">{{ errorFor(`toolOverrides.${t.id}`) }}</p>
      </div>
    </div>
  </SettingsGroup>
</template>

<style scoped>
.row { display: flex; gap: 8px; }
.tool { margin-bottom: 10px; }
label { display: block; font-weight: 600; margin-bottom: 4px; }
</style>
