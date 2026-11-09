<script lang="ts">
/** The plan task a Start run worked on (F15 7B, 7C, 3B). */
export type FromPlan = {
  title: string;
  state: "offer" | "done" | "notDone" | "marked" | "unknown";
  progress?: { done: number; total: number } | null;
  next?: string | null;
};
</script>

<script setup lang="ts">
import type { End } from "../../lib/chat";
import { filesText } from "../../lib/chat";
import { useToastStore } from "../../stores/toast";

defineProps<{ end: End; canRetry: boolean; /** A succeeded Make-changes run: offers Commit… and the preview (M3-D42, D7). */ canCommit?: boolean; checkpointRef?: string | null; fromPlan?: FromPlan | null; /** Start the preview, say it runs, or neither (imported folders). */ preview?: "start" | "running" | null }>();
defineEmits<{ review: []; settings: []; retry: [rule: string]; preview: []; commit: []; build: []; markDone: []; startNext: [] }>();

const toast = useToastStore();
async function copyRef(ref: string) {
  try {
    await navigator.clipboard.writeText(ref);
    toast.show("Copied the checkpoint ref.");
  } catch {
    toast.show("Could not copy.");
  }
}
</script>

<template>
  <div class="end">
    <p class="sentence">
      {{ end.sentence }} <span class="muted tnum">{{ filesText(end.files) }}</span>
      <template v-if="end.action">
        ·
        <template v-if="end.action === 'review'">
          <button type="button" class="btn small primary" @click="$emit('review')">Review changes</button>
          <template v-if="canCommit">
            <button v-if="preview === 'start'" type="button" class="link" @click="$emit('preview')">Start preview</button>
            <span v-else-if="preview === 'running'" class="muted caption">Preview updates by itself</span>
            <button type="button" class="link" @click="$emit('commit')">Commit…</button>
            <button type="button" class="link" @click="$emit('build')">Build release…</button>
          </template>
        </template>
        <button v-else type="button" class="btn small primary" @click="$emit('settings')">Open AI Agents settings</button>
      </template>
    </p>
    <p v-if="fromPlan" class="muted plan-line">
      <template v-if="fromPlan.state === 'marked'">
        <span role="status" aria-live="polite">{{ fromPlan.next || !fromPlan.progress ? `Marked done · ${fromPlan.progress?.done} of ${fromPlan.progress?.total} tasks done` : `All ${fromPlan.progress.total} tasks done` }}</span>
        <button v-if="fromPlan.next" type="button" class="btn small" @click="$emit('startNext')">Start next: {{ fromPlan.next }}</button>
        <button v-else type="button" class="btn small" @click="$emit('build')">Build release…</button>
      </template>
      <template v-else>
        From the plan: {{ fromPlan.title }}<template v-if="fromPlan.state === 'notDone'"> · not done</template><template v-else-if="fromPlan.state === 'done'"> · done</template>
        <button v-if="fromPlan.state === 'offer'" type="button" class="btn small" :aria-label="`Mark ${fromPlan.title} done`" @click="$emit('markDone')">Mark done</button>
      </template>
    </p>
    <div v-if="end.denials.length" class="blocked">
      <h4>Blocked actions</h4>
      <ul>
        <li v-for="(d, i) in end.denials" :key="i">
          <span class="label">{{ d.tool }}</span>
          <code class="mono input-text">{{ typeof d.input === "string" ? d.input : JSON.stringify(d.input) }}</code>
          <button v-if="d.rule" type="button" class="btn small" :disabled="!canRetry" @click="$emit('retry', d.rule!)">Allow and retry</button>
        </li>
      </ul>
    </div>
    <details v-if="end.details || checkpointRef" class="details">
      <summary>Details</summary>
      <pre v-if="end.details" class="mono">{{ end.details }}</pre>
      <p v-if="checkpointRef" class="ref">
        Checkpoint <code class="mono">{{ checkpointRef }}</code>
        <button type="button" class="btn small" @click="copyRef(checkpointRef)">Copy</button>
      </p>
    </details>
  </div>
</template>

<style scoped>
.sentence { margin: 0; font-weight: 600; }
.sentence .muted { font-weight: 400; }
.plan-line { margin: 4px 0 0; font-size: 13px; display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.blocked h4 { font-size: 15px; font-weight: 600; margin: 12px 0 4px; }
ul { list-style: none; margin: 0; padding: 0; }
li { display: flex; align-items: center; gap: 8px; min-height: 32px; flex-wrap: wrap; }
.label { font-weight: 600; }
.input-text { flex: 1; min-width: 0; overflow-wrap: anywhere; color: var(--muted); }
.link { font: inherit; background: none; border: 0; padding: 0 4px; color: var(--accent-text); cursor: pointer; font-weight: 400; }
.link:hover { text-decoration: underline; }
.caption { font-size: 13px; font-weight: 400; padding: 0 4px; }
.ref { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin: 8px 0 0; }
.ref code { overflow-wrap: anywhere; }
.details { margin-top: 8px; font-size: 13px; }
.details summary { cursor: pointer; color: var(--muted); }
.details pre { background: var(--sidebar); padding: 8px 12px; border-radius: 8px; white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
