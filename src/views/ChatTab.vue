<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { commands } from "../bindings";
import type { AgentProfile, ProjectSummary, SessionMode } from "../bindings";
import { call } from "../lib/call";
import { approvalDetail, buildEnd, buildRows, hasFallbackWarning, loadMode, saveMode } from "../lib/chat";
import { wantsTo } from "../lib/notify";
import { STATE_LABELS } from "../lib/runs";
import { useNavStore } from "../stores/nav";
import { isActive, useRunsStore } from "../stores/runs";
import { commitMessage } from "../lib/preview";
import Composer from "../components/chat/Composer.vue";
import Timeline from "../components/chat/Timeline.vue";

const props = defineProps<{ project: ProjectSummary }>();
const emit = defineEmits<{ "review-changes": [] }>();

const runs = useRunsStore();
const nav = useNavStore();

// --- profiles (D52) ---
const profiles = ref<AgentProfile[]>([]);
const profilesLoaded = ref(false);
const noProfile = computed(() => profilesLoaded.value && profiles.value.length === 0);
onMounted(async () => {
  if (!session.value) composer.value?.focus(); // empty state: composer focused (D48)
  const r = await call(commands.listAgentProfiles());
  if (r.ok) profiles.value = r.data.filter((p) => p.enabled);
  profilesLoaded.value = true;
});

// --- composer state ---
const prompt = ref("");
const mode = ref<SessionMode>(loadMode(props.project.id) ?? "plan");
const profileId = ref<string | null>(null);
watch(() => props.project.id, (id) => (mode.value = loadMode(id) ?? "plan"));
watch(mode, (m) => saveMode(props.project.id, m));
const composer = ref<InstanceType<typeof Composer> | null>(null);

// --- selected run ---
const list = computed(() => runs.sessions[props.project.id] ?? []);
const session = computed(() => runs.selectedSession(props.project.id));
const activeRun = computed(() => list.value.find(isActive) ?? null);
watch(() => session.value?.id, (id) => id && void runs.loadEvents(id), { immediate: true });

const events = computed(() => (session.value ? runs.eventsBySession[session.value.id] ?? [] : []));
const rows = computed(() => buildRows(events.value));
const sessionActive = computed(() => !!session.value && isActive(session.value));
const idleSecs = computed(() => {
  const id = session.value?.profileId;
  return (profiles.value.find((p) => p.id === id) ?? profiles.value.find((p) => p.isDefault))?.inactivitySecs ?? null;
});
const end = computed(() => (session.value ? buildEnd(session.value, events.value, idleSecs.value) : null));
const fallback = computed(() => hasFallbackWarning(events.value));
const planActions = computed(
  () => !!session.value && session.value.mode === "plan" && !sessionActive.value && session.value.canResume
    && !list.value.some((s) => s.resumedFrom === session.value!.id),
);
const canRetry = computed(() => !activeRun.value && !!session.value?.canResume);

// --- status strip: state, elapsed ---
const now = ref(Date.now());
let tick: ReturnType<typeof setInterval> | undefined;
onMounted(() => (tick = setInterval(() => (now.value = Date.now()), 1000)));
onUnmounted(() => clearInterval(tick));

function elapsed(from: string | null, to: number) {
  if (!from) return "";
  const secs = Math.max(0, Math.floor((to - new Date(from).getTime()) / 1000));
  return secs >= 60 ? `${Math.floor(secs / 60)}m ${secs % 60}s` : `${secs}s`;
}
const status = computed(() => {
  const s = session.value;
  if (!s) return null;
  const waiting = s.jobState === "waitingForApproval";
  if (waiting) return { tone: "warn" as const, text: "Waiting for your approval", extra: "Inactivity timer is paused" };
  const time = elapsed(s.startedAt ?? s.createdAt, s.completedAt ? new Date(s.completedAt).getTime() : now.value);
  const label = STATE_LABELS[s.jobState];
  if (sessionActive.value) return { tone: "accent" as const, text: label, extra: time };
  return { tone: s.jobState === "succeeded" ? ("ok" as const) : ("warn" as const), text: label, extra: time };
});

// --- approvals: assertive announcement (D59) and Alt+A ---
const pending = computed(() => {
  const row = rows.value.find((r) => r.kind === "approval" && r.approval.answered === null);
  return sessionActive.value && row?.kind === "approval" ? row.approval : null;
});
const announcement = ref("");
watch(
  () => pending.value?.requestId,
  (id) => {
    const a = pending.value;
    announcement.value = id && a ? `Claude wants to ${wantsTo(a.tool)}: ${approvalDetail(a.tool, a.input).split("\n")[0]}` : "";
  },
  { immediate: true },
);
function onKey(e: KeyboardEvent) {
  if (e.altKey && e.key.toLowerCase() === "a") {
    const card = document.querySelector<HTMLElement>("[data-approval]");
    if (card) {
      e.preventDefault();
      card.focus();
    }
  }
}
onMounted(() => window.addEventListener("keydown", onKey));
onUnmounted(() => window.removeEventListener("keydown", onKey));

// --- keep the newest row in view while events stream in ---
const scroller = ref<HTMLElement | null>(null);
watch(() => rows.value.length, async () => {
  const el = scroller.value;
  if (!el || el.scrollHeight - el.scrollTop - el.clientHeight > 120) return;
  await nextTick();
  el.scrollTop = el.scrollHeight;
});

// --- actions ---
async function run() {
  saveMode(props.project.id, mode.value);
  const r = await runs.start(props.project.id, prompt.value.trim(), mode.value, profileId.value);
  if (r.ok) prompt.value = "";
}
const cancel = () => activeRun.value && runs.cancel(activeRun.value.jobId);
const answer = (requestId: string, choice: Parameters<typeof runs.answer>[2]) =>
  session.value && runs.answer(session.value.jobId, requestId, choice);
const make = () => session.value && runs.continueRun(props.project.id, session.value.id);
const retry = (rule: string) => session.value && runs.allowAndRetry(props.project.id, session.value.id, rule);
// "Preview" and "Commit…" on a successful Make-changes end row (M3-D42).
const canCommit = computed(() => !!session.value && session.value.mode === "changes" && session.value.jobState === "succeeded" && end.value?.action === "review");
async function commit() {
  const s = session.value;
  if (!s) return;
  const r = await call(commands.runDiff(s.id));
  if (!r.ok) {
    runs.error = r.error;
    return;
  }
  const paths = r.data.filter((f) => !f.skippedReason).map((f) => f.path);
  nav.commitFiles({ projectId: props.project.id, paths, message: commitMessage(s.prompt) });
}

// "Ask Claude to fix" (M3-D43): fill the composer, run nothing.
watch(
  () => nav.composerHandoff,
  async (h) => {
    if (!h || h.projectId !== props.project.id) return;
    prompt.value = h.text;
    mode.value = "changes";
    nav.composerHandoff = null;
    await nextTick();
    composer.value?.focus();
  },
  { immediate: true },
);
function revise() {
  mode.value = "plan";
  composer.value?.focus();
}
</script>

<template>
  <div class="chat">
    <div ref="scroller" class="scroll">
      <p v-if="runs.error" class="callout danger" role="alert">{{ runs.error.message }}</p>
      <div v-if="!session" class="empty">
        <p>Describe the change you want, then press Run.</p>
        <p class="muted">Plan first: Claude proposes, no files change. Make changes: Claude edits your project.</p>
      </div>
      <template v-else>
        <p v-if="fallback" class="callout warn">Approvals are off for this Claude version; edits will be listed for you to allow.</p>
        <Timeline
          :prompt="session.prompt" :rows="rows" :cwd="project.workspacePath" :active="sessionActive" :end="end"
          :plan-actions="planActions" :can-retry="canRetry" :can-commit="canCommit" :checkpoint-ref="session.checkpointRef"
          @allow="(id) => answer(id, { kind: 'allow' })"
          @deny="(id, message) => answer(id, message ? { kind: 'deny', message } : { kind: 'deny' })"
          @make="make" @revise="revise" @review="emit('review-changes')" @preview="nav.tab = 'preview'" @commit="commit" @settings="nav.openAgentSettings()" @retry="retry"
        />
      </template>
    </div>
    <div class="sr-only" aria-live="assertive" role="alert">{{ announcement }}</div>
    <Composer
      ref="composer" v-model:prompt="prompt" v-model:mode="mode" v-model:profile-id="profileId"
      :profiles="profiles" :no-profile="noProfile" :active="!!activeRun" :status="status"
      @run="run" @cancel="cancel" @setup="nav.openAgentSettings()"
    />
  </div>
</template>

<style scoped>
.chat { display: flex; flex-direction: column; flex: 1; min-height: 0; }
.scroll { flex: 1; min-height: 0; overflow-y: auto; padding-bottom: var(--space); }
.empty { max-width: 860px; }
.empty p { margin: 0 0 4px; }
</style>
