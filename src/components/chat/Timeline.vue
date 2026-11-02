<script setup lang="ts">
import { computed, ref } from "vue";
import { Bot, CircleCheck, CircleX, FileText, Info, ListChecks, Pencil, Terminal, TriangleAlert, User, Wrench } from "lucide-vue-next";
import type { End, Row, Step } from "../../lib/chat";
import { editCounts, stepDetail } from "../../lib/chat";
import { clockTime } from "../../lib/format";
import ApprovalCard from "./ApprovalCard.vue";
import Markdown from "./Markdown.vue";
import PlanCard from "./PlanCard.vue";
import RunEnd from "./RunEnd.vue";

const props = defineProps<{
  prompt: string;
  rows: Row[];
  cwd: string;
  active: boolean;
  end: End | null;
  planActions: boolean;
  canRetry: boolean;
}>();
defineEmits<{
  allow: [requestId: string];
  deny: [requestId: string, message: string | null];
  make: [];
  revise: [];
  review: [];
  settings: [];
  retry: [rule: string];
}>();

const open = ref(new Set<string>());
const toggle = (k: string) => (open.value.has(k) ? open.value.delete(k) : open.value.add(k));

const verbs: Record<string, string> = { Bash: "Ran", Edit: "Edited", MultiEdit: "Edited", Write: "Wrote", NotebookEdit: "Edited" };
const verb = (s: Step) => verbs[s.name] ?? s.name;
const icon = (s: Step) => (s.name === "Bash" ? Terminal : ["Edit", "MultiEdit", "Write", "NotebookEdit"].includes(s.name) ? Pencil : Wrench);
/** A trailing spinner node while Claude works, unless it is waiting on you. */
const working = computed(() => props.active && !props.rows.some((r) => r.kind === "approval" && r.approval.answered === null));
const show = (v: unknown) => (typeof v === "string" ? v : JSON.stringify(v, null, 2));
</script>

<template>
  <div class="timeline" role="log" aria-label="Run timeline">
    <div class="row"><span class="node"><User :size="14" :stroke-width="1.75" aria-hidden="true" /></span><p class="prompt">{{ prompt }}</p></div>

    <template v-for="r in rows" :key="r.key">
      <div v-if="r.kind === 'text'" class="row"><span class="node"><Bot :size="14" :stroke-width="1.75" aria-hidden="true" /></span><Markdown :text="r.text" /><time v-if="r.at" class="time tnum">{{ clockTime(r.at) }}</time></div>

      <div v-else-if="r.kind === 'warning'" class="row"><span class="node"><Info :size="14" :stroke-width="1.75" aria-hidden="true" /></span><p class="muted line">{{ r.text }}</p></div>

      <div v-else-if="r.kind === 'tool'" class="row">
        <span class="node" :class="{ running: active && r.step.output === null, failed: r.step.isError }">
          <component :is="icon(r.step)" :size="14" :stroke-width="1.75" aria-hidden="true" />
        </span>
        <button type="button" class="step" :aria-expanded="open.has(r.key)" @click="toggle(r.key)">
          <span class="verb">{{ verb(r.step) }}</span> <span class="mono path">{{ stepDetail(r.step) }}</span>
          <template v-if="editCounts(r.step.name, r.step.input)">
            <span class="tnum add">+{{ editCounts(r.step.name, r.step.input)!.add }}</span>
            <span class="tnum del">−{{ editCounts(r.step.name, r.step.input)!.del }}</span>
          </template>
        </button>
        <time v-if="r.at" class="time tnum">{{ clockTime(r.at) }}</time>
        <div v-if="open.has(r.key)" class="expand mono"><pre>{{ show(r.step.input) }}</pre><pre v-if="r.step.output">{{ r.step.output }}</pre></div>
      </div>

      <div v-else-if="r.kind === 'reads'" class="row">
        <span class="node"><FileText :size="14" :stroke-width="1.75" aria-hidden="true" /></span>
        <button type="button" class="step" :aria-expanded="open.has(r.key)" @click="toggle(r.key)">
          <span class="muted">{{ r.steps.length === 1 ? `Read ${stepDetail(r.steps[0]!)}` : `Read ${r.steps.length} files` }}</span>
        </button>
        <time v-if="r.at" class="time tnum">{{ clockTime(r.at) }}</time>
        <ul v-if="open.has(r.key) && r.steps.length > 1" class="expand mono"><li v-for="s in r.steps" :key="s.id">{{ stepDetail(s) }}</li></ul>
      </div>

      <div v-else-if="r.kind === 'approval'" class="row">
        <span class="node" :class="{ waiting: r.approval.answered === null && active }"><TriangleAlert :size="14" :stroke-width="1.75" aria-hidden="true" /></span>
        <ApprovalCard
          v-if="r.approval.answered === null && active" data-approval :approval="r.approval" :cwd="cwd"
          @allow="$emit('allow', r.approval.requestId)" @deny="(m) => $emit('deny', r.approval.requestId, m)"
        />
        <p v-else class="muted line">{{ r.approval.answered === false ? "Denied" : r.approval.answered ? "Allowed" : "Not answered" }}: {{ r.approval.description || r.approval.tool }}</p>
        <time v-if="r.at" class="time tnum">{{ clockTime(r.at) }}</time>
      </div>

      <div v-else-if="r.kind === 'plan'" class="row">
        <span class="node"><ListChecks :size="14" :stroke-width="1.75" aria-hidden="true" /></span>
        <PlanCard :plan="r.plan" :actions="planActions" @make="$emit('make')" @revise="$emit('revise')" />
      </div>
    </template>

    <div v-if="working" class="row">
      <span class="node running" aria-hidden="true"></span><p class="muted line">Working…</p>
    </div>

    <div v-if="end" class="row">
      <span class="node" :class="{ failed: end.action === 'settings' }">
        <CircleX v-if="end.action === 'settings'" :size="14" :stroke-width="1.75" aria-hidden="true" />
        <CircleCheck v-else :size="14" :stroke-width="1.75" aria-hidden="true" />
      </span>
      <RunEnd :end="end" :can-retry="canRetry" @review="$emit('review')" @settings="$emit('settings')" @retry="(r) => $emit('retry', r)" />
    </div>
  </div>
</template>

<style scoped>
.timeline { position: relative; max-width: 860px; padding-bottom: var(--space); }
.timeline::before { content: ""; position: absolute; left: 11px; top: 20px; bottom: 20px; width: 2px; background: var(--border); }
.row { position: relative; padding: 6px 64px 6px 40px; min-height: 44px; animation: rise 150ms cubic-bezier(0.2, 0, 0, 1); }
@keyframes rise { from { opacity: 0; transform: translateY(4px); } to { opacity: 1; transform: none; } }
.node { position: absolute; left: 0; top: 8px; width: 24px; height: 24px; border-radius: 50%; background: var(--card); border: 1px solid var(--border-strong); color: var(--muted); display: grid; place-items: center; }
.node.running { border-color: var(--accent); animation: spin 1.6s linear infinite; border-top-color: transparent; }
.node.waiting { background: var(--warn-bg); border-color: var(--warn); color: var(--warn); }
.node.failed { color: var(--danger); }
@keyframes spin { to { transform: rotate(360deg); } }
.time { position: absolute; right: 0; top: 10px; font-size: 12px; color: var(--muted); }
.prompt, .line, .row > .md { padding-top: 3px; }
.prompt { margin: 0; font-weight: 600; white-space: pre-wrap; overflow-wrap: anywhere; }
.line { margin: 0; }
.step { font: inherit; display: flex; gap: 8px; align-items: center; min-height: 28px; background: none; border: 0; padding: 0; color: var(--text); cursor: pointer; text-align: left; max-width: 100%; }
.step:hover .verb { text-decoration: underline; }
.verb { font-weight: 600; }
.path { color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.expand { margin: 0 0 4px; padding: 8px 12px; background: var(--sidebar); border-radius: 8px; font-size: 13.5px; list-style: none; overflow-x: auto; }
.expand pre { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; }
.add { color: var(--ok); }
.del { color: var(--danger); }
.expand pre + pre { margin-top: 8px; }
</style>
