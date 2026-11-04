<script setup lang="ts">
import ConfirmDialog from "./ConfirmDialog.vue";

defineProps<{ appName: string; version: string | null; releasesKept: number }>();
defineEmits<{ confirm: []; cancel: [] }>();
</script>

<template>
  <ConfirmDialog :title="`Uninstall ${appName}?`" confirm-label="Uninstall" @confirm="$emit('confirm')" @cancel="$emit('cancel')">
    <p>
      This removes {{ version ? `${appName} ${version}` : appName }} from GNOME:
    </p>
    <ul>
      <li>the launcher entry</li>
      <li>the app icon</li>
      <li>the {{ releasesKept }} kept release {{ releasesKept === 1 ? "file" : "files" }}</li>
    </ul>
    <p class="muted">Your project files and Git history are untouched.</p>
  </ConfirmDialog>
</template>
