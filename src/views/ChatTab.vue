<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { commands } from "../bindings";
import type { AgentProfile, ProjectSummary, RunAttachment, SessionMode } from "../bindings";
import { call } from "../lib/call";
import { chipOfRun } from "../lib/attachments";
import { useDraftAttachments } from "../lib/draftAttachments";
import { approvalDetail, buildEnd, buildRows, hasFallbackWarning, loadMode, saveMode } from "../lib/chat";
import { wantsTo } from "../lib/notify";
import { STATE_LABELS } from "../lib/runs";
import { useBuildsStore } from "../stores/builds";
import { useCraftsStore } from "../stores/crafts";
import { useNavStore } from "../stores/nav";
import { usePlanStore } from "../stores/plan";
import { useToastStore } from "../stores/toast";
import { taskOfPrompt } from "../lib/plan";
import { isActive, useRunsStore } from "../stores/runs";
import { commitMessage } from "../lib/preview";
import Composer from "../components/chat/Composer.vue";
import Timeline from "../components/chat/Timeline.vue";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import type { FromPlan } from "../components/chat/RunEnd.vue";

const props = defineProps<{ project: ProjectSummary }>();
const emit = defineEmits<{ "review-changes": [] }>();

const runs = useRunsStore();
const nav = useNavStore();
const builds = useBuildsStore();
const planStore = usePlanStore();
const toast = useToastStore();

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
const mode = ref<SessionMode>(loadMode(props.project.id) ?? "plan");
const profileId = ref<string | null>(null);
// D6: set by the composer's New conversation link, cleared by the next Run.
const newConversation = ref(false);
watch(() => props.project.id, (id) => (mode.value = loadMode(id) ?? "plan"));
watch(mode, (m) => saveMode(props.project.id, m));
const composer = ref<InstanceType<typeof Composer> | null>(null);
const draft = useDraftAttachments(props.project.id);
const prompt = draft.prompt;
// Auto-approve: the box follows Settings > Approvals until it is changed here (then kept per project).
const autoDefault = ref(false);
onMounted(async () => {
  const r = await call(commands.getApprovalSettings());
  autoDefault.value = r.ok && r.data.autoApprove;
});
const autoApprove = computed({
  get: () => draft.autoApprove.value ?? autoDefault.value,
  set: (v: boolean | null) => (draft.autoApprove.value = v),
});

// --- attachments: sent chips under the prompt, one call per selected run ---
const sent = ref<RunAttachment[]>([]);
async function loadSent(id: string) {
  const r = await call(commands.listRunAttachments([id]));
  if (r.ok && session.value?.id === id) sent.value = r.data;
}

// --- selected run ---
const list = computed(() => runs.sessions[props.project.id] ?? []);
const session = computed(() => runs.selectedSession(props.project.id));
const activeRun = computed(() => list.value.find(isActive) ?? null);
watch(() => session.value?.id, (id) => {
  sent.value = [];
  if (id) {
    void runs.loadEvents(id);
    void loadSent(id);
  }
}, { immediate: true });
const sentChips = computed(() => sent.value.map(chipOfRun));

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
const continues = computed(() => list.value.some((s) => s.canResume));
/** D6/D14: the caption under a run's prompt. */
const caption = computed(() => {
  const s = session.value;
  if (!s) return undefined;
  if (s.resumeFellBack) return "New conversation: the previous one couldn't be continued";
  if (s.resumedFrom) return "Continues the conversation";
  if (s.conversationStart === "profile") return "New conversation: different agent profile";
  const task = s.mode === "changes" ? taskOfPrompt(s.prompt) : null;
  return task && s.conversationStart === "new" ? `New conversation · From the plan: ${task}` : "New conversation";
});
const canRetry = computed(() => !activeRun.value && !!session.value?.canResume);

// --- project plan (F15): Use as project plan, and the plan task a Start run worked on ---
onMounted(() => void planStore.load(props.project.id));
watch(() => props.project.id, (id) => void planStore.load(id));
// A run ending may have changed the plan file (Claude edits it), so read it again.
watch(() => session.value && !sessionActive.value && session.value.id, (id) => id && void planStore.load(props.project.id));

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
const savedSessions = ref(new Set<string>());
const planSaved = computed(() => !!session.value && savedSessions.value.has(session.value.id));
const confirmReplace = ref(false);
const replaceCounts = computed(() => planStore.progress(props.project.id));
function usePlan() {
  if (replaceCounts.value) confirmReplace.value = true;
  else void savePlan();
}
async function savePlan() {
  confirmReplace.value = false;
  const s = session.value;
  const text = planText.value;
  if (!s || text === null) return;
  const r = await planStore.save(props.project.id, text);
  if (!r.ok) return toast.show(r.error.message);
  savedSessions.value = new Set(savedSessions.value).add(s.id);
  const n = r.data.tasks.length;
  toast.show(`Saved as the project plan (${n} ${n === 1 ? "task" : "tasks"})`, { label: "Open Plan", run: () => (nav.tab = "plan") });
}

const markedSessions = ref(new Set<string>());
const fromPlan = computed<FromPlan | null>(() => {
  const s = session.value;
  const title = s && s.mode === "changes" && end.value ? taskOfPrompt(s.prompt) : null;
  if (!s || !title) return null;
  const pid = props.project.id;
  const progress = planStore.progress(pid);
  if (markedSessions.value.has(s.id)) return { title, state: "marked", progress, next: planStore.nextTask(pid)?.text ?? null };
  if (s.jobState !== "succeeded") return { title, state: "notDone" };
  const task = planStore.plans[pid]?.tasks.find((t) => t.text === title);
  if (!task) return { title, state: "unknown" };
  return { title, state: task.done ? "done" : "offer" };
});
async function markDone() {
  const s = session.value;
  const title = s && taskOfPrompt(s.prompt);
  if (!s || !title) return;
  const r = await planStore.setTask(props.project.id, title, true);
  if (r.ok) markedSessions.value = new Set(markedSessions.value).add(s.id);
  else toast.show(r.error.message);
}
function startNext() {
  const t = planStore.nextTask(props.project.id);
  if (t) void planStore.startTask(props.project.id, t);
}

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
/** A Make changes start is pending: the backend may be unpacking ArtCraft AppImages (D22). */
const preparing = ref(false);
const crafts = useCraftsStore();
async function starting<T>(changes: boolean, start: () => Promise<T>): Promise<T> {
  if (!changes) return start();
  preparing.value = true;
  try {
    return await start();
  } finally {
    preparing.value = false;
    crafts.unpacked(props.project.id);
  }
}
const status = computed(() => {
  const unpacking = preparing.value ? (crafts.firstUse[props.project.id] ?? []) : [];
  if (unpacking.length) return { tone: "accent" as const, text: `Preparing ${unpacking.join(", ")} (first use)…`, extra: "" };
  const s = session.value;
  // M4-DR23: say why Make changes is off while a build is live.
  if (builds.isBuilding(props.project.id) && !activeRun.value) {
    const v = builds.status[props.project.id]?.version;
    return { tone: "warn" as const, text: `Building${v ? ` ${v}` : ""}`, extra: "Make changes is off until the build ends" };
  }
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

// --- drop target: the Tauri drag-drop event is window-wide; this tab is only mounted while visible ---
const dragging = ref(false);
let unlistenDrop: (() => void) | undefined;
onMounted(async () => {
  try {
    unlistenDrop = await getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === "enter" || payload.type === "over") dragging.value = true;
      else dragging.value = false;
      if (payload.type === "drop" && !activeRun.value && payload.paths.length) void draft.attachPaths(payload.paths);
    });
  } catch {
    // Not running inside Tauri (browser dev): no drop support.
  }
});
onUnmounted(() => unlistenDrop?.());

// --- actions ---
async function run() {
  saveMode(props.project.id, mode.value);
  const r = await starting(mode.value === "changes", () =>
    runs.start(props.project.id, prompt.value.trim(), mode.value, profileId.value, { draftId: draft.draftId.value, ids: draft.ids.value }, autoApprove.value, newConversation.value),
  );
  if (r.ok) {
    newConversation.value = false;
    prompt.value = "";
    draft.reset();
  }
}
const cancel = () => activeRun.value && runs.cancel(activeRun.value.jobId);
const answer = (requestId: string, choice: Parameters<typeof runs.answer>[2]) =>
  session.value && runs.answer(session.value.jobId, requestId, choice);
const make = () => {
  const s = session.value;
  return s && starting(true, () => runs.continueRun(props.project.id, s.id, autoApprove.value));
};
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
    mode.value = h.mode ?? "changes";
    nav.composerHandoff = null;
    await nextTick();
    composer.value?.focus();
  },
  { immediate: true },
);
function revise() {
  mode.value = "plan";
  newConversation.value = false; // Revise plan resumes the plan session (D6)
  composer.value?.focus();
}
</script>

<template>
  <div class="chat">
    <div v-if="dragging" class="drop" aria-hidden="true">{{ activeRun ? "Attach files after this run finishes" : "Drop files to attach" }}</div>
    <div ref="scroller" class="scroll">
      <p v-if="runs.error" class="callout danger" role="alert">{{ runs.error.message }}</p>
      <div v-if="!session" class="empty">
        <p>Describe the change you want, then press Run.</p>
        <p class="muted">Plan first: Claude proposes, no files change. Make changes: Claude edits your project.</p>
      </div>
      <template v-else>
        <p v-if="fallback" class="callout warn">Approvals are off for this Claude version; edits will be listed for you to allow.</p>
        <Timeline
          :prompt="session.prompt" :caption="caption" :rows="rows" :cwd="project.workspacePath" :active="sessionActive" :end="end"
          :attachments="sentChips" :plan-actions="planActions" :can-retry="canRetry" :can-commit="canCommit" :checkpoint-ref="session.checkpointRef"
          :can-use-plan="canUsePlan" :plan-saved="planSaved" :from-plan="fromPlan"
          @use-plan="usePlan" @mark-done="markDone" @start-next="startNext"
          @allow="(id) => answer(id, { kind: 'allow' })"
          @deny="(id, message) => answer(id, message ? { kind: 'deny', message } : { kind: 'deny' })"
          @make="make" @revise="revise" @review="emit('review-changes')" @preview="nav.tab = 'preview'" @commit="commit" @build="nav.openBuildDialog(project.id)" @settings="nav.openAgentSettings()" @retry="retry"
        />
      </template>
    </div>
    <ConfirmDialog v-if="confirmReplace && replaceCounts" title="Replace the project plan?" confirm-label="Replace plan" @confirm="savePlan" @cancel="confirmReplace = false">
      <p>The current plan has {{ replaceCounts.done }} of {{ replaceCounts.total }} tasks done. Its ticks are replaced by this plan.</p>
    </ConfirmDialog>
    <div class="sr-only" aria-live="assertive" role="alert">{{ announcement }}</div>
    <Composer
      ref="composer" v-model:prompt="prompt" v-model:mode="mode" v-model:profile-id="profileId" v-model:auto-approve="autoApprove" v-model:new-conversation="newConversation" :continues="continues"
      :project-id="project.id" :project-name="project.name" :profiles="profiles" :no-profile="noProfile" :active="!!activeRun" :changes-blocked="builds.isBuilding(project.id)" :status="status"
      :attachments="draft.chips.value" :announce="draft.announce.value"
      @attach="draft.attachFiles" @paste="(fs) => fs.forEach(draft.attachPasted)" @paste-paths="draft.attachPaths" @remove-chip="draft.remove"
      @run="run" @cancel="cancel" @setup="nav.openAgentSettings()"
    />
  </div>
</template>

<style scoped>
.chat { position: relative; display: flex; flex-direction: column; flex: 1; min-height: 0; }
.drop { position: absolute; inset: 0; z-index: 5; display: grid; place-items: center; font-weight: 600; pointer-events: none; background: color-mix(in srgb, var(--info-bg) 85%, transparent); outline: 2px dashed var(--accent); outline-offset: -2px; border-radius: 10px; }
.scroll { flex: 1; min-height: 0; overflow-y: auto; padding-bottom: var(--space); }
.empty { max-width: 860px; }
.empty p { margin: 0 0 4px; }
</style>
