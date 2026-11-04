<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { Copy } from "lucide-vue-next";
import { useToastStore } from "../../stores/toast";

const props = defineProps<{ lines: string[]; firstSeq: number; truncated?: boolean; label?: string }>();
const toast = useToastStore();

const follow = ref(true);
const query = ref("");
const pane = ref<HTMLElement | null>(null);
const find = ref<HTMLInputElement | null>(null);

// Auto-scroll pauses when the user scrolls up and resumes at the bottom.
const atBottom = (el: HTMLElement) => el.scrollHeight - el.scrollTop - el.clientHeight < 24;
function onScroll() {
  if (pane.value) follow.value = atBottom(pane.value);
}
function toBottom() {
  const el = pane.value;
  if (el) el.scrollTop = el.scrollHeight;
}
watch(() => props.lines.length, async () => {
  if (!follow.value) return;
  await nextTick();
  toBottom();
});
watch(follow, async (on) => {
  if (!on) return;
  await nextTick();
  toBottom();
});

const kind = (l: string) =>
  /^\s*(error|\berror\[)/i.test(l) ? "err"
    : /^\s*warning\b/i.test(l) ? "warn"
      : /^\s*(Compiling|Downloading|Downloaded|Fresh|Updating|Locking|Blocking|Finished)\b/.test(l) ? "progress" : "";

/** Splits a line around case-insensitive matches of the query so they can be marked. */
function parts(line: string): { text: string; hit: boolean }[] {
  const q = query.value.toLowerCase();
  if (!q) return [{ text: line, hit: false }];
  const out: { text: string; hit: boolean }[] = [];
  const low = line.toLowerCase();
  let at = 0;
  for (let i = low.indexOf(q); i >= 0; i = low.indexOf(q, at)) {
    if (i > at) out.push({ text: line.slice(at, i), hit: false });
    out.push({ text: line.slice(i, i + q.length), hit: true });
    at = i + q.length;
  }
  if (at < line.length) out.push({ text: line.slice(at), hit: false });
  return out;
}
const matches = computed(() => (query.value ? props.lines.filter((l) => l.toLowerCase().includes(query.value.toLowerCase())).length : 0));

async function copy() {
  try {
    await navigator.clipboard.writeText(props.lines.join("\n"));
    toast.show(`Copied ${props.lines.length.toLocaleString("en-US")} lines.`);
  } catch {
    toast.show("Could not copy the log.");
  }
}
// Ctrl+F is scoped to the log: only while focus is inside this view.
function onKey(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
    e.preventDefault();
    find.value?.focus();
    find.value?.select();
  }
}
</script>

<template>
  <section class="log" :aria-label="label ?? 'Preview log'" @keydown="onKey">
    <header class="bar">
      <span class="caption muted tnum">Log · last {{ lines.length.toLocaleString("en-US") }} lines<template v-if="truncated"> · file size limit reached</template></span>
      <label class="findl">
        <span class="sr-only">Find in log</span>
        <input ref="find" v-model="query" type="search" class="input find" placeholder="Find in log" autocomplete="off" @keydown.esc.stop="query = ''" />
      </label>
      <span v-if="query" class="caption muted tnum" aria-live="polite">{{ matches }} {{ matches === 1 ? "match" : "matches" }}</span>
      <label class="switch-l">
        <button type="button" class="switch" role="switch" :aria-checked="follow" @click="follow = !follow"><span class="knob" /></button>
        <span>Follow output</span>
      </label>
      <button type="button" class="btn small icon" aria-label="Copy log" title="Copy log" @click="copy"><Copy :size="14" :stroke-width="1.75" aria-hidden="true" /></button>
    </header>
    <div ref="pane" class="pane mono" role="log" tabindex="0" aria-label="Log lines" @scroll="onScroll">
      <!-- ponytail: renders every kept line (max 50,000); a virtual list if that gets slow. -->
      <div v-for="(l, i) in lines" :key="firstSeq + i" class="ln" :class="kind(l)">
        <span class="no tnum" aria-hidden="true">{{ firstSeq + i + 1 }}</span>
        <span class="txt"><template v-for="(p, j) in parts(l)" :key="j"><mark v-if="p.hit">{{ p.text }}</mark><template v-else>{{ p.text }}</template></template></span>
      </div>
      <p v-if="!lines.length" class="muted empty">No output yet.</p>
    </div>
  </section>
</template>

<style scoped>
.log { background: var(--card); border: 1px solid var(--border); border-radius: 10px; display: flex; flex-direction: column; min-height: 200px; flex: 1; min-width: 0; }
.bar { display: flex; gap: 12px; align-items: center; padding: 8px 12px; border-bottom: 1px solid var(--border); flex-wrap: wrap; }
.caption { font-size: 13px; margin-right: auto; }
.find { width: 200px; padding: 2px 8px; }
.switch-l { display: flex; gap: 8px; align-items: center; font-size: 13px; margin: 0; font-weight: 400; }
.switch { position: relative; width: 40px; height: 22px; border-radius: 11px; border: 0; padding: 0; background: var(--border-strong); cursor: pointer; }
.switch[aria-checked="true"] { background: var(--accent-strong); }
.knob { position: absolute; top: 2px; left: 2px; width: 18px; height: 18px; border-radius: 50%; background: #fff; transition: transform 120ms; }
.switch[aria-checked="true"] .knob { transform: translateX(18px); }
.icon { display: inline-flex; padding: 4px 8px; }
.pane { flex: 1; min-height: 0; overflow: auto; padding: 8px 0; font-size: 13px; }
.ln { display: flex; gap: 12px; padding: 0 12px 0 0; }
.no { flex: none; width: 48px; text-align: right; color: var(--muted); user-select: none; }
.txt { white-space: pre-wrap; overflow-wrap: anywhere; min-width: 0; }
.err { color: var(--danger); }
.warn { color: var(--warn); }
.progress { color: var(--muted); }
mark { background: var(--info-bg); color: inherit; }
.empty { margin: 0; padding: 0 12px; }
</style>
