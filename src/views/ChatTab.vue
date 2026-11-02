<script setup lang="ts">
// Placeholder: built by the Chat lane (T10).
import type { ProjectSummary } from "../bindings";
defineProps<{ project: ProjectSummary }>();
defineEmits<{ "review-changes": [] }>();
</script>

<template>
  <p class="muted">Chat</p>
</template>
