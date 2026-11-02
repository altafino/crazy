<script setup lang="ts">
import { nextTick, ref } from "vue";
import { TriangleAlert } from "lucide-vue-next";
import type { Approval } from "../../lib/chat";
import { approvalDetail } from "../../lib/chat";
import { wantsTo } from "../../lib/notify";

const props = defineProps<{ approval: Approval; cwd: string; busy?: boolean }>();
const emit = defineEmits<{ allow: []; deny: [message: string | null] }>();

const noting = ref(false);
const note = ref("");
const noteEl = ref<HTMLTextAreaElement | null>(null);
const root = ref<HTMLElement | null>(null);
defineExpose({ focus: () => root.value?.focus() });

async function openNote() {
  noting.value = true;
  await nextTick();
  noteEl.value?.focus();
}
// No default button: Enter never approves. A note is sent only with the explicit Deny button.
const deny = () => emit("deny", noting.value && note.value.trim() ? note.value.trim() : null);
</script>

<template>
  <section ref="root" class="approval" tabindex="-1" :aria-label="`Claude wants to ${wantsTo(props.approval.tool)}`">
    <h3><TriangleAlert :size="16" :stroke-width="1.75" aria-hidden="true" /> Claude wants to {{ wantsTo(props.approval.tool) }}</h3>
    <pre class="mono detail">{{ approvalDetail(props.approval.tool, props.approval.input) }}</pre>
    <p v-if="props.approval.description" class="desc">{{ props.approval.description }}</p>
    <p class="caption mono">{{ cwd }}</p>
    <div v-if="noting" class="note">
      <label>
        <span class="sr-only">Note for Claude</span>
        <textarea v-model="note" class="input" rows="2" placeholder="Tell Claude what to do instead" aria-label="Note for Claude" ref="noteEl" />
      </label>
    </div>
    <div class="actions">
      <button v-if="!noting" type="button" class="link" @click="openNote">Deny with a note…</button>
      <span class="spacer" />
      <button type="button" class="btn" :disabled="busy" @click="deny">Deny</button>
      <button type="button" class="btn primary" :disabled="busy" @click="emit('allow')">Allow once</button>
    </div>
  </section>
</template>

<style scoped>
.approval { background: var(--warn-bg); border: 1px solid var(--warn); border-radius: var(--radius); padding: var(--space); }
h3 { display: flex; align-items: center; gap: 8px; margin: 0 0 8px; font-size: 15px; font-weight: 600; color: var(--warn); }
.detail { background: var(--card); color: var(--text); padding: 8px 12px; border-radius: 8px; margin: 0 0 8px; white-space: pre-wrap; overflow-wrap: anywhere; max-height: 240px; overflow: auto; }
.desc { margin: 0 0 4px; }
.caption { font-size: 13px; color: var(--muted); margin: 0 0 12px; }
.note { margin-bottom: 12px; }
.actions { display: flex; gap: 8px; align-items: center; }
.spacer { flex: 1; }
.link { font: inherit; background: none; border: 0; padding: 0; color: var(--accent-text); cursor: pointer; }
.link:hover { text-decoration: underline; }
</style>
