<script setup lang="ts">
import { computed } from "vue";
import { openUrl } from "@tauri-apps/plugin-opener";
import { renderMarkdown } from "../../lib/markdown";

const props = defineProps<{ text: string }>();
// Safe: markdown-it runs with html:false and the renderer escapes everything else (D60).
const html = computed(() => renderMarkdown(props.text));

function onClick(e: MouseEvent) {
  const a = e.target instanceof Element ? e.target.closest<HTMLElement>("a[data-url]") : null;
  if (!a) return;
  e.preventDefault();
  void openUrl(a.dataset.url!).catch(() => {});
}
</script>

<template>
  <!-- eslint-disable-next-line vue/no-v-html -->
  <div class="md" v-html="html" @click="onClick" />
</template>

<style scoped>
.md :deep(p) { margin: 0 0 8px; }
.md :deep(p:last-child) { margin-bottom: 0; }
.md :deep(ul), .md :deep(ol) { margin: 0 0 8px; padding-left: 24px; }
.md :deep(code) { font-family: var(--mono); font-size: 13.5px; background: var(--sidebar); padding: 0 4px; border-radius: 6px; }
.md :deep(pre) { font-family: var(--mono); font-size: 13.5px; background: var(--sidebar); padding: 8px 12px; border-radius: 8px; overflow-x: auto; margin: 0 0 8px; }
.md :deep(pre code) { background: none; padding: 0; }
.md :deep(.md-link) { color: var(--accent-text); }
.md :deep(.md-url) { color: var(--muted); font-size: 13px; overflow-wrap: anywhere; }
</style>
