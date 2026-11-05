<script setup lang="ts">
import { onMounted, ref } from "vue";
import { commands } from "../../bindings";
import { call } from "../../lib/call";

const MIN = 0;
const MAX = 60;
const DEFAULT = 2;
// Raw text-or-number so "abc" or "" is reported, not silently coerced.
const secs = ref<number | string>(DEFAULT);
const error = ref<string | null>(null);
const failure = ref<string | null>(null);
const saved = ref(false);

onMounted(async () => {
  const r = await call(commands.getDatabaseLimits());
  if (r.ok) secs.value = r.data.queryTimeLimitSecs;
});

async function save() {
  saved.value = false;
  failure.value = null;
  const n = secs.value === "" ? NaN : Number(secs.value);
  error.value = Number.isInteger(n) && n >= MIN && n <= MAX ? null : `Enter a whole number from ${MIN} to ${MAX}.`;
  if (error.value) return;
  const r = await call(commands.setDatabaseLimits({ queryTimeLimitSecs: n }));
  if (r.ok) {
    secs.value = r.data.queryTimeLimitSecs;
    saved.value = true;
  } else {
    const f = r.error.fieldErrors[0];
    if (f) error.value = f.message;
    else failure.value = r.error.message;
  }
}
function reset() {
  secs.value = DEFAULT;
  return save();
}
</script>

<template>
  <div class="card" @keydown.enter.prevent="save">
    <h2>Database tab</h2>
    <p class="muted">How long the Database tab may spend on one read, such as counting rows or filtering. Applies at once.</p>
    <div class="lim">
      <label for="db-limit">Query time limit (seconds)</label>
      <input
        id="db-limit" v-model.number="secs" type="number" class="input num tnum" inputmode="numeric" :min="MIN" :max="MAX"
        :aria-invalid="!!error" aria-describedby="db-limit-h"
      />
      <p id="db-limit-h" class="muted hint">{{ MIN }} to {{ MAX }}; 0 = no limit</p>
      <p v-if="error" class="field-error" role="alert">{{ error }}</p>
    </div>
    <p v-if="failure" class="callout danger" role="alert">{{ failure }}</p>
    <p aria-live="polite" class="muted">{{ saved ? "Database settings saved." : "" }}</p>
    <div class="row">
      <button type="button" class="btn" @click="save">Save database settings</button>
      <button type="button" class="btn" @click="reset">Reset to default</button>
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
