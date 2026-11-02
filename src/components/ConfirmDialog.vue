<script setup lang="ts">
import Modal from "./Modal.vue";

defineProps<{ title: string; confirmLabel: string }>();
const emit = defineEmits<{ confirm: []; cancel: [] }>();
</script>

<template>
  <Modal :title="title" @close="emit('cancel')">
    <div class="body"><slot /></div>
    <div class="actions">
      <button type="button" class="btn" @click="emit('cancel')">Cancel</button>
      <button type="button" class="btn primary" @click="emit('confirm')">{{ confirmLabel }}</button>
    </div>
  </Modal>
</template>

<style scoped>
.body { margin-bottom: var(--space); }
.body :deep(p) { margin: 0 0 8px; }
.actions { display: flex; justify-content: flex-end; gap: 8px; }
</style>
