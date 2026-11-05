<script setup lang="ts">
import { computed } from "vue";
import { iconOf } from "../lib/projectIcons";

// Decorative: the project name always sits next to it.
const props = withDefaults(defineProps<{ projectId: string; name: string; size?: number }>(), { size: 32 });
const src = computed(() => iconOf(props.projectId));
</script>

<template>
  <img v-if="src" class="app-icon" :src="src" alt="" aria-hidden="true" :width="size" :height="size" />
  <span v-else class="app-icon letter" aria-hidden="true" :style="{ width: `${size}px`, height: `${size}px`, fontSize: `${Math.round(size * 0.45)}px` }">{{ name.charAt(0).toUpperCase() }}</span>
</template>

<style scoped>
/* The picture as the launcher shows it: no rounding or crop on top of it. */
.app-icon { flex: none; display: block; object-fit: contain; }
.letter { border-radius: 22%; background: var(--info-bg); color: var(--accent-text); display: grid; place-items: center; font-weight: 700; }
</style>
