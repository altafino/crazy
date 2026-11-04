<script setup lang="ts">
import { useId } from "vue";
import type { CommitInfo } from "../../bindings";
import { relTime } from "./time";

/** M5 commit list: semantic list, 40px rows, hash, subject, time (caption line under 900px). */
defineProps<{ heading: string; commits: CommitInfo[] }>();
const id = useId();
</script>

<template>
  <section class="commits" :aria-labelledby="id">
    <h3 :id="id">{{ heading }}</h3>
    <ul>
      <li v-for="c in commits" :key="c.hash">
        <span class="mono hash">{{ c.short }}</span>
        <span class="subject" :title="c.subject">{{ c.subject }}</span>
        <span class="when muted">{{ relTime(c.time * 1000) }}</span>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.commits h3 { margin: 0 0 4px; font-size: 1em; font-weight: 600; }
ul { list-style: none; margin: 0; padding: 0; }
li { display: grid; grid-template-columns: 72px minmax(0, 1fr) auto; gap: 8px; align-items: center; min-height: 40px; border-bottom: 1px solid var(--border); }
li:last-child { border-bottom: 0; }
.subject { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.when { font-size: 13px; font-variant-numeric: tabular-nums; }
@media (max-width: 899px) {
  li { grid-template-columns: 72px minmax(0, 1fr); padding: 4px 0; }
  .when { grid-column: 2; }
}
</style>
