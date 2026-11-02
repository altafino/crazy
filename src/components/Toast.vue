<script setup lang="ts">
import { onUnmounted, ref, watch } from "vue";
import { useToastStore } from "../stores/toast";

// Shown 5 s, paused on hover or focus; never takes focus (DESIGN.md "Toast").
const toast = useToastStore();
const paused = ref(false);
let timer: ReturnType<typeof setTimeout> | undefined;

function arm() {
  clearTimeout(timer);
  const id = toast.current?.id;
  if (id !== undefined && !paused.value) timer = setTimeout(() => toast.dismiss(id), 5000);
}
watch(() => toast.current?.id, arm);
watch(paused, arm);
onUnmounted(() => clearTimeout(timer));

function act() {
  const t = toast.current;
  if (!t?.action) return;
  toast.dismiss(t.id);
  t.action.run();
}
</script>

<template>
  <div class="region" aria-live="polite">
    <div
      v-if="toast.current" :key="toast.current.id" class="toast"
      @mouseenter="paused = true" @mouseleave="paused = false" @focusin="paused = true" @focusout="paused = false"
    >
      <span>{{ toast.current.text }}</span>
      <button v-if="toast.current.action" type="button" class="link" @click="act">{{ toast.current.action.label }}</button>
    </div>
  </div>
</template>

<style scoped>
.region { position: fixed; left: 0; right: 0; bottom: 16px; display: flex; justify-content: center; pointer-events: none; z-index: 50; }
.toast { pointer-events: auto; max-width: 480px; display: flex; gap: 12px; align-items: center; background: var(--card); border: 1px solid var(--border); border-radius: 10px; padding: 10px 14px; }
.link { font: inherit; background: none; border: 0; padding: 0; color: var(--accent-text); cursor: pointer; font-weight: 600; }
.link:hover { text-decoration: underline; }
</style>
