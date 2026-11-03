<script setup lang="ts">
import ConfirmDialog from "./ConfirmDialog.vue";

// Stub: lane UI-B owns the real component with this same contract.
defineProps<{ appName: string; version: string | null; releasesKept: number }>();
defineEmits<{ confirm: []; cancel: [] }>();
</script>

<template>
  <ConfirmDialog :title="`Uninstall ${appName}?`" confirm-label="Uninstall" @confirm="$emit('confirm')" @cancel="$emit('cancel')">
    <p>Removes the launcher, the icon and {{ releasesKept }} kept {{ releasesKept === 1 ? "release" : "releases" }}<template v-if="version"> (active: {{ version }})</template>.</p>
    <p class="muted">Your project files and Git history are not touched.</p>
  </ConfirmDialog>
</template>
