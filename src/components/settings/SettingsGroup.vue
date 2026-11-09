<script setup lang="ts" generic="T extends object">
import { onBeforeUnmount, onMounted, reactive, ref, useId } from "vue";
import { CircleCheck } from "lucide-vue-next";
import type { Outcome } from "../../lib/call";

/*
 * One bordered Settings box: fields save themselves (design D3, eng D4).
 *
 *   field blur/Enter/change ─▶ validate(field, value) ─▶ commit(field, value)
 *        │ invalid ─▶ errorFor(field), nothing sent
 *        ▼
 *   queue (one save at a time) ─▶ doc = lastSaved ⊕ {field: value} ─▶ save(doc)
 *        │ ok ─▶ lastSaved = result; "Saved" for 2 s        │ fieldErrors ─▶ errorFor(field)
 *        │                                                  │ other error ─▶ callout + Retry
 *
 * Each queued save merges only its own fields into the latest saved document, after the previous
 * save has resolved, so two quick commits never overwrite each other (A1). Esc in a field
 * (marked data-field="<path>") puts the saved value back. A path may be dotted: "toolOverrides.git".
 * ponytail: queued commits for the same field are not coalesced; fine at human typing speed.
 */
const props = defineProps<{
  title: string;
  load: () => Promise<Outcome<T>>;
  save: (doc: T) => Promise<Outcome<T>>;
  /** Message for a bad value, or nothing when it is fine. Only the committed field is checked (Q2). */
  validate?: (field: string, value: unknown) => string | null | undefined;
}>();

defineSlots<{
  default(p: {
    /** The last saved document. */
    doc: T;
    /** Editable copy: bind inputs here; commit when the edit is done. */
    draft: T;
    /** Saves one field. Resolves true when it was sent and stored. */
    commit: (field: string, value: unknown) => Promise<boolean>;
    /** Saves several fields as one save (Reset, Undo). */
    commitMany: (patch: Record<string, unknown>) => Promise<boolean>;
    errorFor: (field: string) => string | undefined;
  }): unknown;
}>();

const hid = useId();
// The documents are plain JSON of an unknown shape; paths are walked by hand.
/* eslint-disable @typescript-eslint/no-explicit-any */
const saved = ref<any>(null);
const draft = ref<any>(null);
let optimistic: any = null;
const loading = ref(true);
const loadError = ref<string | null>(null);
const errors = reactive<Record<string, string>>({});
const failure = ref<{ message: string; patch: Record<string, unknown> } | null>(null);
const showSaving = ref(false);
const showSaved = ref(false);

const get = (o: any, path: string) => path.split(".").reduce((a, k) => a?.[k], o);
function set(o: any, path: string, v: unknown) {
  const ks = path.split(".");
  const last = ks.pop()!;
  let t = o;
  for (const k of ks) t = t[k] ??= {};
  t[last] = v;
}
const clone = <X,>(x: X): X => JSON.parse(JSON.stringify(x));
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

async function doLoad() {
  loading.value = true;
  loadError.value = null;
  const r = await props.load();
  if (r.ok) {
    saved.value = clone(r.data);
    draft.value = clone(r.data);
    optimistic = clone(r.data);
  } else loadError.value = r.error.message;
  loading.value = false;
}
onMounted(doLoad);

let savingTimer: ReturnType<typeof setTimeout> | undefined;
let savedTimer: ReturnType<typeof setTimeout> | undefined;
onBeforeUnmount(() => {
  clearTimeout(savingTimer);
  clearTimeout(savedTimer);
});

let pending = 0;
let tail: Promise<boolean> = Promise.resolve(true);

async function doSave(patch: Record<string, unknown>): Promise<boolean> {
  const next = clone(saved.value);
  for (const [f, v] of Object.entries(patch)) set(next, f, v);
  let r: Outcome<T>;
  try {
    r = await props.save(next);
  } catch (e) {
    r = { ok: false, error: { message: e instanceof Error ? e.message : String(e), fieldErrors: [] } };
  }
  if (--pending === 0) {
    clearTimeout(savingTimer);
    savingTimer = undefined;
    showSaving.value = false;
  }
  if (r.ok) {
    saved.value = clone(r.data);
    for (const f of Object.keys(patch)) set(draft.value, f, get(saved.value, f) ?? "");
    failure.value = null;
    showSaved.value = true;
    clearTimeout(savedTimer);
    savedTimer = setTimeout(() => (showSaved.value = false), 2000);
    return true;
  }
  optimistic = clone(saved.value);
  if (r.error.fieldErrors.length) for (const e of r.error.fieldErrors) errors[e.field] = e.message;
  else failure.value = { message: r.error.message, patch };
  return false;
}

function enqueue(patch: Record<string, unknown>): Promise<boolean> {
  pending++;
  savingTimer ??= setTimeout(() => (showSaving.value = true), 400); // "Saving…" only when it takes a while
  tail = pending > 1 ? tail.then(() => doSave(patch)) : doSave(patch);
  return tail;
}

function commitMany(patch: Record<string, unknown>): Promise<boolean> {
  if (!saved.value) return Promise.resolve(false);
  const changed: Record<string, unknown> = {};
  let invalid = false;
  for (const [f, v] of Object.entries(patch)) {
    const m = props.validate?.(f, v);
    if (m) {
      errors[f] = m;
      invalid = true;
    } else if (same(get(optimistic, f) ?? "", v ?? "")) delete errors[f]; // nothing new to save
    else changed[f] = v;
  }
  if (invalid || !Object.keys(changed).length) return Promise.resolve(false);
  for (const [f, v] of Object.entries(changed)) {
    delete errors[f];
    set(optimistic, f, v);
    set(draft.value, f, v);
  }
  return enqueue(changed);
}
const commit = (field: string, value: unknown) => commitMany({ [field]: value });

function retry() {
  const p = failure.value?.patch;
  failure.value = null;
  if (p) void commitMany(p);
}

function onKeydown(e: KeyboardEvent) {
  if (e.key !== "Escape" || !draft.value) return;
  const f = (e.target as HTMLElement | null)?.dataset?.field;
  if (!f) return;
  set(draft.value, f, get(saved.value, f) ?? "");
  delete errors[f];
}
</script>

<template>
  <div class="card" role="group" :aria-labelledby="hid" @keydown="onKeydown">
    <div class="head">
      <h2 :id="hid">{{ title }}</h2>
      <span class="caption" aria-live="polite">
        <template v-if="showSaving">Saving…</template>
        <span v-else-if="showSaved" class="ok"><CircleCheck :size="14" aria-hidden="true" /> Saved</span>
      </span>
    </div>

    <p v-if="loading" class="muted" role="status">Loading settings…</p>
    <div v-else-if="loadError" class="callout danger" role="alert">
      <p>Couldn't load settings</p>
      <button type="button" class="btn small" @click="doLoad">Retry</button>
      <details class="details"><summary>Details</summary><pre class="mono">{{ loadError }}</pre></details>
    </div>
    <template v-else>
      <slot :doc="saved" :draft="draft" :commit="commit" :commit-many="commitMany" :error-for="(f: string) => errors[f]" />
      <div v-if="failure" class="callout danger" role="alert">
        <p>Couldn't save {{ title }} settings</p>
        <button type="button" class="btn small" @click="retry">Retry</button>
        <details class="details"><summary>Details</summary><pre class="mono">{{ failure.message }}</pre></details>
      </div>
    </template>
  </div>
</template>

<style scoped>
.card { background: var(--card); border: 1px solid var(--border); border-radius: var(--radius); padding: var(--space); margin-bottom: var(--space); }
.head { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; }
.caption { font-size: 13px; color: var(--muted); white-space: nowrap; }
.ok { color: var(--ok); display: inline-flex; align-items: center; gap: 4px; }
.callout { margin: 12px 0 0; }
.callout p { margin: 0 0 8px; font-weight: 600; }
.details pre { margin: 4px 0 0; white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
