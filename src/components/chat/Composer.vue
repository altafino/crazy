<script setup lang="ts">
import { computed, ref } from "vue";
import { Paperclip } from "lucide-vue-next";
import { commands } from "../../bindings";
import type { AgentProfile, SessionMode } from "../../bindings";
import { runBlockReason } from "../../lib/attachments";
import type { Chip } from "../../lib/attachments";
import AttachmentChips from "./AttachmentChips.vue";

const props = defineProps<{
  profiles: AgentProfile[];
  noProfile: boolean;
  active: boolean;
  /** A build is live: Make changes is off (M4-DR23). */
  changesBlocked?: boolean;
  status: { tone: "accent" | "warn" | "ok"; text: string; extra: string } | null;
  attachments: Chip[];
  /** Attachment announcements, read by the status strip's live region. */
  announce: string;
}>();
const prompt = defineModel<string>("prompt", { required: true });
const mode = defineModel<SessionMode>("mode", { required: true });
const profileId = defineModel<string | null>("profileId", { required: true });
/** Allow commands and file edits without a card (Make changes only; Settings > Approvals). */
const autoApprove = defineModel<boolean | null>("autoApprove", { default: false });
const emit = defineEmits<{ run: []; cancel: []; setup: []; attach: []; paste: [files: File[]]; pastePaths: [paths: string[]]; removeChip: [key: string] }>();

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

/** Why Run is off (DR-D4); an error wins over a conversion still running. */
const blockReason = computed(() => runBlockReason(props.attachments));
async function onPaste(e: ClipboardEvent) {
  const dt = e.clipboardData;
  // Files copied in a file manager arrive as text/uri-list, which WebKitGTK hides from the event:
  // the backend reads the paths from GTK. Checked first, so a copied image keeps its own name.
  // No files after all (a copied link): paste the text as usual.
  if (dt?.types?.includes("text/uri-list")) {
    e.preventDefault();
    const text = dt.getData("text/plain");
    const paths = await commands.clipboardFilePaths();
    if (paths.length) emit("pastePaths", paths);
    else if (text) document.execCommand("insertText", false, text);
    return;
  }
  const files = [...(dt?.files ?? [])];
  const items = [...(dt?.items ?? [])].filter((i) => i.kind === "file").map((i) => i.getAsFile()).filter((f): f is File => !!f);
  const images = (files.length ? files : items).filter((f) => f.type.startsWith("image/"));
  if (images.length) {
    e.preventDefault();
    emit("paste", images);
    return;
  }
  // WebKitGTK (2.52) hands an image paste an empty clipboardData; the async Clipboard API still
  // has it. Only when the event carries nothing, so text pastes are never touched.
  if (dt?.types?.length || !navigator.clipboard?.read) return;
  try {
    for (const item of await navigator.clipboard.read()) {
      const type = item.types.find((t) => t.startsWith("image/"));
      if (type) images.push(new File([await item.getType(type)], "", { type }));
    }
  } catch {
    return; // no permission or nothing readable: same as an empty paste
  }
  if (images.length) emit("paste", images);
}
function submit() {
  if (props.active || blockReason.value || (props.changesBlocked && mode.value === "changes")) return;
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
      <span class="sr-only">{{ announce }}</span>
    </p>
    <AttachmentChips :chips="attachments" removable @remove="(k) => emit('removeChip', k)" @focus-input="area?.focus()" />
    <label class="field">
      <span class="sr-only">Describe what to change</span>
      <textarea
        ref="area" v-model="prompt" class="input" rows="3" :disabled="active"
        placeholder="Describe what to change" @input="grow" @keydown.ctrl.enter.prevent="submit" @paste="onPaste"
      />
    </label>
    <div class="controls">
      <button type="button" class="btn attach" aria-label="Attach files" title="Attach files" :disabled="active" @click="emit('attach')">
        <Paperclip :size="16" :stroke-width="1.75" aria-hidden="true" /><span class="lbl">Attach files</span>
      </button>
      <div class="seg" role="radiogroup" aria-label="Mode">
        <label v-for="m in modes" :key="m.id" :class="{ on: mode === m.id }">
          <input v-model="mode" type="radio" name="mode" :value="m.id" :disabled="active || (changesBlocked && m.id === 'changes')" />
          {{ m.label }}
        </label>
      </div>
      <label class="auto" :title="mode === 'plan' ? 'Plan first always asks before anything runs' : 'Allow commands and file edits without asking. Risky ones still ask unless turned off in Settings > Approvals.'">
        <input
          type="checkbox" :checked="!!autoApprove && mode === 'changes'" :disabled="active || mode === 'plan'" aria-describedby="auto-hint"
          @change="autoApprove = ($event.target as HTMLInputElement).checked"
        />
        Auto-approve
        <span id="auto-hint" class="sr-only">{{ mode === "plan" ? "Off in Plan first: plans always ask." : "Allows commands and file edits without asking; risky ones still ask unless turned off in Settings, Approvals." }}</span>
      </label>
      <label class="profile">
        <span class="sr-only">Agent profile</span>
        <select v-model="profileId" class="input" :disabled="active || !profiles.length">
          <option :value="null">{{ defaultName }}</option>
          <option v-for="p in profiles" :key="p.id" :value="p.id">{{ p.name }}</option>
        </select>
      </label>
      <span class="spacer" />
      <p v-if="blockReason" id="run-reason" class="reason caption">{{ blockReason }}</p>
      <button v-if="active" type="button" class="btn" @click="emit('cancel')">Cancel</button>
      <button v-else type="button" class="btn primary" :disabled="(!noProfile && !prompt.trim()) || (!!changesBlocked && mode === 'changes') || !!blockReason" :aria-describedby="blockReason ? 'run-reason' : undefined" @click="submit">
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
.auto { display: inline-flex; align-items: center; gap: 6px; cursor: pointer; }
.auto:has(input:disabled) { color: var(--muted); cursor: default; }
.spacer { flex: 1; }
.attach { display: inline-flex; align-items: center; gap: 6px; }
.reason { margin: 0; }
/* DR-D7: below 900px Attach is a 32px icon button and the reason gets its own line above the row. */
@media (max-width: 899px) {
  .attach { width: 32px; height: 32px; padding: 0; justify-content: center; }
  .lbl { display: none; }
  .reason { order: -1; flex-basis: 100%; }
}
</style>
