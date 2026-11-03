<script setup lang="ts">
import { computed, ref } from "vue";
import type { AgentProfile, SessionMode } from "../../bindings";

const props = defineProps<{
  profiles: AgentProfile[];
  noProfile: boolean;
  active: boolean;
  /** A build is live: Make changes is off (M4-DR23). */
  changesBlocked?: boolean;
  status: { tone: "accent" | "warn" | "ok"; text: string; extra: string } | null;
}>();
const prompt = defineModel<string>("prompt", { required: true });
const mode = defineModel<SessionMode>("mode", { required: true });
const profileId = defineModel<string | null>("profileId", { required: true });
const emit = defineEmits<{ run: []; cancel: []; setup: [] }>();

const modes: { id: SessionMode; label: string }[] = [
  { id: "plan", label: "Plan first" },
  { id: "changes", label: "Make changes" },
];
const area = ref<HTMLTextAreaElement | null>(null);
/** The null option runs with the project's or global default; name it ("Claude (default)"). */
const defaultName = computed(() => {
  const d = props.profiles.find((p) => p.isDefault);
  return d ? `${d.name.replace(/\s*\(default\)$/i, "")} (default)` : "Default profile";
});
defineExpose({ focus: () => area.value?.focus() });

function submit() {
  if (props.active || (props.changesBlocked && mode.value === "changes")) return;
  if (props.noProfile) emit("setup");
  else if (prompt.value.trim()) emit("run");
}
function grow(e: Event) {
  const t = e.target as HTMLTextAreaElement;
  t.style.height = "auto";
  t.style.height = `${Math.min(t.scrollHeight, 10 * 22.5)}px`;
}
</script>

<template>
  <div class="composer">
    <p class="strip" aria-live="polite">
      <template v-if="status">
        <span class="dot" :class="status.tone" aria-hidden="true" />
        <span>{{ status.text }}</span>
        <span class="caption tnum">{{ status.extra }}</span>
      </template>
    </p>
    <label class="field">
      <span class="sr-only">Describe what to change</span>
      <textarea
        ref="area" v-model="prompt" class="input" rows="3" :disabled="active"
        placeholder="Describe what to change" @input="grow" @keydown.ctrl.enter.prevent="submit"
      />
    </label>
    <div class="controls">
      <div class="seg" role="radiogroup" aria-label="Mode">
        <label v-for="m in modes" :key="m.id" :class="{ on: mode === m.id }">
          <input v-model="mode" type="radio" name="mode" :value="m.id" :disabled="active || (changesBlocked && m.id === 'changes')" />
          {{ m.label }}
        </label>
      </div>
      <label class="profile">
        <span class="sr-only">Agent profile</span>
        <select v-model="profileId" class="input" :disabled="active || !profiles.length">
          <option :value="null">{{ defaultName }}</option>
          <option v-for="p in profiles" :key="p.id" :value="p.id">{{ p.name }}</option>
        </select>
      </label>
      <span class="spacer" />
      <button v-if="active" type="button" class="btn" @click="emit('cancel')">Cancel</button>
      <button v-else type="button" class="btn primary" :disabled="(!noProfile && !prompt.trim()) || (!!changesBlocked && mode === 'changes')" @click="submit">
        {{ noProfile ? "Set up Claude first" : "Run" }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.composer { background: var(--card); border-top: 1px solid var(--border); padding: 8px 0 0; }
.strip { display: flex; gap: 8px; align-items: center; min-height: 24px; margin: 0 0 4px; font-size: 13px; }
.dot { width: 8px; height: 8px; border-radius: 50%; }
.dot.accent { background: var(--accent); }
.dot.warn { background: var(--warn); }
.dot.ok { background: var(--ok); }
.caption { color: var(--muted); font-size: 13px; }
.field { display: block; }
textarea.input { resize: none; min-height: 3lh; }
.controls { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-top: 8px; }
.seg { display: inline-flex; border: 1px solid var(--border-strong); border-radius: 8px; overflow: hidden; }
.seg label { position: relative; padding: 6px 12px; cursor: pointer; transition: background 150ms cubic-bezier(0.2, 0, 0, 1); }
.seg label.on { background: var(--accent-strong); color: var(--on-accent); }
.seg input { position: absolute; opacity: 0; inset: 0; margin: 0; cursor: pointer; }
.seg label:focus-within { outline: 2px solid var(--accent); outline-offset: 2px; }
.profile select { width: auto; }
.spacer { flex: 1; }
</style>
