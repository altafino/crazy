<script setup lang="ts">
import { computed } from "vue";
import { Check, Circle, Loader, Minus, X } from "lucide-vue-next";
import type { PreviewPhase } from "../../bindings";
import { stepStates } from "../../lib/preview";

const props = defineProps<{ phase: PreviewPhase | null | undefined; installSkipped: boolean }>();
const steps = computed(() => stepStates(props.phase, props.installSkipped));
const icons = { done: Check, failed: X, active: Loader, pending: Circle, skipped: Minus };
const words = { done: "done", failed: "failed", active: "in progress", pending: "pending", skipped: "skipped" };
</script>

<template>
  <ol class="stepper" aria-label="Preview progress">
    <template v-for="(s, i) in steps" :key="i">
      <li v-if="i" class="sep" aria-hidden="true">—</li>
      <li class="step" :class="s.state">
        <component :is="icons[s.state]" :size="14" :stroke-width="1.75" aria-hidden="true" />
        <span>{{ s.label }}</span>
        <span class="sr-only">, {{ words[s.state] }}</span>
      </li>
    </template>
  </ol>
</template>

<style scoped>
.stepper { display: flex; gap: 8px; align-items: center; list-style: none; margin: 0; padding: 0; font-size: 13px; flex-wrap: wrap; }
.step { display: flex; align-items: center; gap: 4px; color: var(--muted); }
.sep { color: var(--muted); }
.step.done svg { color: var(--ok); }
.step.failed { color: var(--danger); }
.step.active { color: var(--text); }
.step.active svg { color: var(--accent); animation: spin 1.6s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
@media (prefers-reduced-motion: reduce) { .step.active svg { animation: none; } }
</style>
