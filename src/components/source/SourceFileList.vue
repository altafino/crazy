<script setup lang="ts">
import { computed, useId } from "vue";
import type { StatusEntry } from "../../bindings";

/** Multi-select listbox (M3-D47): one Tab stop, arrows move and show the diff, Space ticks. */
const props = defineProps<{ entries: StatusEntry[]; ticked: string[]; active: string | null; total: number; truncated: boolean }>();
const emit = defineEmits<{ "update:ticked": [paths: string[]]; select: [path: string] }>();

const id = useId();
const LETTER = { modified: "M", added: "A", deleted: "D", renamed: "R", untracked: "U", conflicted: "!" } as const;
const WORD = { modified: "modified", added: "added", deleted: "deleted", renamed: "renamed", untracked: "new", conflicted: "conflict" } as const;

/** Conflicts first (M3), then the backend order. */
const rows = computed(() => [...props.entries].sort((a, b) => Number(b.kind === "conflicted") - Number(a.kind === "conflicted")));
const on = computed(() => new Set(props.ticked));
const optId = (i: number) => `${id}-o${i}`;
const activeIndex = computed(() => rows.value.findIndex((r) => r.path === props.active));
const allOn = computed(() => rows.value.length > 0 && rows.value.every((r) => on.value.has(r.path)));
const split = (p: string) => {
  const i = p.lastIndexOf("/") + 1;
  return { dir: p.slice(0, i), base: p.slice(i) };
};

function toggle(path: string) {
  emit("update:ticked", on.value.has(path) ? props.ticked.filter((p) => p !== path) : [...props.ticked, path]);
}
function toggleAll() {
  emit("update:ticked", allOn.value ? [] : rows.value.map((r) => r.path));
}
function move(i: number) {
  const r = rows.value[Math.max(0, Math.min(rows.value.length - 1, i))];
  if (r) emit("select", r.path);
}
function onKey(e: KeyboardEvent) {
  const i = activeIndex.value;
  if (e.key === "ArrowDown") move(i + 1);
  else if (e.key === "ArrowUp") move(i < 0 ? 0 : i - 1);
  else if (e.key === "Home") move(0);
  else if (e.key === "End") move(rows.value.length - 1);
  else if (e.key === " " && rows.value[i]) toggle(rows.value[i]!.path);
  else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") toggleAll();
  else return;
  e.preventDefault();
}
</script>

<template>
  <div class="files">
    <div class="head">
      <strong class="tnum">{{ total }} changed {{ total === 1 ? "file" : "files" }}</strong>
      <label class="all"><input type="checkbox" :checked="allOn" @change="toggleAll" /> Select all</label>
    </div>
    <ul
      class="list" role="listbox" aria-multiselectable="true" aria-label="Uncommitted files" tabindex="0"
      :aria-activedescendant="activeIndex >= 0 ? optId(activeIndex) : undefined" @keydown="onKey"
    >
      <li
        v-for="(f, i) in rows" :id="optId(i)" :key="f.path" role="option" class="row"
        :class="{ on: f.path === active }" :aria-selected="on.has(f.path)" :title="f.path" @click="emit('select', f.path)"
      >
        <input type="checkbox" tabindex="-1" aria-hidden="true" :checked="on.has(f.path)" @click.stop @change="toggle(f.path)" />
        <span class="letter" :class="f.kind" aria-hidden="true">{{ LETTER[f.kind] }}</span>
        <span class="sr-only">{{ WORD[f.kind] }}</span>
        <span class="path mono"><span class="dir">{{ split(f.path).dir }}</span><span class="base">{{ split(f.path).base }}</span></span>
        <span v-if="f.kind === 'conflicted'" class="badge warn">conflict</span>
        <span v-if="f.submodule" class="badge">submodule</span>
        <span v-if="f.binary" class="badge">binary</span>
        <span v-if="f.nonUtf8" class="badge warn">odd file name</span>
        <span class="counts tnum">
          <span v-if="f.additions" class="add">+{{ f.additions }}</span>
          <span v-if="f.deletions" class="del">−{{ f.deletions }}</span>
        </span>
      </li>
    </ul>
    <p v-if="truncated" class="cap muted">Showing <span class="tnum">{{ entries.length }}</span> of <span class="tnum">{{ total }}</span> files.</p>
  </div>
</template>

<style scoped>
.files { display: flex; flex-direction: column; min-height: 0; height: 100%; }
.head { display: flex; justify-content: space-between; align-items: center; padding: 8px 12px; border-bottom: 1px solid var(--border); }
.all { display: inline-flex; gap: 6px; align-items: center; cursor: pointer; }
.list { list-style: none; margin: 0; padding: 4px; overflow: auto; flex: 1; min-height: 0; }
.row { display: flex; align-items: center; gap: 8px; border-radius: 6px; padding: 6px 8px; cursor: pointer; }
.row:hover { background: var(--sidebar); }
.row.on { background: var(--accent-strong); color: var(--on-accent); }
.list:focus-visible { outline-offset: -2px; }
.letter { width: 1.2em; font-weight: 700; color: var(--accent-text); flex: none; }
.letter.added, .letter.untracked { color: var(--ok); }
.letter.deleted { color: var(--danger); }
.letter.conflicted { color: var(--warn); }
.row.on .letter { color: inherit; }
.path { display: flex; min-width: 0; flex: 1; }
.dir { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; flex: 0 1 auto; }
.base { white-space: nowrap; flex: none; }
.counts { display: flex; gap: 6px; flex: none; font-size: 0.9em; }
.add { color: var(--ok); }
.del { color: var(--danger); }
.row.on .add, .row.on .del { color: inherit; }
.cap { margin: 0; padding: 6px 12px; border-top: 1px solid var(--border); font-size: 0.9em; }
input[type="checkbox"] { width: 16px; height: 16px; margin: 0; flex: none; accent-color: var(--accent-strong); }
</style>
