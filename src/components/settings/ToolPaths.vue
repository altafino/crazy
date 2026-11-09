<script setup lang="ts">
import { computed, reactive, watch } from "vue";
import type { Settings } from "../../bindings";
import { pickPath } from "../../lib/pick";
import { useDoctorStore } from "../../stores/doctor";
import { useNavStore } from "../../stores/nav";
import { useSettingsStore } from "../../stores/settings";
import SettingsGroup from "./SettingsGroup.vue";

/** One row per Doctor tool: the path field plus what Studio really uses, from the Doctor report (D4). */
// Saves through the settings store: it is the one owner of the document (A3). The store re-runs
// the Doctor only when an override changed (R7).
const settings = useSettingsStore();
const doctor = useDoctorStore();
const nav = useNavStore();
const load = () => (settings.settings ? Promise.resolve({ ok: true as const, data: settings.settings }) : settings.load());
const save = (d: Settings) => settings.save({ ...d, toolOverrides: Object.fromEntries(Object.entries(d.toolOverrides).filter(([, v]) => v.trim())) }); // empty = auto-detect

// Fallback until the first doctor report arrives.
const FALLBACK_IDS = ["git", "cargo", "rustc", "node", "pnpm", "cargo-tauri", "claude", "gh", "webkit2gtk", "xdg-open", "libfuse2"];
const tools = computed(() => (doctor.report ? doctor.report.tools.map((t) => ({ id: t.id, name: t.name, s: t })) : FALLBACK_IDS.map((id) => ({ id, name: id, s: undefined }))));

/** Rows whose path was just saved and whose check is still running. */
const rechecking = reactive(new Set<string>());
watch(() => doctor.loading, (l) => l || rechecking.clear());

async function commitTool(id: string, value: string, commit: (f: string, v: unknown) => Promise<boolean>) {
  rechecking.add(id);
  const saved = await commit(`toolOverrides.${id}`, value.trim());
  if (!saved || !doctor.loading) rechecking.delete(id); // nothing sent, or the check already finished
}
async function browse(id: string, commit: (f: string, v: unknown) => Promise<boolean>) {
  const p = await pickPath(false);
  if (p) await commitTool(id, p, commit);
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
          @blur="commitTool(t.id, draft.toolOverrides[t.id] ?? '', commit)" @keydown.enter.prevent="commitTool(t.id, draft.toolOverrides[t.id] ?? '', commit)"
        />
        <button type="button" class="btn" :aria-label="`Browse for ${t.name}`" @click="browse(t.id, commit)">Browse…</button>
      </div>
      <div :id="`tool-${t.id}-cap`" class="cap">
        <p v-if="errorFor(`toolOverrides.${t.id}`)" class="field-error">{{ errorFor(`toolOverrides.${t.id}`) }}</p>
        <p v-if="!t.s || rechecking.has(t.id)" class="muted">Checking…</p>
        <template v-else-if="t.s.state === 'installed'">
          <p v-if="t.s.source === 'override'" class="muted">
            Using your path · {{ t.s.version }}
            <button type="button" class="link" :aria-label="`Clear the path for ${t.name}`" @click="commitTool(t.id, '', commit)">Clear</button>
          </p>
          <p v-else class="muted">Found: {{ t.s.path }} · {{ t.s.version }}</p>
        </template>
        <p v-else-if="t.s.state === 'missing'" class="muted">
          <span class="badge warn">Not found</span>
          <button type="button" class="link" :aria-label="`How to install ${t.name}`" @click="nav.go('doctor')">How to install</button>
        </p>
        <p v-else-if="t.s.state === 'misconfigured'" class="field-error">{{ t.s.reason }}</p>
      </div>
    </div>
  </SettingsGroup>
</template>

<style scoped>
.row { display: flex; gap: 8px; }
.tool { margin-bottom: 12px; }
label { display: block; font-weight: 600; margin-bottom: 4px; }
.cap p { margin: 4px 0 0; font-size: 13px; overflow-wrap: anywhere; }
.link { font: inherit; background: none; border: 0; padding: 0 4px; min-height: 24px; color: var(--accent-text); cursor: pointer; }
.link:hover { text-decoration: underline; }
</style>
