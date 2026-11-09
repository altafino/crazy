<script setup lang="ts">
import Markdown from "./Markdown.vue";

defineProps<{ plan: string; actions: boolean; busy?: boolean; /** The plan has tasks and no run is active (F15). */ canUse?: boolean; saved?: boolean }>();
defineEmits<{ make: []; revise: []; use: [] }>();
</script>

<template>
  <section class="plan" aria-label="Plan">
    <h4>Plan</h4>
    <Markdown :text="plan" />
    <div v-if="actions" class="actions">
      <button type="button" class="btn" @click="$emit('revise')">Revise plan</button>
      <span v-if="saved" class="muted saved">Saved as project plan</span>
      <button v-else-if="canUse" type="button" class="btn" @click="$emit('use')">Use as project plan</button>
      <button type="button" class="btn primary" :disabled="busy" @click="$emit('make')">Make these changes</button>
    </div>
  </section>
</template>

<style scoped>
.plan { background: var(--card); border: 1px solid var(--border); border-radius: var(--radius); padding: var(--space); }
h4 { margin: 0 0 8px; font-size: 15px; font-weight: 600; }
.saved { align-self: center; font-size: 13px; }
.actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 12px; }
</style>
