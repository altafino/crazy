<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import type { PlanTask, ProjectSummary } from "../bindings";
import Markdown from "../components/chat/Markdown.vue";
import { PLAN_FILE, addTasksPrompt, fixPlanPrompt, planPrompt } from "../lib/plan";
import { useNavStore } from "../stores/nav";
import { usePlanStore } from "../stores/plan";
import { useToastStore } from "../stores/toast";

const props = defineProps<{ project: ProjectSummary }>();
const nav = useNavStore();
const store = usePlanStore();
const toast = useToastStore();

const id = computed(() => props.project.id);
const plan = computed(() => store.plans[id.value]);
const readError = computed(() => store.readErrors[id.value] ?? null);
const progress = computed(() => store.progress(id.value));
const blocked = computed(() => store.blockedReason(id.value));
const running = computed(() => store.runningTask(id.value));
const nextOpen = computed(() => store.nextTask(id.value));
const allDone = computed(() => !!progress.value && progress.value.done === progress.value.total);

const loading = ref(true);
/** The spinner shows only for work over 400 ms (DESIGN.md "States"). */
const slow = ref(false);
const description = ref("");
const announce = ref("");
const saving = ref(false);
let slowTimer: ReturnType<typeof setTimeout> | undefined;
let seq = 0;

async function load() {
  const mine = ++seq;
  loading.value = plan.value === undefined;
  slow.value = false;
  clearTimeout(slowTimer);
  slowTimer = setTimeout(() => (slow.value = true), 400);
  await store.load(id.value);
  if (mine !== seq) return;
  clearTimeout(slowTimer);
  loading.value = false;
  slow.value = false;
}
onMounted(load);
onUnmounted(() => clearTimeout(slowTimer));
watch(id, load);
// A run or build ends: the plan file may have changed (Claude edits it, Chat ticks).
watch(blocked, (now, was) => {
  if (was && !now) void load();
});

const sentence = computed(() => {
  const p = progress.value;
  if (!p) return "";
  return allDone.value ? `All ${p.total} tasks done` : `${p.done} of ${p.total} tasks done`;
});
const saved = computed(() => {
  const at = plan.value?.savedAt;
  if (!at) return "";
  const d = new Date(at);
  const date = d.toLocaleDateString("en-GB", { day: "numeric", month: "short" });
  const time = d.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
  return ` · saved ${date} ${time}`;
});
// 1D: first paragraph, then the rest behind a native <details>.
const summary = computed(() => {
  const [first = "", ...rest] = (plan.value?.summary ?? "").trim().split(/\n\s*\n/);
  return { first, rest: rest.join("\n\n").trim() };
});

function makePlan() {
  const text = description.value.trim();
  if (text) nav.askClaude({ projectId: id.value, text: planPrompt(text), mode: "plan" });
}

async function tick(t: PlanTask, done: boolean) {
  saving.value = true;
  const r = await store.setTask(id.value, t.text, done);
  saving.value = false;
  if (!r.ok) {
    toast.show(r.error.message);
    return;
  }
  const p = store.progress(id.value);
  if (done && p) announce.value = `${t.text} marked done, ${p.done} of ${p.total}`;
}

async function start(t: PlanTask) {
  const r = await store.startTask(id.value, t);
  if (!r.ok) toast.show(r.error.message);
}

const off = computed(() => !!blocked.value || saving.value);
</script>

<template>
  <div class="plan">
    <p v-if="loading" class="muted" role="status">Reading the plan…<span v-if="slow" class="spin" aria-hidden="true" /></p>

    <!-- Unreadable file (2E) -->
    <div v-else-if="readError" class="callout danger" role="alert">
      <p class="first">Studio can't read {{ PLAN_FILE }}</p>
      <p class="actions">
        <button type="button" class="btn" @click="nav.askClaude({ projectId: id, text: fixPlanPrompt(readError) })">Ask Claude to fix</button>
      </p>
      <details class="details"><summary>Details</summary><pre>{{ readError }}</pre></details>
    </div>

    <!-- No plan yet (2A) -->
    <form v-else-if="!plan" class="empty" @submit.prevent="makePlan">
      <p>No plan yet. Describe your app and Claude will break it into tasks.</p>
      <label for="plan-desc">What should this app do?</label>
      <textarea id="plan-desc" v-model="description" class="input" rows="4" />
      <p><button type="submit" class="btn primary" :disabled="!description.trim()">Make a plan</button></p>
    </form>

    <!-- A plan without tasks (2E) -->
    <div v-else-if="!plan.tasks.length" class="empty">
      <p>This plan has no tasks yet.</p>
      <p><button type="button" class="btn primary" @click="nav.askClaude({ projectId: id, text: addTasksPrompt(), mode: 'plan' })">Ask Claude to add tasks</button></p>
    </div>

    <template v-else>
      <section class="status" aria-label="Plan">
        <div class="row">
          <div class="text">
            <p id="plan-sentence" class="sentence">{{ sentence }}</p>
            <p class="caption muted">From {{ PLAN_FILE }}{{ saved }}</p>
          </div>
          <button v-if="allDone" type="button" class="btn primary" @click="nav.openBuildDialog(id)">Build release…</button>
        </div>
        <progress class="bar" :class="{ done: allDone }" :value="progress!.done" :max="progress!.total" aria-labelledby="plan-sentence" />
        <p v-if="blocked" id="plan-why" class="caption muted">{{ blocked }}</p>
      </section>

      <div v-if="summary.first" class="summary">
        <Markdown :text="summary.first" />
        <details v-if="summary.rest"><summary>Show full summary</summary><Markdown :text="summary.rest" /></details>
      </div>

      <ul class="tasks" aria-label="Tasks">
        <li v-for="(t, i) in plan.tasks" :key="`${i}-${t.text}`" class="task" :class="{ done: t.done }">
          <input
            :id="`task-${i}`" type="checkbox" :checked="t.done" :disabled="off"
            :aria-labelledby="`task-t${i}`" :aria-describedby="[!t.done && t.detail ? `task-d${i}` : '', blocked ? 'plan-why' : ''].filter(Boolean).join(' ') || undefined"
            @change="tick(t, ($event.target as HTMLInputElement).checked)"
          />
          <div class="body">
            <label :id="`task-t${i}`" :for="`task-${i}`" class="title">{{ t.text }}</label>
            <p v-if="!t.done && t.detail" :id="`task-d${i}`" class="caption muted detail">{{ t.detail }}</p>
          </div>
          <template v-if="!t.done">
            <span v-if="running === t.text" class="badge info"><span class="spin" aria-hidden="true" />In progress</span>
            <button
              v-else type="button" class="btn small" :class="{ primary: nextOpen === t }" :disabled="off"
              :aria-label="`Start: ${t.text}`" :aria-describedby="blocked ? 'plan-why' : undefined" @click="start(t)"
            >Start</button>
          </template>
        </li>
      </ul>
      <p class="caption muted">To change the plan, ask in Chat with Plan first, then press Use as project plan.</p>
    </template>
    <p class="sr-only" role="status" aria-live="polite">{{ announce }}</p>
  </div>
</template>

<style scoped>
.plan { display: flex; flex-direction: column; gap: 12px; flex: 1; min-height: 0; min-width: 0; overflow: auto; }
.empty { max-width: 640px; }
.empty p { margin: 0 0 12px; }
.empty label { display: block; font-weight: 600; margin-bottom: 4px; }
.empty textarea { width: 100%; resize: vertical; margin-bottom: 12px; }
.status { display: flex; flex-direction: column; gap: 8px; }
.row { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
.text { flex: 1 1 280px; min-width: 0; }
.sentence { margin: 0; font-weight: 600; }
.caption { margin: 0; font-size: 13px; overflow-wrap: anywhere; }
.btn.primary, .btn.small { min-height: 32px; }
.bar { appearance: none; -webkit-appearance: none; width: 100%; height: 6px; border: 0; border-radius: 999px; overflow: hidden; background: var(--border); color: var(--accent); }
.bar::-webkit-progress-bar { background: var(--border); border-radius: 999px; }
.bar::-webkit-progress-value { background: var(--accent); border-radius: 999px; }
.bar::-moz-progress-bar { background: var(--accent); border-radius: 999px; }
.bar.done { color: var(--ok); }
.bar.done::-webkit-progress-value { background: var(--ok); }
.bar.done::-moz-progress-bar { background: var(--ok); }
.summary :deep(p) { margin: 0 0 8px; }
.summary summary { cursor: pointer; color: var(--accent-text); font-size: 13px; }
.tasks { list-style: none; margin: 0; padding: 0; }
.task { display: flex; gap: 10px; align-items: flex-start; padding: 8px 0; min-height: 32px; border-bottom: 1px solid var(--border); }
.task:first-child { border-top: 1px solid var(--border); }
.task input { flex: none; margin: 3px 0 0; width: 16px; height: 16px; }
.body { flex: 1; min-width: 0; }
.title { overflow-wrap: anywhere; }
.task.done .title { color: var(--muted); }
.detail { margin-top: 2px; }
.task .btn, .task .badge { flex: none; align-self: flex-start; }
.badge.info { display: inline-flex; align-items: center; gap: 6px; background: var(--info-bg); }
.spin { display: inline-block; width: 12px; height: 12px; margin-left: 8px; border: 2px solid var(--border-strong); border-top-color: var(--accent); border-radius: 50%; animation: spin 0.8s linear infinite; vertical-align: -1px; }
.badge .spin { margin-left: 0; }
@keyframes spin { to { transform: rotate(360deg); } }
@media (prefers-reduced-motion: reduce) { .spin { animation: none; } }
.first { margin: 0 0 8px; font-weight: 600; }
.actions { margin: 0 0 8px; }
.details pre { margin: 4px 0 0; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 12px; }
</style>
