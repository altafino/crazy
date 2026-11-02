<script setup lang="ts">
import { ref } from "vue";
import type { Identity } from "../../bindings";

/** Inline "who are you" form (M3-D12); saved with `git config --local` by the backend. */
const props = defineProps<{ initial?: Partial<Identity>; busy?: boolean; submitLabel: string }>();
const emit = defineEmits<{ submit: [identity: Identity] }>();

const name = ref(props.initial?.name ?? "");
const email = ref(props.initial?.email ?? "");
const valid = () => name.value.trim() !== "" && /^\S+@\S+$/.test(email.value.trim());
function go() {
  if (valid() && !props.busy) emit("submit", { name: name.value.trim(), email: email.value.trim() });
}
</script>

<template>
  <form class="callout identity" aria-label="Your Git name and email" @submit.prevent="go">
    <p>Git needs to know who is committing. This is saved for this project only.</p>
    <label>Your name <input v-model="name" class="input" autocomplete="name" /></label>
    <label>Your email <input v-model="email" class="input" type="email" autocomplete="email" /></label>
    <button type="submit" class="btn primary" :disabled="!valid() || busy">{{ submitLabel }}</button>
  </form>
</template>

<style scoped>
.identity { display: flex; flex-direction: column; gap: 8px; align-items: flex-start; color: var(--text); }
.identity p { margin: 0; }
.identity label { display: flex; flex-direction: column; gap: 2px; width: 100%; }
</style>
