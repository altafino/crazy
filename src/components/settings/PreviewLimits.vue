<script setup lang="ts">
import { commands } from "../../bindings";
import type { PreviewLimits } from "../../bindings";
import { call } from "../../lib/call";
import { DEFAULT_LIMITS, LIMIT_RANGES, validateLimits } from "../../lib/preview";
import { useToastStore } from "../../stores/toast";
import SettingsGroup from "./SettingsGroup.vue";

const toast = useToastStore();
const fields = Object.keys(LIMIT_RANGES) as (keyof PreviewLimits)[];
const load = () => call(commands.getPreviewSettings());
const save = (d: PreviewLimits) => call(commands.savePreviewSettings(d));
// Inputs hold raw text-or-number so "abc" or "" is reported, not silently coerced.
const num = (v: unknown) => (v === "" ? NaN : Number(v));
/** Only the committed field is checked; the others are filled with valid defaults (Q2). */
const validate = (f: string, v: unknown) => validateLimits({ ...DEFAULT_LIMITS, [f]: v })[f as keyof PreviewLimits];

/** Saves the defaults, then offers Undo, which saves the values that were there (D5). */
async function reset(doc: PreviewLimits, commitMany: (p: Record<string, unknown>) => Promise<boolean>) {
  const before = { ...doc };
  if (await commitMany({ ...DEFAULT_LIMITS })) toast.show("Preview limits reset to defaults", { label: "Undo", run: () => void commitMany(before) });
}
</script>

<template>
  <SettingsGroup v-slot="{ doc, draft, commit, commitMany, errorFor }" title="Preview" :load="load" :save="save" :validate="validate">
    <p class="muted">How much of a preview's output Studio keeps. Applies to previews started afterwards.</p>
    <div v-for="k in fields" :key="k" class="lim">
      <label :for="`pv-${k}`">{{ LIMIT_RANGES[k].label }}</label>
      <input
        :id="`pv-${k}`" v-model.number="draft[k]" type="number" class="input num tnum" inputmode="numeric" :data-field="k"
        :min="LIMIT_RANGES[k].min" :max="LIMIT_RANGES[k].max" :aria-invalid="!!errorFor(k)" :aria-describedby="`pv-${k}-h`"
        @blur="commit(k, num(draft[k]))" @keydown.enter.prevent="commit(k, num(draft[k]))"
      />
      <p :id="`pv-${k}-h`" class="muted hint">{{ LIMIT_RANGES[k].min.toLocaleString("en-US") }} to {{ LIMIT_RANGES[k].max.toLocaleString("en-US") }}</p>
      <p v-if="errorFor(k)" class="field-error" role="alert">{{ errorFor(k) }}</p>
    </div>
    <div class="foot"><button type="button" class="btn small" @click="reset(doc, commitMany)">Reset to defaults</button></div>
  </SettingsGroup>
</template>

<style scoped>
.lim { margin-bottom: 10px; }
label { display: block; font-weight: 600; margin-bottom: 4px; }
.num { width: 140px; }
.hint { margin: 4px 0 0; font-size: 13px; }
.foot { display: flex; gap: 8px; }
</style>
