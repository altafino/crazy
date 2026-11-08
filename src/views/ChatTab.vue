<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { commands } from "../bindings";
import type { AgentProfile, ProjectSummary, SessionMode } from "../bindings";
import { call } from "../lib/call";
import { useDraftAttachments } from "../lib/draftAttachments";
import { approvalDetail, buildRows, loadMode, saveMode } from "../lib/chat";
import { wantsTo } from "../lib/notify";
import { STATE_LABELS, runDay } from "../lib/runs";
import { useBuildsStore } from "../stores/builds";
import { useCraftsStore } from "../stores/crafts";
import { useNavStore } from "../stores/nav";
import { usePlanStore } from "../stores/plan";
import { isActive, useRunsStore } from "../stores/runs";
import Composer from "../components/chat/Composer.vue";
import RunItem from "../components/chat/RunItem.vue";
import { TriangleAlert } from "lucide-vue-next";

const props = defineProps<{ project: ProjectSummary }>();
const emit = defineEmits<{ "review-changes": [] }>();

const runs = useRunsStore();
const nav = useNavStore();
const builds = useBuildsStore();
const planStore = usePlanStore();

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

// --- the thread: the project's runs, oldest at the top (D2) ---
const PAGE = 20;
const shown = ref(PAGE);
const list = computed(() => runs.sessions[props.project.id] ?? []);
/** The newest run: the status strip, approvals and the composer follow it. */
const session = computed(() => list.value[0] ?? null);
const visible = computed(() => list.value.slice(0, shown.value).reverse());
const hasEarlier = computed(() => list.value.length > shown.value);
const activeRun = computed(() => list.value.find(isActive) ?? null);
const sessionsLoaded = computed(() => props.project.id in runs.sessions);
/** Day divider text before a run, when it begins a new day. */
const dayBefore = (i: number) => {
  const d = runDay(visible.value[i]!);
  return i > 0 && runDay(visible.value[i - 1]!) === d ? null : d;
};
// Focus in an older run puts its controls in the Tab order; the previous one leaves it (D13).
const focused = ref<string | null>(null);
const sessionActive = computed(() => !!session.value && isActive(session.value));
const events = computed(() => (session.value ? runs.eventsBySession[session.value.id] ?? [] : []));
const rows = computed(() => buildRows(events.value));
async function showEarlier() {
  const el = scroller.value;
  const fromBottom = el ? el.scrollHeight - el.scrollTop : 0;
  shown.value += PAGE;
  await nextTick();
  if (el) el.scrollTop = el.scrollHeight - fromBottom; // keep the run you were reading in place
}
const continues = computed(() => list.value.some((s) => s.canResume));
// --- project plan (F15): a run ending may change the plan file ---
onMounted(() => void planStore.load(props.project.id));
watch(() => props.project.id, (id) => void planStore.load(id));
// A run ending may have changed the plan file (Claude edits it), so read it again.
watch(() => session.value && !sessionActive.value && session.value.id, (id) => id && void planStore.load(props.project.id));

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

// --- scrolling: follow new rows only while near the bottom; otherwise offer the jump pill (D5) ---
const scroller = ref<HTMLElement | null>(null);
const NEAR = 120;
const atBottom = ref(true);
/** New rows arrived while scrolled up. */
const unseen = ref(false);
const toBottom = () => scroller.value && (scroller.value.scrollTop = scroller.value.scrollHeight);
function onScroll() {
  const el = scroller.value!;
  atBottom.value = el.scrollHeight - el.scrollTop - el.clientHeight <= NEAR;
  if (atBottom.value) unseen.value = false;
}
function jump() {
  toBottom();
  atBottom.value = true;
  unseen.value = false;
}
watch(() => rows.value.length, async () => {
  if (!atBottom.value) {
    unseen.value = true;
    return;
  }
  await nextTick();
  toBottom();
});
onMounted(async () => {
  await nextTick();
  toBottom(); // open at the newest run
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
const make = (sessionId: string) => starting(true, () => runs.continueRun(props.project.id, sessionId, autoApprove.value));

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
    <div class="thread-wrap">
      <div ref="scroller" class="scroll" data-thread-scroll @scroll="onScroll">
        <div v-if="runs.error && !sessionsLoaded" class="callout danger" role="alert">
          <p>Couldn't load this project's runs <button type="button" class="btn small" @click="runs.loadSessions(project.id)">Retry</button></p>
          <details><summary>Details</summary>{{ runs.error.message }}</details>
        </div>
        <p v-else-if="runs.error" class="callout danger" role="alert">{{ runs.error.message }}</p>
        <p v-if="!sessionsLoaded && !runs.error" class="muted">Reading runs…</p>
        <div v-else-if="!session && sessionsLoaded" class="empty">
          <p>Describe the change you want, then press Run.</p>
          <p class="muted">Plan first: Claude proposes, no files change. Make changes: Claude edits your project.</p>
        </div>
        <div v-else class="thread" role="log" aria-label="Run timeline">
          <button v-if="hasEarlier" type="button" class="btn small earlier" @click="showEarlier">Show earlier runs</button>
          <template v-for="(s, i) in visible" :key="s.id">
            <div v-if="dayBefore(i)" class="day muted" role="separator" :aria-label="dayBefore(i)!"><span>{{ dayBefore(i) }}</span></div>
            <RunItem
              :session="s" :project="project" :profiles="profiles" :newest="s.id === session!.id" :tabbable="s.id === session!.id || s.id === focused"
              @focus="focused = $event" @make="make" @revise="revise" @review-changes="emit('review-changes')"
            />
          </template>
        </div>
      </div>
      <button v-if="unseen && !atBottom" type="button" class="jump" :class="{ warn: !!pending }" @click="jump">
        <TriangleAlert v-if="pending" :size="14" :stroke-width="1.75" aria-hidden="true" />
        {{ pending ? "Waiting for your approval" : "New activity" }} · Jump to latest
      </button>
    </div>
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
.thread-wrap { position: relative; flex: 1; min-height: 0; display: flex; flex-direction: column; }
.scroll { flex: 1; min-height: 0; overflow-y: auto; padding-bottom: var(--space); }
.thread { max-width: 860px; }
.earlier { margin-bottom: 8px; }
.day { display: flex; align-items: center; gap: 12px; margin: 12px 0 4px; font-size: 13px; }
.day::before, .day::after { content: ""; flex: 1; height: 1px; background: var(--border); }
.jump { position: absolute; left: 50%; bottom: 8px; transform: translateX(-50%); display: flex; align-items: center; gap: 6px; font: inherit; font-size: 13px; background: var(--card); color: var(--text); border: 1px solid var(--border); border-radius: 999px; padding: 4px 12px; cursor: pointer; box-shadow: 0 1px 4px rgb(0 0 0 / 0.12); }
.jump.warn { background: var(--warn-bg); border-color: var(--warn); color: var(--warn); }
.empty { max-width: 860px; }
.empty p { margin: 0 0 4px; }
</style>
