<script setup lang="ts">
import { commands } from "../../bindings";
import { call } from "../../lib/call";
import { useToastStore } from "../../stores/toast";
import SettingsGroup from "./SettingsGroup.vue";

type Limits = { queryTimeLimitSecs: number };
const MIN = 0;
const MAX = 60;
const DEFAULT = 2;
const toast = useToastStore();
const load = () => call(commands.getDatabaseLimits());
const save = (d: Limits) => call(commands.setDatabaseLimits(d));
// Raw text-or-number so "abc" or "" is reported, not silently coerced.
const num = (v: unknown) => (v === "" ? NaN : Number(v));
const validate = (_f: string, v: unknown) => (Number.isInteger(v) && (v as number) >= MIN && (v as number) <= MAX ? null : `Enter a whole number from ${MIN} to ${MAX}.`);

/** Saves the default, then offers Undo, which saves the value that was there (D5). */
async function reset(doc: Limits, commitMany: (p: Record<string, unknown>) => Promise<boolean>) {
  const before = { ...doc };
  if (await commitMany({ queryTimeLimitSecs: DEFAULT })) toast.show("Database limits reset to defaults", { label: "Undo", run: () => void commitMany(before) });
}
</script>

<template>
  <SettingsGroup v-slot="{ doc, draft, commit, commitMany, errorFor }" title="Database tab" :load="load" :save="save" :validate="validate">
    <p class="muted">How long the Database tab may spend on one read, such as counting rows or filtering. Applies at once.</p>
    <div class="lim">
      <label for="db-limit">Query time limit (seconds)</label>
      <input
        id="db-limit" v-model.number="draft.queryTimeLimitSecs" type="number" class="input num tnum" inputmode="numeric" data-field="queryTimeLimitSecs"
        :min="MIN" :max="MAX" :aria-invalid="!!errorFor('queryTimeLimitSecs')" aria-describedby="db-limit-h"
        @blur="commit('queryTimeLimitSecs', num(draft.queryTimeLimitSecs))" @keydown.enter.prevent="commit('queryTimeLimitSecs', num(draft.queryTimeLimitSecs))"
      />
      <p id="db-limit-h" class="muted hint">{{ MIN }} to {{ MAX }}; 0 = no limit</p>
      <p v-if="errorFor('queryTimeLimitSecs')" class="field-error" role="alert">{{ errorFor("queryTimeLimitSecs") }}</p>
    </div>
    <div class="foot"><button type="button" class="btn small" @click="reset(doc, commitMany)">Reset to default</button></div>
  </SettingsGroup>
</template>

<style scoped>
.lim { margin-bottom: 10px; }
label { display: block; font-weight: 600; margin-bottom: 4px; }
.num { width: 140px; }
.hint { margin: 4px 0 0; font-size: 13px; }
.foot { display: flex; gap: 8px; }
</style>
