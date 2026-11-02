<script setup lang="ts">
import type { End } from "../../lib/chat";
import { filesText } from "../../lib/chat";

defineProps<{ end: End; canRetry: boolean }>();
defineEmits<{ review: []; settings: []; retry: [rule: string] }>();
</script>

<template>
  <div class="end">
    <p class="sentence">
      {{ end.sentence }} <span class="muted tnum">{{ filesText(end.files) }}</span>
      <template v-if="end.action">
        ·
        <button v-if="end.action === 'review'" type="button" class="btn small primary" @click="$emit('review')">Review changes</button>
        <button v-else type="button" class="btn small primary" @click="$emit('settings')">Open AI Agents settings</button>
      </template>
    </p>
    <div v-if="end.denials.length" class="blocked">
      <h3>Blocked actions</h3>
      <ul>
        <li v-for="(d, i) in end.denials" :key="i">
          <span class="label">{{ d.tool }}</span>
          <code class="mono input-text">{{ typeof d.input === "string" ? d.input : JSON.stringify(d.input) }}</code>
          <button v-if="d.rule" type="button" class="btn small" :disabled="!canRetry" @click="$emit('retry', d.rule!)">Allow and retry</button>
        </li>
      </ul>
    </div>
    <details v-if="end.details" class="details">
      <summary>Details</summary>
      <pre class="mono">{{ end.details }}</pre>
    </details>
  </div>
</template>

<style scoped>
.sentence { margin: 0; font-weight: 600; }
.sentence .muted { font-weight: 400; }
.blocked h3 { font-size: 15px; font-weight: 600; margin: 12px 0 4px; }
ul { list-style: none; margin: 0; padding: 0; }
li { display: flex; align-items: center; gap: 8px; min-height: 32px; flex-wrap: wrap; }
.label { font-weight: 600; }
.input-text { flex: 1; min-width: 0; overflow-wrap: anywhere; color: var(--muted); }
.details { margin-top: 8px; font-size: 13px; }
.details summary { cursor: pointer; color: var(--muted); }
.details pre { background: var(--sidebar); padding: 8px 12px; border-radius: 8px; white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
