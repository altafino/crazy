<script setup lang="ts">
import { ref } from "vue";
import type { Shot } from "../bindings";
import Modal from "./Modal.vue";

/** A row of screenshot thumbnails; one opens them large. */
defineProps<{ shots: Shot[]; name: string }>();
const open = ref<number | null>(null);
</script>

<template>
  <!-- Clicks stay here: the project card around it opens the project on click. -->
  <div class="shots" @click.stop>
    <button
      v-for="(s, i) in shots" :key="s.path" type="button" class="thumb"
      :aria-label="`Screenshot ${i + 1} of ${name}`" @click="open = i"
    >
      <img :src="s.src" alt="" loading="lazy" />
    </button>
    <Modal v-if="open !== null" :title="`${name}: screenshot ${open + 1} of ${shots.length}`" wide @close="open = null">
      <img class="large" :src="shots[open]!.src" :alt="`Screenshot ${open + 1} of ${name}`" />
      <div class="viewer-foot">
        <button v-if="shots.length > 1" type="button" class="btn small" @click="open = (open! + shots.length - 1) % shots.length">Previous</button>
        <button v-if="shots.length > 1" type="button" class="btn small" @click="open = (open! + 1) % shots.length">Next</button>
        <button type="button" class="btn small primary close" @click="open = null">Close</button>
      </div>
    </Modal>
  </div>
</template>

<style scoped>
.shots { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 6px; }
.thumb { padding: 0; border: 1px solid var(--border); border-radius: 6px; overflow: hidden; background: var(--bg); cursor: zoom-in; aspect-ratio: 16 / 10; }
.thumb:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
.thumb img { width: 100%; height: 100%; object-fit: cover; object-position: top; display: block; }
.large { display: block; max-width: 100%; max-height: 70vh; margin: 0 auto; border: 1px solid var(--border); border-radius: 6px; }
.viewer-foot { display: flex; gap: 8px; margin-top: 12px; }
.close { margin-left: auto; }
</style>
