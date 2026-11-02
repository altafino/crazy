<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { CircleCheck, Loader2, Plus, Trash2, X } from "lucide-vue-next";
import { commands } from "../../bindings";
import type { AgentProfile_Deserialize, FieldError, ProbeResult } from "../../bindings";
import { call } from "../../lib/call";
import { useAgentsStore } from "../../stores/agents";
import { useDoctorStore } from "../../stores/doctor";
import ConfirmDialog from "../ConfirmDialog.vue";

const agents = useAgentsStore();
const doctor = useDoctorStore();

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
}

onMounted(async () => {
  if (!doctor.report) doctor.run(false);
  await agents.load();
  if (agents.profiles[0]) fill(agents.profiles[0]);
});

function reviewProposal() {
  if (agents.proposal) fill(agents.proposal.profile);
  bannerOpen.value = false;
}
function addProfile() {
  fill({ ...blank(), isDefault: false });
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
  saved.value = true;
  await agents.load();
}

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
      <button v-for="p in agents.profiles" :key="p.id" type="button" class="btn small" :aria-pressed="p.id === form.id" @click="fill(p)">{{ p.name }}</button>
      <button type="button" class="btn small" @click="addProfile"><Plus :size="14" aria-hidden="true" /> Add profile</button>
    </div>

    <form class="card" @submit.prevent="save">
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
      <p aria-live="polite" class="ok">{{ saved ? "Saved" : "" }}</p>

      <div class="actions">
        <button type="button" class="btn" :disabled="testing" @click="test">Test profile</button>
        <button type="submit" class="btn primary">Save</button>
        <button v-if="form.id" type="button" class="btn destructive" @click="confirmDelete = true"><Trash2 :size="14" aria-hidden="true" /> Delete</button>
      </div>
    </form>

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
.ok { color: var(--ok); display: flex; align-items: center; gap: 4px; }
.spin { animation: spin 1s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
.callout .btn { margin-right: 8px; }
</style>
