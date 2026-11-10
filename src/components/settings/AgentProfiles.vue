<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { CircleCheck, Loader2, Plus, Trash2, X } from "lucide-vue-next";
import { commands } from "../../bindings";
import type { AgentProfile_Deserialize, FieldError, ProbeResult, ProfileUsage, UsageTotals } from "../../bindings";
import { call } from "../../lib/call";
import { useAgentsStore } from "../../stores/agents";
import { useDoctorStore } from "../../stores/doctor";
import { useNavStore } from "../../stores/nav";
import ConfirmDialog from "../ConfirmDialog.vue";
import Modal from "../Modal.vue";

const agents = useAgentsStore();
const doctor = useDoctorStore();
const nav = useNavStore();

interface Form {
  id: string;
  name: string;
  provider: string;
  executable: string;
  env: { name: string; value: string }[];
  model: string;
  inactivitySecs: number;
  instructions: string;
  isDefault: boolean;
  enabled: boolean;
}

const blank = (): Form => ({ id: "", name: "Claude", provider: "claude", executable: "", env: [], model: "", inactivitySecs: 600, instructions: "", isDefault: true, enabled: true });

const form = ref<Form>(blank());
const errors = ref<FieldError[]>([]);
const message = ref<string | null>(null);
const saved = ref(false);
const testing = ref(false);
const probe = ref<ProbeResult | null>(null);
const confirmDelete = ref(false);
/** The form as last loaded or saved; any difference is an unsaved edit (D14). */
const baseline = ref(JSON.stringify(form.value));
const dirty = computed(() => JSON.stringify(form.value) !== baseline.value);
/** Set while "Discard changes?" is open: what to do once the person discards. */
const leaving = ref<(() => void) | null>(null);
/** Runs `go` at once, or after the person agrees to drop their edits. */
function guard(go: () => void) {
  if (dirty.value) leaving.value = go;
  else go();
}
function discard() {
  const go = leaving.value;
  leaving.value = null;
  go?.();
}
const bannerOpen = ref(true);

const claudeTool = computed(() => doctor.report?.tools.find((t) => t.id === "claude"));
const noCli = computed(() => claudeTool.value?.state === "missing");
const hint = computed(() => (claudeTool.value?.state === "missing" ? claudeTool.value.hint : ""));
const aliasEnv = computed(() => agents.proposal?.profile.env.map((e) => `${e.name}=${e.value}`).join(" ") ?? "");
const showBanner = computed(() => bannerOpen.value && !agents.profiles.length && !!agents.proposal && !form.value.id);

/** The backend decides the verified range (claude/mod.rs VERIFIED_MIN). */
const unverified = computed(() => !!probe.value?.ok && !probe.value.verified);
const versionLabel = computed(() => probe.value?.version?.match(/\d+\.\d+\.\d+/)?.[0] ?? probe.value?.version ?? "");

function fill(p: { id: string; name: string; provider: string; executable: string; env: { name: string; value: string }[]; model?: string | null; inactivitySecs: number; instructions: string; isDefault: boolean; enabled: boolean }) {
  form.value = { ...p, env: p.env.map((e) => ({ ...e })), model: p.model ?? "" };
  errors.value = [];
  probe.value = null;
  saved.value = false;
  message.value = null;
  baseline.value = JSON.stringify(form.value);
}

let savedTimer: ReturnType<typeof setTimeout> | undefined;
function flashSaved() {
  saved.value = true;
  clearTimeout(savedTimer);
  savedTimer = setTimeout(() => (saved.value = false), 2000);
}
// Switching Settings tab or view asks too: the nav store calls this before it leaves.
nav.leaveGuard = (proceed) => {
  if (!dirty.value) return false;
  leaving.value = proceed;
  return true;
};
onBeforeUnmount(() => {
  clearTimeout(savedTimer);
  nav.leaveGuard = null;
});

onMounted(async () => {
  if (!doctor.report) doctor.run(false);
  await agents.load();
  if (agents.profiles[0]) fill(agents.profiles[0]);
});

function reviewProposal() {
  guard(() => {
    if (agents.proposal) fill(agents.proposal.profile);
    bannerOpen.value = false;
  });
}
function addProfile() {
  guard(() => fill({ ...blank(), isDefault: false }));
}

const payload = (): AgentProfile_Deserialize => ({ ...form.value, model: form.value.model.trim() || null });
const fieldErr = (f: string) => errors.value.filter((e) => e.field === f);
const envErr = (name: string) => fieldErr(`env.${name}`);

async function test() {
  testing.value = true;
  probe.value = null;
  const r = await call(commands.probeAgentProfile(payload()));
  testing.value = false;
  probe.value = r.ok ? r.data : { ok: false, version: null, verified: false, message: r.error.message };
}

async function save() {
  errors.value = [];
  message.value = null;
  saved.value = false;
  const r = await call(commands.saveAgentProfile(payload()));
  if (!r.ok) {
    errors.value = r.error.fieldErrors;
    if (!r.error.fieldErrors.length) message.value = r.error.message;
    return;
  }
  form.value.id = r.data.id;
  baseline.value = JSON.stringify(form.value);
  flashSaved();
  await agents.load();
}

// ---- usage of the saved profile shown
const usage = ref<ProfileUsage | null>(null);
const usageError = ref<string | null>(null);
watch(
  () => form.value.id,
  async (id) => {
    usage.value = null;
    usageError.value = null;
    if (!id) return;
    const r = await call(commands.agentProfileUsage(id));
    if (id !== form.value.id) return; // switched profile meanwhile
    if (r.ok) usage.value = r.data;
    else usageError.value = r.error.message;
  },
  { immediate: true },
);
const PERIODS = [
  ["today", "Today"],
  ["week", "Last 7 days"],
  ["month", "Last 30 days"],
  ["all", "All time"],
] as const;
const COLUMNS = [
  ["inputTokens", "Input"],
  ["outputTokens", "Output"],
  ["cacheReadTokens", "Cache read"],
  ["cacheWriteTokens", "Cache write"],
] as const;
const compact = new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 });
const exact = new Intl.NumberFormat();
const usd = new Intl.NumberFormat(undefined, { style: "currency", currency: "USD" });
const money = (n: number | null) => usd.format(n ?? 0); // specta types f64 as nullable
const tokens = (t: UsageTotals) => COLUMNS.reduce((sum, [k]) => sum + (t[k] ?? 0), 0);
const month = computed(() => usage.value?.month);

async function remove() {
  confirmDelete.value = false;
  const r = await call(commands.deleteAgentProfile(form.value.id));
  if (!r.ok) {
    message.value = r.error.message;
    return;
  }
  await agents.load();
  fill(agents.profiles[0] ?? blank());
}
</script>

<template>
  <div class="agents">
    <p v-if="noCli" class="callout warn" role="status">
      Claude Code isn't installed or isn't on your PATH. <span v-if="hint">{{ hint }}</span>
    </p>

    <div v-if="showBanner" class="callout" role="region" aria-label="Suggested profile">
      <p>
        We found your Claude setup from your shell alias 'claude'<span v-if="aliasEnv"> ({{ aliasEnv }})</span>. Use it?
      </p>
      <button type="button" class="btn small primary" @click="reviewProposal">Review and save</button>
      <button type="button" class="btn small" @click="bannerOpen = false">Dismiss</button>
    </div>

    <div v-if="agents.profiles.length" class="picker" role="group" aria-label="Profiles">
      <button v-for="p in agents.profiles" :key="p.id" type="button" class="btn small" :aria-pressed="p.id === form.id" @click="guard(() => fill(p))">{{ p.name }}</button>
      <button type="button" class="btn small" @click="addProfile"><Plus :size="14" aria-hidden="true" /> Add profile</button>
    </div>

    <form class="card" @submit.prevent="save">
      <div class="head">
        <h2>Profile</h2>
        <span class="caption" aria-live="polite"><span v-if="saved" class="ok"><CircleCheck :size="14" aria-hidden="true" /> Saved</span></span>
      </div>

      <div class="field">
        <label for="ag-name">Name</label>
        <input id="ag-name" v-model="form.name" class="input" autocomplete="off" :aria-invalid="fieldErr('name').length > 0" />
        <p v-for="e in fieldErr('name')" :key="e.message" class="field-error">{{ e.message }}</p>
      </div>

      <div class="field">
        <label for="ag-exe">Executable path</label>
        <input id="ag-exe" v-model="form.executable" class="input mono" autocomplete="off" :aria-invalid="fieldErr('executable').length > 0" />
        <p v-for="e in fieldErr('executable')" :key="e.message" class="field-error">{{ e.message }}</p>
        <p v-if="testing" class="muted" role="status"><Loader2 :size="14" class="spin" aria-hidden="true" /> Testing…</p>
        <p v-else-if="probe?.ok" class="ok" role="status"><CircleCheck :size="14" aria-hidden="true" /> Works · v{{ versionLabel }}</p>
        <p v-else-if="probe" class="field-error" role="status">{{ probe.message }}</p>
        <p v-if="unverified" class="callout warn" role="status">Not verified with this version; approvals may fall back to strict mode</p>
      </div>

      <fieldset class="field">
        <legend>Environment variables</legend>
        <div v-for="(row, i) in form.env" :key="i">
          <div class="env-row">
            <input v-model="row.name" class="input mono" :aria-label="`Variable name ${i + 1}`" autocomplete="off" :aria-invalid="envErr(row.name).length > 0" />
            <span aria-hidden="true">=</span>
            <input v-model="row.value" class="input mono" :aria-label="`Value ${i + 1}`" autocomplete="off" :aria-invalid="envErr(row.name).length > 0" />
            <button type="button" class="btn small" :aria-label="`Remove ${row.name || 'variable'}`" @click="form.env.splice(i, 1)"><X :size="14" aria-hidden="true" /></button>
          </div>
          <p v-for="e in envErr(row.name)" :key="e.message" class="field-error">{{ e.message }}</p>
        </div>
        <button type="button" class="btn small" @click="form.env.push({ name: '', value: '' })"><Plus :size="14" aria-hidden="true" /> Add variable</button>
        <p class="muted caption">Secret-looking variables (tokens, keys) are refused; sign in through the Claude CLI instead.</p>
      </fieldset>

      <div class="field">
        <label for="ag-model">Model (optional)</label>
        <input id="ag-model" v-model="form.model" class="input mono" autocomplete="off" />
      </div>

      <div class="field">
        <label for="ag-idle">Inactivity limit (seconds)</label>
        <input id="ag-idle" v-model.number="form.inactivitySecs" type="number" min="60" max="7200" class="input" :aria-invalid="fieldErr('inactivitySecs').length > 0" />
        <p v-for="e in fieldErr('inactivitySecs')" :key="e.message" class="field-error">{{ e.message }}</p>
      </div>

      <div class="field">
        <label for="ag-instr">Instructions</label>
        <textarea id="ag-instr" v-model="form.instructions" class="input" rows="6"></textarea>
      </div>

      <div class="field flags">
        <label><input v-model="form.enabled" type="checkbox" /> Enabled</label>
        <label><input v-model="form.isDefault" type="checkbox" /> Default profile</label>
      </div>

      <p v-if="message" class="callout danger" role="alert">{{ message }}</p>

      <div class="actions">
        <button type="button" class="btn" :disabled="testing" @click="test">Test profile</button>
        <button type="submit" class="btn primary">Save</button>
        <button v-if="form.id" type="button" class="btn destructive" @click="confirmDelete = true"><Trash2 :size="14" aria-hidden="true" /> Delete</button>
      </div>
      <p class="muted note">Profiles save with Save. Test first if you like.</p>
    </form>

    <section v-if="form.id" class="card" aria-labelledby="ag-usage">
      <div class="head">
        <h2 id="ag-usage">Usage</h2>
        <span v-if="month?.runs" class="caption muted num">
          Last 30 days: {{ compact.format(tokens(month)) }} tokens · {{ money(month.costUsd) }}
        </span>
      </div>
      <p v-if="usageError" class="field-error" role="status">Couldn't load usage: {{ usageError }}</p>
      <p v-else-if="!usage" class="muted" role="status"><Loader2 :size="14" class="spin" aria-hidden="true" /> Counting…</p>
      <p v-else-if="!usage.all.runs" class="muted">No finished runs with this profile yet.</p>
      <template v-else>
        <div class="scroll">
          <table class="usage">
            <thead>
              <tr>
                <th scope="col"><span class="sr-only">Period</span></th>
                <th scope="col">Runs</th>
                <th v-for="[k, label] in COLUMNS" :key="k" scope="col">{{ label }}</th>
                <th scope="col">Est. cost</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="[p, label] in PERIODS" :key="p" :class="{ empty: !usage[p].runs }">
                <th scope="row">{{ label }}</th>
                <td>{{ exact.format(usage[p].runs) }}</td>
                <td v-for="[k] in COLUMNS" :key="k" :title="`${exact.format(usage[p][k] ?? 0)} tokens`">{{ compact.format(usage[p][k] ?? 0) }}</td>
                <td>{{ money(usage[p].costUsd) }}</td>
              </tr>
            </tbody>
          </table>
        </div>
        <p class="muted note">Finished runs only. The cost is Claude Code's estimate at API prices; on a Claude subscription you aren't billed per token.</p>
      </template>
    </section>

    <Modal v-if="leaving" :title="`Discard changes to ${form.name || 'this profile'}?`" @close="leaving = null">
      <div class="body"><p>Your edits to this profile haven't been saved.</p></div>
      <div class="dialog-actions">
        <button type="button" class="btn" @click="leaving = null">Keep editing</button>
        <button type="button" class="btn destructive filled" @click="discard">Discard</button>
      </div>
    </Modal>

    <ConfirmDialog v-if="confirmDelete" title="Delete profile?" confirm-label="Delete" @confirm="remove" @cancel="confirmDelete = false">
      <p>Delete "{{ form.name }}"? Past runs keep their history.</p>
    </ConfirmDialog>
  </div>
</template>

<style scoped>
.card { background: var(--card); border: 1px solid var(--border); border-radius: var(--radius); padding: var(--space); margin-bottom: var(--space); }
.field { margin-bottom: 12px; border: 0; padding: 0; }
label, legend { display: block; font-weight: 600; margin-bottom: 4px; }
.flags label { display: inline-block; font-weight: 400; margin-right: 16px; }
.picker, .actions { display: flex; gap: 8px; margin-bottom: var(--space); flex-wrap: wrap; }
.env-row { display: flex; gap: 8px; align-items: center; margin-bottom: 6px; }
.caption { font-size: 0.85em; }
.head { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; }
.head .caption { font-size: 13px; white-space: nowrap; }
.note { margin: 12px 0 0; font-size: 13px; }
.dialog-actions { display: flex; justify-content: flex-end; gap: 8px; }
.body p { margin: 0 0 var(--space); }
.ok { color: var(--ok); display: flex; align-items: center; gap: 4px; }
.spin { animation: spin 1s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
.callout .btn { margin-right: 8px; }
/* Release-table pattern (DESIGN.md): semantic table, caption-size muted header, 1px dividers. */
.num, .usage td { font-variant-numeric: tabular-nums; }
.scroll { overflow-x: auto; }
.usage { width: 100%; border-collapse: collapse; }
.usage th, .usage td { padding: 0 12px; height: 40px; text-align: right; white-space: nowrap; border-bottom: 1px solid var(--border); }
.usage th:first-child { text-align: left; padding-left: 0; }
.usage td:last-child, .usage thead th:last-child { padding-right: 0; }
.usage thead th { font-size: 13px; font-weight: 400; color: var(--muted); height: 32px; }
.usage tbody th { font-weight: 600; }
.usage tbody tr:last-child > * { border-bottom: 0; }
.usage tr.empty td { color: var(--muted); }
</style>
