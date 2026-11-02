<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, useId } from "vue";

defineProps<{ title: string }>();
const emit = defineEmits<{ close: [] }>();

const titleId = useId();
const box = ref<HTMLElement | null>(null);
const opener = document.activeElement as HTMLElement | null;

const FOCUSABLE = "input:not([disabled]), button:not([disabled]), select, textarea, [href]";

onMounted(() => {
  const first = box.value?.querySelector<HTMLElement>(FOCUSABLE);
  (first ?? box.value)?.focus();
});
onBeforeUnmount(() => opener?.focus?.());

function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") {
    e.stopPropagation();
    emit("close");
    return;
  }
  if (e.key !== "Tab" || !box.value) return;
  // Keep Tab inside the dialog.
  const items = [...box.value.querySelectorAll<HTMLElement>(FOCUSABLE)];
  if (!items.length) return;
  const first = items[0]!;
  const last = items[items.length - 1]!;
  if (e.shiftKey && document.activeElement === first) {
    e.preventDefault();
    last.focus();
  } else if (!e.shiftKey && document.activeElement === last) {
    e.preventDefault();
    first.focus();
  }
}
</script>

<template>
  <div class="overlay" @mousedown.self="emit('close')">
    <div ref="box" class="dialog" role="dialog" aria-modal="true" :aria-labelledby="titleId" tabindex="-1" @keydown="onKey">
      <h2 :id="titleId">{{ title }}</h2>
      <slot />
    </div>
  </div>
</template>

<style scoped>
.overlay { position: fixed; inset: 0; background: rgb(0 0 0 / 0.45); display: grid; place-items: center; padding: var(--space); z-index: 10; }
.dialog { background: var(--card); color: var(--text); border-radius: 14px; padding: 24px; width: min(560px, 100%); max-height: 90vh; overflow: auto; border: 1px solid var(--border); }
.dialog:focus { outline: none; }
</style>
