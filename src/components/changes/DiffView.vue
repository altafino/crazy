<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { commands } from "../../bindings";
import type { FileChange, FileDiff } from "../../bindings";
import { call } from "../../lib/call";
import type { Outcome } from "../../lib/call";
import { binaryCopy } from "../../lib/format";

/** `loader` lets the Source tab reuse this view with its own diff command. */
const props = defineProps<{ sessionId: string; file: FileChange; reloadKey?: string; loader?: (full: boolean) => Promise<Outcome<FileDiff>> }>();

const diff = ref<FileDiff | null>(null);
const error = ref<string | null>(null);
/** "Show full diff" was pressed for this file. */
const full = ref(false);

async function load() {
  diff.value = null;
  error.value = null;
  const { sessionId, file } = props;
  const r = await (props.loader ? props.loader(full.value) : call(commands.runFileDiff(sessionId, file.path, full.value)));
  if (sessionId !== props.sessionId || file.path !== props.file.path) return; // stale
  if (r.ok) diff.value = r.data;
  else error.value = r.error.message;
}
watch(() => [props.sessionId, props.file.path, props.reloadKey], () => {
  full.value = false;
  void load();
}, { immediate: true });
function showFull() {
  full.value = true;
  void load();
}

type Row = { head?: string; n?: number; kind?: "context" | "added" | "removed"; text?: string };
const rows = computed<Row[]>(() => {
  const out: Row[] = [];
  for (const h of diff.value?.hunks ?? []) {
    out.push({ head: `@@ -${h.oldStart},${h.oldLines} +${h.newStart},${h.newLines} @@` });
    let o = h.oldStart;
    let n = h.newStart;
    for (const l of h.lines) {
      if (l.kind === "removed") out.push({ kind: l.kind, text: l.text, n: o++ });
      else {
        out.push({ kind: l.kind, text: l.text, n: n++ });
        if (l.kind === "context") o++;
      }
    }
  }
  return out;
});
const MARK = { context: " ", added: "+", removed: "−" } as const;
</script>

<template>
  <section class="diff" :aria-label="`Diff of ${file.path}`">
    <header class="fhead">
      <strong class="mono path">{{ file.path }}</strong>
      <span v-if="file.oldPath" class="muted mono">← {{ file.oldPath }}</span>
      <span class="counts tnum"><span class="add">+{{ file.additions }}</span> <span class="del">−{{ file.deletions }}</span></span>
    </header>
    <div class="body">
      <div v-if="error" class="muted pad">
        Couldn't load this diff.
        <details><summary>Details</summary><pre class="mono">{{ error }}</pre></details>
      </div>
      <p v-else-if="!diff" class="muted pad">Loading…</p>
      <p v-else-if="diff.binary" class="muted pad">{{ binaryCopy(diff.oldSize, diff.newSize) }}</p>
      <p v-else-if="!rows.length" class="muted pad">No line changes to show.</p>
      <template v-else>
        <div class="code mono" aria-label="Diff lines">
          <div v-for="(r, i) in rows" :key="i" class="ln" :class="r.head ? 'hunk' : r.kind">
            <template v-if="r.head"><span class="gut" /><span class="mark" /><span class="txt">{{ r.head }}</span></template>
            <template v-else>
              <span class="gut tnum">{{ r.n }}</span><span class="mark" aria-hidden="true">{{ MARK[r.kind!] }}</span><span class="txt">{{ r.text }}</span>
            </template>
          </div>
        </div>
        <p v-if="diff.truncated" class="cut muted pad">
          {{ full ? "Showing the first 100,000 lines." : "Showing the first 5,000 lines." }}
          <button v-if="!full" type="button" class="btn small" @click="showFull">Show full diff</button>
        </p>
      </template>
    </div>
  </section>
</template>

<style scoped>
.diff { display: flex; flex-direction: column; min-height: 0; min-width: 0; height: 100%; }
.fhead { display: flex; gap: 12px; align-items: baseline; padding: 8px 12px; border-bottom: 1px solid var(--border); }
.path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.counts { margin-left: auto; flex: none; }
.add { color: var(--ok); }
.del { color: var(--danger); }
.body { overflow: auto; flex: 1; min-height: 0; }
.pad { padding: 12px; margin: 0; }
pre { white-space: pre-wrap; margin: 4px 0 0; }
.code { min-width: max-content; }
.ln { display: flex; white-space: pre; }
.gut { width: 48px; flex: none; text-align: right; padding-right: 8px; color: var(--muted); }
.mark { width: 1.5em; flex: none; text-align: center; }
.txt { flex: 1; padding-right: 12px; }
.added { background: var(--ok-bg); }
.removed { background: var(--danger-bg); }
.hunk { background: var(--sidebar); color: var(--muted); font-size: 0.85em; }
</style>
