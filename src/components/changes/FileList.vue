<script setup lang="ts">
import type { FileChange } from "../../bindings";

defineProps<{ files: FileChange[]; selected: string | null }>();
const emit = defineEmits<{ select: [path: string] }>();

const LETTER = { added: "A", modified: "M", deleted: "D", renamed: "R", binary: "B", notCaptured: "?" } as const;
/** Middle truncation: the folder part shrinks, the file name stays. */
const split = (p: string) => {
  const i = p.lastIndexOf("/") + 1;
  return { dir: p.slice(0, i), base: p.slice(i) };
};
</script>

<template>
  <div class="files">
    <ul class="list" aria-label="Changed files">
      <li v-for="f in files" :key="f.path">
        <button type="button" class="row" :class="{ on: f.path === selected }" :title="f.path" :aria-current="f.path === selected" @click="emit('select', f.path)">
          <span class="letter" :class="f.status" aria-hidden="true">{{ LETTER[f.status] }}</span>
          <span class="sr-only">{{ f.status }}</span>
          <span class="path mono"><span class="dir">{{ split(f.path).dir }}</span><span class="base">{{ split(f.path).base }}</span></span>
          <span class="counts tnum">
            <span v-if="f.additions" class="add">+{{ f.additions }}</span>
            <span v-if="f.deletions" class="del">−{{ f.deletions }}</span>
          </span>
        </button>
      </li>
    </ul>
    <label class="narrow">
      <span class="sr-only">Changed file</span>
      <select class="input" :value="selected ?? ''" @change="emit('select', ($event.target as HTMLSelectElement).value)">
        <option v-for="f in files" :key="f.path" :value="f.path">{{ f.path }} (+{{ f.additions }} −{{ f.deletions }})</option>
      </select>
    </label>
  </div>
</template>

<style scoped>
.files { height: 100%; min-height: 0; }
.list { list-style: none; margin: 0; padding: 4px; overflow: auto; height: 100%; }
.row { display: flex; align-items: center; gap: 8px; width: 100%; font: inherit; text-align: left; background: none; border: 0; border-radius: 6px; padding: 6px 8px; color: var(--text); cursor: pointer; }
.row:hover { background: var(--sidebar); }
.row.on { background: var(--accent-strong); color: var(--on-accent); }
.letter { width: 1.2em; font-weight: 700; color: var(--accent-text); flex: none; }
.letter.added { color: var(--ok); }
.letter.deleted { color: var(--danger); }
.row.on .letter { color: inherit; }
.path { display: flex; min-width: 0; flex: 1; }
.dir { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; flex: 0 1 auto; }
.base { white-space: nowrap; flex: none; }
.counts { display: flex; gap: 6px; flex: none; font-size: 0.9em; }
.add { color: var(--ok); }
.del { color: var(--danger); }
.row.on .add, .row.on .del { color: inherit; }
.narrow { display: none; }
@media (max-width: 899px) {
  .list { display: none; }
  .narrow { display: block; }
}
</style>
