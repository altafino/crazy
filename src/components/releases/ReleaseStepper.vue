<script setup lang="ts">
import { computed } from "vue";
import { Check, Circle, Loader } from "lucide-vue-next";
import type { BuildPhase } from "../../bindings";
import { buildSteps } from "./helpers";

const props = defineProps<{ phase: BuildPhase | null | undefined }>();
const steps = computed(() => buildSteps(props.phase));
const icons = { done: Check, active: Loader, pending: Circle } as const;
const words = { done: "done", active: "in progress", pending: "pending" } as const;
type S = keyof typeof icons;
</script>

<template>
  <ol class="stepper" aria-label="Build progress">
    <template v-for="(s, i) in steps" :key="i">
      <li v-if="i" class="sep" aria-hidden="true">—</li>
      <li class="step" :class="s.state">
        <component :is="icons[s.state as S]" :size="14" :stroke-width="1.75" aria-hidden="true" />
        <span>{{ s.label }}</span>
        <span class="sr-only">, {{ words[s.state as S] }}</span>
      </li>
    </template>
  </ol>
</template>

<style scoped>
.stepper { display: flex; gap: 8px; align-items: center; list-style: none; margin: 0; padding: 0; font-size: 13px; flex-wrap: wrap; }
.step { display: flex; align-items: center; gap: 4px; color: var(--muted); }
.sep { color: var(--muted); }
.step.done svg { color: var(--ok); }
.step.active { color: var(--text); }
.step.active svg { color: var(--accent); animation: spin 1.6s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
@media (prefers-reduced-motion: reduce) { .step.active svg { animation: none; } }
</style>
