<script setup lang="ts">
import { onMounted, reactive, ref } from "vue";
import { commands } from "../../bindings";
import type { AttachmentLimits_Serialize as Limits } from "../../bindings";
import { call } from "../../lib/call";
import { ATTACH_RANGES, DEFAULT_ATTACH_LIMITS, validateAttachLimits } from "../../lib/attachments";

// Inputs hold raw text-or-number so "abc" or "" is reported, not silently coerced.
const form = reactive<Record<keyof Limits, number | string>>({ ...DEFAULT_ATTACH_LIMITS });
const errors = ref<Partial<Record<keyof Limits, string>>>({});
const failure = ref<string | null>(null);
const saved = ref(false);
const fields = Object.keys(ATTACH_RANGES) as (keyof Limits)[];

onMounted(async () => {
  const r = await call(commands.getAttachmentLimits());
  if (r.ok) Object.assign(form, r.data);
});

async function save() {
  saved.value = false;
  failure.value = null;
  const n = Object.fromEntries(fields.map((k) => [k, form[k] === "" ? NaN : Number(form[k])])) as Record<keyof Limits, number>;
  errors.value = validateAttachLimits(n);
  if (Object.keys(errors.value).length) return;
  const r = await call(commands.setAttachmentLimits(n));
  if (r.ok) {
    Object.assign(form, r.data);
    saved.value = true;
  } else {
    for (const e of r.error.fieldErrors) if (fields.includes(e.field as keyof Limits)) errors.value[e.field as keyof Limits] = e.message;
    if (!Object.keys(errors.value).length) failure.value = r.error.message;
  }
}
function reset() {
  Object.assign(form, DEFAULT_ATTACH_LIMITS);
  errors.value = {};
  saved.value = false;
  return save(); // resetting means applying the defaults
}
</script>

<template>
  <div class="card" @keydown.enter.prevent="save">
    <h2>Attachments</h2>
    <p class="muted">What a chat prompt may carry. Applies to files attached afterwards.</p>
    <div v-for="k in fields" :key="k" class="lim">
      <label :for="`at-${k}`">{{ ATTACH_RANGES[k].label }}</label>
      <input
        :id="`at-${k}`" v-model.number="form[k]" type="number" class="input num tnum" inputmode="numeric"
        :min="ATTACH_RANGES[k].min" :max="ATTACH_RANGES[k].max" :aria-invalid="!!errors[k]" :aria-describedby="`at-${k}-h`"
      />
      <p :id="`at-${k}-h`" class="muted hint">{{ ATTACH_RANGES[k].min.toLocaleString("en-US") }} to {{ ATTACH_RANGES[k].max.toLocaleString("en-US") }}</p>
      <p v-if="errors[k]" class="field-error" role="alert">{{ errors[k] }}</p>
    </div>
    <p v-if="failure" class="callout danger" role="alert">{{ failure }}</p>
    <p aria-live="polite" class="muted">{{ saved ? "Attachment settings saved." : "" }}</p>
    <div class="row">
      <button type="button" class="btn" @click="save">Save attachment settings</button>
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
