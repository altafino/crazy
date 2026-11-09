<script setup lang="ts">
import { commands } from "../../bindings";
import type { AttachmentLimits_Serialize as Limits } from "../../bindings";
import { call } from "../../lib/call";
import { ATTACH_RANGES, DEFAULT_ATTACH_LIMITS, validateAttachLimits } from "../../lib/attachments";
import { useToastStore } from "../../stores/toast";
import SettingsGroup from "./SettingsGroup.vue";

const toast = useToastStore();
const fields = Object.keys(ATTACH_RANGES) as (keyof Limits)[];
const load = () => call(commands.getAttachmentLimits());
const save = (d: Limits) => call(commands.setAttachmentLimits(d));
// Inputs hold raw text-or-number so "abc" or "" is reported, not silently coerced.
const num = (v: unknown) => (v === "" ? NaN : Number(v));
/** Only the committed field is checked; the others are filled with valid defaults (Q2). */
const validate = (f: string, v: unknown) => validateAttachLimits({ ...DEFAULT_ATTACH_LIMITS, [f]: v })[f as keyof Limits];

/** Saves the defaults, then offers Undo, which saves the values that were there (D5). */
async function reset(doc: Limits, commitMany: (p: Record<string, unknown>) => Promise<boolean>) {
  const before = { ...doc };
  if (await commitMany({ ...DEFAULT_ATTACH_LIMITS })) toast.show("Attachment limits reset to defaults", { label: "Undo", run: () => void commitMany(before) });
}
</script>

<template>
  <SettingsGroup v-slot="{ doc, draft, commit, commitMany, errorFor }" title="Attachments" :load="load" :save="save" :validate="validate">
    <p class="muted">What a chat prompt may carry. Applies to files attached afterwards.</p>
    <div v-for="k in fields" :key="k" class="lim">
      <label :for="`at-${k}`">{{ ATTACH_RANGES[k].label }}</label>
      <input
        :id="`at-${k}`" v-model.number="draft[k]" type="number" class="input num tnum" inputmode="numeric" :data-field="k"
        :min="ATTACH_RANGES[k].min" :max="ATTACH_RANGES[k].max" :aria-invalid="!!errorFor(k)" :aria-describedby="`at-${k}-h`"
        @blur="commit(k, num(draft[k]))" @keydown.enter.prevent="commit(k, num(draft[k]))"
      />
      <p :id="`at-${k}-h`" class="muted hint">{{ ATTACH_RANGES[k].min.toLocaleString("en-US") }} to {{ ATTACH_RANGES[k].max.toLocaleString("en-US") }}</p>
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
