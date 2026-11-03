<script setup lang="ts">
import { computed, ref } from "vue";
import { Check, ChevronDown, GitBranch } from "lucide-vue-next";
import { commands } from "../../bindings";
import type { HeadState } from "../../bindings";
import { sourceErrorText, src } from "./errors";
import { useToastStore } from "../../stores/toast";

const props = defineProps<{ projectId: string; head: HeadState; locked: boolean; lockReason: string }>();
const emit = defineEmits<{ changed: [] }>();
const toast = useToastStore();

const open = ref(false);
const branches = ref<string[]>([]);
const wholeRepo = ref(false);
const filter = ref("");
const creating = ref(false);
const newName = ref("");
const error = ref<string | null>(null);

const label = computed(() =>
  props.head.kind === "detached" ? `Detached at ${props.head.short}` : props.head.kind === "unborn" ? `${props.head.name} (no commits yet)` : props.head.name,
);
const current = computed(() => (props.head.kind === "branch" ? props.head.name : null));
const shown = computed(() => branches.value.filter((b) => b.toLowerCase().includes(filter.value.toLowerCase())));

async function toggle() {
  open.value = !open.value;
  if (!open.value) return;
  error.value = null;
  creating.value = false;
  filter.value = "";
  const r = await src(commands.sourceListBranches(props.projectId));
  if (r.ok) {
    branches.value = r.data.branches;
    wholeRepo.value = r.data.affectsWholeRepo;
  } else error.value = sourceErrorText(r.error);
}
async function act(kind: "switch" | "create", name: string) {
  error.value = null;
  const r = await src(kind === "switch" ? commands.sourceSwitchBranch(props.projectId, name) : commands.sourceCreateBranch(props.projectId, name));
  if (!r.ok) {
    error.value = sourceErrorText(r.error);
    return;
  }
  open.value = false;
  newName.value = "";
  toast.show(kind === "switch" ? `Switched to ${name}` : `Created and switched to ${name}`);
  emit("changed");
}
</script>

<template>
  <div class="branch" @keydown.esc="open = false">
    <button type="button" class="input trigger" aria-haspopup="true" :aria-expanded="open" @click="toggle">
      <GitBranch :size="16" aria-hidden="true" /> <span class="name mono">{{ label }}</span> <ChevronDown :size="16" aria-hidden="true" />
    </button>
    <div v-if="open" class="menu card" role="group" aria-label="Branches">
      <p v-if="locked" class="muted note">{{ lockReason }}</p>
      <form v-if="creating" class="new" @submit.prevent="act('create', newName.trim())">
        <label>New branch name <input v-model="newName" class="input" autofocus /></label>
        <button type="submit" class="btn small primary" :disabled="!newName.trim()">Create</button>
      </form>
      <button v-else type="button" class="item" @click="creating = true">New branch…</button>
      <label v-if="branches.length > 10" class="filter"><span class="sr-only">Filter branches</span>
        <input v-model="filter" class="input" placeholder="Filter branches" />
      </label>
      <ul class="list">
        <li v-for="b in shown" :key="b">
          <button type="button" class="item" :disabled="locked || b === current" :aria-current="b === current" @click="act('switch', b)">
            <Check :size="14" :style="{ visibility: b === current ? 'visible' : 'hidden' }" aria-hidden="true" /> <span class="mono">{{ b }}</span>
          </button>
        </li>
      </ul>
      <p v-if="wholeRepo" class="muted note">Switching changes the whole repository, not just this folder.</p>
    </div>
    <p v-if="error" class="field-error" role="alert" aria-live="assertive">{{ error }}</p>
  </div>
</template>

<style scoped>
.branch { position: relative; }
.trigger { display: inline-flex; align-items: center; gap: 8px; width: auto; cursor: pointer; text-align: left; }
.name { max-width: 24ch; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.menu { position: absolute; z-index: 5; top: calc(100% + 4px); left: 0; width: 280px; padding: 6px; background: var(--card); border: 1px solid var(--border); border-radius: var(--radius); }
.list { list-style: none; margin: 4px 0 0; padding: 0; max-height: 240px; overflow: auto; }
.item { display: flex; gap: 8px; align-items: center; width: 100%; font: inherit; background: none; border: 0; border-radius: 6px; padding: 6px 8px; color: var(--text); text-align: left; cursor: pointer; }
.item:hover:not(:disabled) { background: var(--sidebar); }
.item:disabled { cursor: default; }
.item:disabled:not([aria-current="true"]) { opacity: 0.5; }
.new, .filter { display: flex; flex-direction: column; gap: 6px; padding: 4px 2px; }
.note { margin: 6px 8px; font-size: 0.9em; }
</style>
