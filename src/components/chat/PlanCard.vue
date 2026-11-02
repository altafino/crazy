<script setup lang="ts">
import Markdown from "./Markdown.vue";

defineProps<{ plan: string; actions: boolean; busy?: boolean }>();
defineEmits<{ make: []; revise: [] }>();
</script>

<template>
  <section class="plan" aria-label="Plan">
    <h3>Plan</h3>
    <Markdown :text="plan" />
    <div v-if="actions" class="actions">
      <button type="button" class="btn" @click="$emit('revise')">Revise plan</button>
      <button type="button" class="btn primary" :disabled="busy" @click="$emit('make')">Make these changes</button>
    </div>
  </section>
</template>

<style scoped>
.plan { background: var(--card); border: 1px solid var(--border); border-radius: var(--radius); padding: var(--space); }
h3 { margin: 0 0 8px; font-size: 15px; font-weight: 600; }
.actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 12px; }
</style>
