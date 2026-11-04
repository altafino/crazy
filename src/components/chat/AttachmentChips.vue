<script setup lang="ts">
import { computed, nextTick, ref } from "vue";
import { FileCode, FileText, FileType, Image as ImageIcon, LoaderCircle, X } from "lucide-vue-next";
import { openPath } from "@tauri-apps/plugin-opener";
import { chipDetail, chipLabel, middleTruncate } from "../../lib/attachments";
import type { Chip } from "../../lib/attachments";
import { useToastStore } from "../../stores/toast";
import Modal from "../Modal.vue";

const props = defineProps<{ chips: Chip[]; removable?: boolean }>();
const emit = defineEmits<{ remove: [key: string]; "focus-input": [] }>();

const toast = useToastStore();
const list = ref<HTMLElement | null>(null);
const shown = computed(() => props.chips.filter((c) => c.visible));
const icons = { image: ImageIcon, pdf: FileText, text: FileCode, docx: FileType };
const busy = (c: Chip) => c.state === "adding" || c.state === "converting";
/** Errors, pending and pruned chips are not clickable; the rest open their stored file. */
const clickable = (c: Chip) => c.state === "ready" && !!c.path;
const press = (c: Chip) => (c.kind === "image" ? `Preview ${c.name}` : `Open ${c.name}`);

// --- image preview (DR-D9): Modal.vue; Esc closes and focus returns to the chip ---
const preview = ref<Chip | null>(null);
let trigger: HTMLElement | null = null;
function activate(c: Chip, e: Event) {
  if (c.kind === "image") {
    trigger = e.currentTarget as HTMLElement;
    preview.value = c;
  } else void openFile(c);
}
function closePreview() {
  preview.value = null;
  void nextTick(() => trigger?.focus());
}
async function openFile(c: Chip) {
  try {
    await openPath(c.path!);
  } catch {
    toast.show(`Couldn't open ${c.name}`);
  }
}

// --- keyboard: Delete/Backspace removes a focused chip, focus goes to the next one or the textarea ---
async function onKey(e: KeyboardEvent, c: Chip, i: number) {
  if (!props.removable || busy(c) || (e.key !== "Delete" && e.key !== "Backspace")) return;
  e.preventDefault();
  emit("remove", c.key);
  await nextTick();
  const next = list.value?.querySelectorAll<HTMLElement>(":scope > li")[i]?.querySelector<HTMLElement>("button");
  if (next) next.focus();
  else emit("focus-input");
}
</script>

<template>
  <ul v-if="shown.length" ref="list" class="chips" role="list" aria-label="Attached files">
    <li
      v-for="(c, i) in shown" :key="c.key" class="chip" :class="{ error: c.state === 'error' }" role="listitem"
      :aria-label="chipLabel(c)" :title="c.name" @keydown="onKey($event, c, i)"
    >
      <component :is="clickable(c) ? 'button' : 'span'" class="body" :type="clickable(c) ? 'button' : undefined" :aria-label="clickable(c) ? press(c) : undefined" @click="clickable(c) && activate(c, $event)">
        <img v-if="c.src" class="thumb" :src="c.src" :alt="c.name" />
        <component :is="icons[c.kind ?? 'text']" v-else class="ico" :size="16" :stroke-width="1.75" aria-hidden="true" />
        <span class="name">{{ middleTruncate(c.name) }}</span>
        <template v-if="c.state === 'error'"><span aria-hidden="true">·</span><span class="reason">{{ c.error }}</span></template>
        <template v-else>
          <LoaderCircle v-if="busy(c)" class="spin" :size="14" :stroke-width="1.75" aria-hidden="true" />
          <span class="detail muted" :class="{ tnum: c.state === 'ready' && c.kind !== 'docx' }">{{ chipDetail(c) }}</span>
        </template>
      </component>
      <button v-if="removable && !busy(c)" type="button" class="x" :aria-label="`Remove ${c.name}`" @click="emit('remove', c.key)">
        <X :size="14" :stroke-width="1.75" aria-hidden="true" />
      </button>
    </li>
  </ul>
  <Modal v-if="preview" :title="preview.name" @close="closePreview">
    <img v-if="preview.src" class="full" :src="preview.src" :alt="preview.name" />
    <p v-else class="muted">Preview isn't available.</p>
    <div class="actions"><button type="button" class="btn" @click="openFile(preview)">Open in default app</button></div>
  </Modal>
</template>

<style scoped>
.chips { list-style: none; margin: 0 0 8px; padding: 0; display: flex; flex-wrap: wrap; gap: 6px; max-height: calc(2 * 28px + 6px); overflow-y: auto; }
.chip { display: inline-flex; align-items: center; height: 28px; max-width: 100%; box-sizing: border-box; border: 1px solid var(--border); border-radius: 8px; background: var(--card); padding: 0 0 0 8px; font-size: 13px; }
.chip.error { background: var(--danger-bg); border-color: var(--danger); color: var(--danger); }
.body { display: inline-flex; align-items: center; gap: 6px; min-width: 0; font: inherit; color: inherit; background: none; border: 0; padding: 0; height: 100%; }
button.body { cursor: pointer; }
.chip:not(:has(.x)) { padding-right: 8px; }
.thumb { width: 24px; height: 24px; border-radius: 4px; object-fit: cover; }
.ico { flex: none; }
.name { white-space: nowrap; }
.detail { white-space: nowrap; }
.reason { white-space: nowrap; }
.spin { animation: spin 1.6s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
.x { display: grid; place-items: center; width: 28px; height: 100%; border: 0; background: none; color: inherit; cursor: pointer; padding: 0; }
.full { display: block; max-width: 100%; max-height: 60vh; margin: 0 auto 12px; object-fit: contain; }
.actions { display: flex; justify-content: flex-end; }
</style>
