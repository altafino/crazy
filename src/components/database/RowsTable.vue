<script setup lang="ts">
import { computed } from "vue";
import type { RowsPage } from "../../bindings";
import { num, show } from "../../lib/appdb";

const props = defineProps<{ page: RowsPage }>();
const emit = defineEmits<{ go: [offset: number] }>();

const from = computed(() => props.page.offset + 1);
const to = computed(() => props.page.offset + props.page.rows.length);
const count = computed(() => (props.page.total == null ? "many rows" : `${num(props.page.total)} ${props.page.total === 1 ? "row" : "rows"}`));
const caption = computed(() => `${props.page.table}, ${count.value}, showing ${from.value} to ${to.value}`);
const hasNext = computed(() => (props.page.total == null ? props.page.rows.length >= props.page.pageSize : to.value < props.page.total));
const nextTo = computed(() => {
  const end = props.page.offset + 2 * props.page.pageSize;
  return props.page.total == null ? end : Math.min(end, props.page.total);
});
const prevOffset = computed(() => Math.max(0, props.page.offset - props.page.pageSize));
const range = computed(() => `Rows ${from.value}–${to.value}`);
</script>

<template>
  <div class="rows">
    <div class="scroll" tabindex="0" role="region" :aria-label="`${page.table} rows`">
      <table>
        <caption class="sr-only">{{ caption }}</caption>
        <thead>
          <tr><th v-for="c in page.columns" :key="c" scope="col" class="mono">{{ c }}</th></tr>
        </thead>
        <tbody>
          <tr v-for="(row, i) in page.rows" :key="page.offset + i">
            <td v-for="(cell, j) in row" :key="j" :class="[show(cell).kind]" :title="show(cell).title">
              <span class="v">{{ show(cell).text }}</span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
    <div class="pager">
      <span class="muted tnum">{{ range }} of {{ page.total == null ? "many" : num(page.total) }}</span>
      <span class="spacer" />
      <button type="button" class="btn small" :disabled="page.offset === 0" @click="emit('go', prevOffset)">Previous page</button>
      <button type="button" class="btn small" :disabled="!hasNext" :aria-label="`Next page, rows ${from + page.pageSize} to ${nextTo}`" @click="emit('go', page.offset + page.pageSize)">Next</button>
    </div>
  </div>
</template>

<style scoped>
.rows { display: flex; flex-direction: column; min-height: 0; min-width: 0; flex: 1; }
/* DR-12: wide tables scroll sideways in here only, with sticky headers. */
.scroll { overflow: auto; min-height: 0; flex: 1; }
table { border-collapse: collapse; width: 100%; }
th { position: sticky; top: 0; background: var(--bg); text-align: left; font-size: 13px; font-weight: 400; color: var(--muted); padding: 4px 12px; border-bottom: 1px solid var(--border-strong); white-space: nowrap; }
td { height: 40px; padding: 0 12px; border-bottom: 1px solid var(--border); max-width: 40ch; white-space: nowrap; }
.v { display: block; overflow: hidden; text-overflow: ellipsis; }
td.number { text-align: right; font-variant-numeric: tabular-nums; }
td.null, td.blob { color: var(--muted); }
td.null { font-style: italic; }
.pager { display: flex; gap: 8px; align-items: center; padding: 8px 0; white-space: nowrap; flex-wrap: wrap; }
.spacer { flex: 1; }
</style>
