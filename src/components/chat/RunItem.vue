<script setup lang="ts">
import { computed, onMounted, onUnmounted, onUpdated, ref, watch } from "vue";
import { commands } from "../../bindings";
import type { AgentProfile, ProjectSummary, RunAttachment, SessionSummary } from "../../bindings";
import { call } from "../../lib/call";
import { chipOfRun } from "../../lib/attachments";
import { buildEnd, buildRows, hasFallbackWarning } from "../../lib/chat";
import { taskOfPrompt } from "../../lib/plan";
import { commitMessage } from "../../lib/preview";
import { STATE_LABELS, runClock } from "../../lib/runs";
import { useBuildsStore } from "../../stores/builds";
import { useNavStore } from "../../stores/nav";
import { usePlanStore } from "../../stores/plan";
import { usePreviewStore } from "../../stores/preview";
import { isActive, useRunsStore } from "../../stores/runs";
import { useToastStore } from "../../stores/toast";
import ConfirmDialog from "../ConfirmDialog.vue";
import Timeline from "./Timeline.vue";
import type { FromPlan } from "./RunEnd.vue";

/**
 * One run of the thread (D2): its prompt, rows and end row, with the state that belongs to this
 * run alone. The newest run loads its events at once; older runs show a placeholder and load them
 * when they come within one viewport of the thread (R5).
 */
const props = defineProps<{
  session: SessionSummary;
  project: ProjectSummary;
  profiles: AgentProfile[];
  /** The newest run: loads at once. */
  newest: boolean;
  /** Its controls are in the Tab order (D13): the newest run, or the run focus is in. */
  tabbable: boolean;
}>();
const emit = defineEmits<{ "review-changes": []; revise: []; make: [sessionId: string]; focus: [sessionId: string] }>();

const runs = useRunsStore();
const nav = useNavStore();
const planStore = usePlanStore();
const toast = useToastStore();
const preview = usePreviewStore();
const builds = useBuildsStore();

const s = computed(() => props.session);
const pid = computed(() => props.project.id);
const list = computed(() => runs.sessions[pid.value] ?? []);
const activeRun = computed(() => list.value.find(isActive) ?? null);
const sessionActive = computed(() => isActive(s.value));

// --- events and attachments: loaded once per run (R5) ---
const events = computed(() => runs.eventsBySession[s.value.id]);
const loaded = computed(() => !!events.value);
const loadState = ref<"idle" | "loading" | "error">("idle");
const sent = ref<RunAttachment[]>([]);
let requested = false;
async function loadSent() {
  const id = s.value.id;
  const r = await call(commands.listRunAttachments([id]));
  if (r.ok && s.value.id === id) sent.value = r.data;
}
async function load() {
  requested = true;
  loadState.value = "loading";
  const [r] = await Promise.all([runs.loadEvents(s.value.id), loadSent()]);
  loadState.value = r.ok ? "idle" : "error";
}
watch(() => props.newest || sessionActive.value, (eager) => eager && void load(), { immediate: true });

const article = ref<HTMLElement | null>(null);
let observer: IntersectionObserver | undefined;
onMounted(() => {
  if (props.newest || sessionActive.value) return;
  if (loaded.value) return void loadSent(); // events are cached from an earlier visit
  const root = article.value?.closest<HTMLElement>("[data-thread-scroll]") ?? null;
  if (typeof IntersectionObserver === "undefined") return void load();
  observer = new IntersectionObserver(
    (entries) => {
      if (!entries.some((e) => e.isIntersecting)) return;
      observer?.disconnect();
      if (!requested) void load();
    },
    { root, rootMargin: `${root?.clientHeight ?? 0}px 0px` },
  );
  observer.observe(article.value!);
});
onUnmounted(() => observer?.disconnect());

// --- roving tabindex (D13): older runs' controls leave the Tab order until focus enters them ---
function rove() {
  article.value?.querySelectorAll<HTMLElement>("button, summary, a[href], input, textarea, select").forEach((el) => {
    if (props.tabbable) el.removeAttribute("tabindex");
    else el.tabIndex = -1;
  });
}
onMounted(rove);
onUpdated(rove);
watch(() => props.tabbable, rove, { flush: "post" });

// --- what the run shows ---
const rows = computed(() => (events.value ? buildRows(events.value) : []));
const sentChips = computed(() => sent.value.map(chipOfRun));
const idleSecs = computed(() => {
  const id = s.value.profileId;
  return (props.profiles.find((p) => p.id === id) ?? props.profiles.find((p) => p.isDefault))?.inactivitySecs ?? null;
});
const end = computed(() => (events.value ? buildEnd(s.value, events.value, idleSecs.value) : null));
const fallback = computed(() => !!events.value && hasFallbackWarning(events.value));
const planActions = computed(
  () => s.value.mode === "plan" && !sessionActive.value && s.value.canResume && !list.value.some((x) => x.resumedFrom === s.value.id),
);
/** D6/D14: the caption under a run's prompt. */
const caption = computed(() => {
  const x = s.value;
  if (x.resumeFellBack) return "New conversation: the previous one couldn't be continued";
  if (x.resumedFrom) return "Continues the conversation";
  if (x.conversationStart === "profile") return "New conversation: different agent profile";
  const task = x.mode === "changes" ? taskOfPrompt(x.prompt) : null;
  return task && x.conversationStart === "new" ? `New conversation · From the plan: ${task}` : "New conversation";
});
const canRetry = computed(() => !activeRun.value && s.value.canResume);
const name = computed(() => `${s.value.prompt.trim().split("\n")[0]}, ${STATE_LABELS[s.value.jobState].toLowerCase()}`);
const placeholder = computed(() => `${STATE_LABELS[s.value.jobState]} · ${runClock(s.value)}`);

// --- project plan (F15): Use as project plan, and the plan task a Start run worked on ---
const planText = computed(() => [...rows.value].reverse().find((r) => r.kind === "plan")?.plan ?? null);
const planTaskCount = ref(0);
watch(planText, async (t) => {
  planTaskCount.value = 0;
  if (t === null) return;
  try {
    const n = (await commands.parsePlan(t)).tasks.length;
    if (planText.value === t) planTaskCount.value = n;
  } catch {
    // No count: no Use button.
  }
}, { immediate: true });
const canUsePlan = computed(() => planTaskCount.value > 0 && !activeRun.value);
const planSaved = ref(false);
const confirmReplace = ref(false);
const replaceCounts = computed(() => planStore.progress(pid.value));
function usePlan() {
  if (replaceCounts.value) confirmReplace.value = true;
  else void savePlan();
}
async function savePlan() {
  confirmReplace.value = false;
  const text = planText.value;
  if (text === null) return;
  const r = await planStore.save(pid.value, text);
  if (!r.ok) return toast.show(r.error.message);
  planSaved.value = true;
  const n = r.data.tasks.length;
  toast.show(`Saved as the project plan (${n} ${n === 1 ? "task" : "tasks"})`, { label: "Open Plan", run: () => (nav.tab = "plan") });
}

const marked = ref(false);
const fromPlan = computed<FromPlan | null>(() => {
  const x = s.value;
  const title = x.mode === "changes" && end.value ? taskOfPrompt(x.prompt) : null;
  if (!title) return null;
  const progress = planStore.progress(pid.value);
  if (marked.value) return { title, state: "marked", progress, next: planStore.nextTask(pid.value)?.text ?? null };
  if (x.jobState !== "succeeded") return { title, state: "notDone" };
  const task = planStore.plans[pid.value]?.tasks.find((t) => t.text === title);
  if (!task) return { title, state: "unknown" };
  return { title, state: task.done ? "done" : "offer" };
});
async function markDone() {
  const title = taskOfPrompt(s.value.prompt);
  if (!title) return;
  const r = await planStore.setTask(pid.value, title, true);
  if (r.ok) marked.value = true;
  else toast.show(r.error.message);
}
function startNext() {
  const t = planStore.nextTask(pid.value);
  if (t) void planStore.startTask(pid.value, t);
}

// --- actions on this run ---
const answer = (requestId: string, choice: Parameters<typeof runs.answer>[2]) => runs.answer(s.value.jobId, requestId, choice);
const retry = (rule: string) => runs.allowAndRetry(pid.value, s.value.id, rule);
// "Start preview" and "Commit…" on a successful Make-changes end row (M3-D42, D7).
const canCommit = computed(() => s.value.mode === "changes" && s.value.jobState === "succeeded" && end.value?.action === "review");
// Imported folders have no preview; while a build is live the Preview tab can't start one either.
const previewMode = computed(() => {
  if (!props.project.supportsOperations) return null;
  if (preview.status[pid.value]?.live) return "running";
  return builds.isBuilding(pid.value) ? null : "start";
});
const previewBusy = ref(false);
async function startPreview() {
  if (previewBusy.value) return;
  previewBusy.value = true;
  await preview.start(pid.value);
  previewBusy.value = false;
  nav.tab = "preview";
}
async function commit() {
  const r = await call(commands.runDiff(s.value.id));
  if (!r.ok) {
    runs.error = r.error;
    return;
  }
  const paths = r.data.filter((f) => !f.skippedReason).map((f) => f.path);
  nav.commitFiles({ projectId: pid.value, paths, message: commitMessage(s.value.prompt) });
}
</script>

<template>
  <article ref="article" class="run" :aria-labelledby="`run-${session.id}-name`" @focusin="emit('focus', session.id)">
    <span :id="`run-${session.id}-name`" class="sr-only">{{ name }}</span>
    <p v-if="fallback" class="callout warn">Approvals are off for this Claude version; edits will be listed for you to allow.</p>
    <Timeline
      :prompt="session.prompt" :prompt-id="`run-${session.id}-prompt`" :caption="caption" :rows="rows" :cwd="project.workspacePath" :active="sessionActive" :end="end"
      :attachments="sentChips" :plan-actions="planActions" :can-retry="canRetry" :can-commit="canCommit" :checkpoint-ref="session.checkpointRef"
      :can-use-plan="canUsePlan" :plan-saved="planSaved" :from-plan="fromPlan" :preview="canCommit ? previewMode : null"
      @use-plan="usePlan" @mark-done="markDone" @start-next="startNext"
      @allow="(id) => answer(id, { kind: 'allow' })"
      @deny="(id, message) => answer(id, message ? { kind: 'deny', message } : { kind: 'deny' })"
      @make="emit('make', session.id)" @revise="emit('revise')" @review="emit('review-changes')" @preview="startPreview" @commit="commit" @build="nav.openBuildDialog(project.id)" @settings="nav.openAgentSettings()" @retry="retry"
    >
      <template v-if="!loaded" #status>
        <template v-if="loadState === 'error'">
          <span class="muted">Couldn't load this run</span>
          <button type="button" class="btn small retry" @click="load">Retry</button>
        </template>
        <span v-else-if="loadState === 'loading'" class="muted">Loading…<span class="spin" aria-hidden="true" /></span>
        <span v-else class="muted">{{ placeholder }}</span>
      </template>
    </Timeline>
    <ConfirmDialog v-if="confirmReplace && replaceCounts" title="Replace the project plan?" confirm-label="Replace plan" @confirm="savePlan" @cancel="confirmReplace = false">
      <p>The current plan has {{ replaceCounts.done }} of {{ replaceCounts.total }} tasks done. Its ticks are replaced by this plan.</p>
    </ConfirmDialog>
  </article>
</template>

<style scoped>
.run { max-width: 860px; }
.retry { margin-left: 8px; }
/* The spinner only shows when loading takes longer than 400 ms. */
.spin { display: inline-block; width: 12px; height: 12px; margin-left: 8px; vertical-align: -2px; border: 2px solid var(--accent); border-top-color: transparent; border-radius: 50%; opacity: 0; animation: show 0s 400ms forwards, turn 1s linear infinite; }
@keyframes show { to { opacity: 1; } }
@keyframes turn { to { transform: rotate(360deg); } }
</style>
