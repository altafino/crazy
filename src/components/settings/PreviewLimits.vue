<script setup lang="ts">
import { onMounted, reactive, ref } from "vue";
import { commands } from "../../bindings";
import type { PreviewLimits } from "../../bindings";
import { call } from "../../lib/call";
import { DEFAULT_LIMITS, LIMIT_RANGES, validateLimits } from "../../lib/preview";

// Inputs hold raw text-or-number so "abc" or "" is reported, not silently coerced.
const form = reactive<Record<keyof PreviewLimits, number | string>>({ ...DEFAULT_LIMITS });
const errors = ref<Partial<Record<keyof PreviewLimits, string>>>({});
const failure = ref<string | null>(null);
const saved = ref(false);
const fields = Object.keys(LIMIT_RANGES) as (keyof PreviewLimits)[];

onMounted(async () => {
  const r = await call(commands.getPreviewSettings());
  if (r.ok) Object.assign(form, r.data);
});

async function save() {
  saved.value = false;
  failure.value = null;
  const n = Object.fromEntries(fields.map((k) => [k, form[k] === "" ? NaN : Number(form[k])])) as Record<keyof PreviewLimits, number>;
  errors.value = validateLimits(n);
  if (Object.keys(errors.value).length) return;
  const r = await call(commands.savePreviewSettings(n));
  if (r.ok) {
    Object.assign(form, r.data);
    saved.value = true;
  } else {
    for (const e of r.error.fieldErrors) if (fields.includes(e.field as keyof PreviewLimits)) errors.value[e.field as keyof PreviewLimits] = e.message;
    if (!Object.keys(errors.value).length) failure.value = r.error.message;
  }
}
function reset() {
  Object.assign(form, DEFAULT_LIMITS);
  errors.value = {};
  saved.value = false;
  return save(); // resetting means applying the defaults
}
</script>

<template>
  <div class="card" @keydown.enter.prevent="save">
    <h2>Preview</h2>
    <p class="muted">How much of a preview's output Studio keeps. Applies to previews started afterwards.</p>
    <div v-for="k in fields" :key="k" class="lim">
      <label :for="`pv-${k}`">{{ LIMIT_RANGES[k].label }}</label>
      <input
        :id="`pv-${k}`" v-model.number="form[k]" type="number" class="input num tnum" inputmode="numeric"
        :min="LIMIT_RANGES[k].min" :max="LIMIT_RANGES[k].max" :aria-invalid="!!errors[k]" :aria-describedby="`pv-${k}-h`"
      />
      <p :id="`pv-${k}-h`" class="muted hint">{{ LIMIT_RANGES[k].min.toLocaleString("en-US") }} to {{ LIMIT_RANGES[k].max.toLocaleString("en-US") }}</p>
      <p v-if="errors[k]" class="field-error" role="alert">{{ errors[k] }}</p>
    </div>
    <p v-if="failure" class="callout danger" role="alert">{{ failure }}</p>
    <p aria-live="polite" class="muted">{{ saved ? "Preview settings saved." : "" }}</p>
    <div class="row">
      <button type="button" class="btn" @click="save">Save preview settings</button>
      <button type="button" class="btn" @click="reset">Reset to defaults</button>
    </div>
  </div>
</template>

<style scoped>
.card { background: var(--card); border: 1px solid var(--border); border-radius: var(--radius); padding: var(--space); margin-bottom: var(--space); }
.lim { margin-bottom: 10px; }
label { display: block; font-weight: 600; margin-bottom: 4px; }
.num { width: 140px; }
.hint { margin: 4px 0 0; font-size: 13px; }
.row { display: flex; gap: 8px; }
</style>
