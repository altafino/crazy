<script setup lang="ts">
import { nextTick, onMounted, ref } from "vue";
import { commands, type Settings } from "../bindings";
import PreviewLimits from "../components/settings/PreviewLimits.vue";
import DatabaseLimits from "../components/settings/DatabaseLimits.vue";
import AttachmentLimits from "../components/settings/AttachmentLimits.vue";
import AgentProfiles from "../components/settings/AgentProfiles.vue";
import ApprovalSettings from "../components/settings/ApprovalSettings.vue";
import ExportSettings from "../components/settings/ExportSettings.vue";
import SettingsGroup from "../components/settings/SettingsGroup.vue";
import ToolPaths from "../components/settings/ToolPaths.vue";
import { call, type Outcome } from "../lib/call";
import { saveNotifyFinish } from "../lib/notify";
import { useUpdateStore } from "../stores/update";
import { pickPath } from "../lib/pick";
import { useDoctorStore } from "../stores/doctor";
import { SETTINGS_TABS, useNavStore, type SettingsSection } from "../stores/nav";
import { useSettingsStore } from "../stores/settings";

const settings = useSettingsStore();
const doctor = useDoctorStore();
const nav = useNavStore();
const TABS = SETTINGS_TABS;
const section = ref<SettingsSection>(nav.settingsSection);
/** The tab row scrolls sideways when it doesn't fit; keep the active tab in view. */
function reveal() {
  void nextTick(() => document.getElementById(`set-tab-${section.value}`)?.scrollIntoView?.({ block: "nearest", inline: "nearest" }));
}
/** Leaving AI Agents with unsaved edits asks first (the guard lives in the nav store). */
function select(id: SettingsSection) {
  nav.guarded(() => {
    section.value = id;
    reveal();
  });
}
function onTabKey(e: KeyboardEvent) {
  const i = TABS.findIndex((t) => t.id === section.value);
  const to = { ArrowRight: (i + 1) % TABS.length, ArrowLeft: (i + TABS.length - 1) % TABS.length, Home: 0, End: TABS.length - 1 }[e.key];
  if (to === undefined) return;
  e.preventDefault();
  const id = TABS[to]!.id;
  nav.guarded(() => {
    section.value = id;
    void nextTick(() => document.getElementById(`set-tab-${id}`)?.focus());
    reveal();
  });
}
onMounted(() => {
  reveal();
  if (!doctor.report) void doctor.run(false);
});

// Projects folder: saves through the settings store, the one owner of the document (A3).
const loadSettings = () => (settings.settings ? Promise.resolve({ ok: true as const, data: settings.settings }) : settings.load());
const saveSettings = (d: Settings) => settings.save({ ...d, defaultProjectsDir: d.defaultProjectsDir.trim() });
async function browseDir(draft: Settings, commit: (f: string, v: unknown) => Promise<boolean>) {
  const p = await pickPath(true, draft.defaultProjectsDir);
  if (p) void commit("defaultProjectsDir", p);
}

// Notifications: the backend setting (D8); saving also applies it to finish notifications.
type Notify = { notifyFinish: boolean };
const loadNotify = async (): Promise<Outcome<Notify>> => {
  const r = await call(commands.getUiState());
  return r.ok ? { ok: true, data: { notifyFinish: r.data.notifyFinish } } : r;
};
const saveNotify = async (d: Notify): Promise<Outcome<Notify>> => {
  const r = await saveNotifyFinish(d.notifyFinish);
  return r.ok ? { ok: true, data: d } : r;
};

// Updates: the automatic check for a new Studio release; saving applies it at once.
type Updates = { checkUpdates: boolean };
const loadUpdates = async (): Promise<Outcome<Updates>> => {
  const r = await call(commands.getUiState());
  return r.ok ? { ok: true, data: { checkUpdates: r.data.checkUpdates } } : r;
};
const saveUpdates = async (d: Updates): Promise<Outcome<Updates>> => {
  const r = await call(commands.setCheckUpdates(d.checkUpdates));
  if (r.ok) useUpdateStore().setAuto(d.checkUpdates);
  return r.ok ? { ok: true, data: d } : r;
};
</script>

<template>
  <section aria-labelledby="set-h">
    <div class="view-head"><h1 id="set-h">Settings</h1></div>

    <div role="tablist" aria-label="Settings sections" class="tabs" @keydown="onTabKey">
      <button
        v-for="t in TABS" :id="`set-tab-${t.id}`" :key="t.id" type="button" role="tab" class="tab"
        :aria-selected="section === t.id" :aria-controls="`set-panel-${t.id}`" :tabindex="section === t.id ? 0 : -1"
        @click="select(t.id)"
      >{{ t.label }}</button>
    </div>

    <div v-if="section === 'agents'" id="set-panel-agents" role="tabpanel" aria-labelledby="set-tab-agents"><AgentProfiles /></div>
    <div v-else-if="section === 'approvals'" id="set-panel-approvals" role="tabpanel" aria-labelledby="set-tab-approvals"><ApprovalSettings /></div>

    <div v-else-if="section === 'limits'" id="set-panel-limits" role="tabpanel" aria-labelledby="set-tab-limits">
      <PreviewLimits />
      <AttachmentLimits />
      <DatabaseLimits />
    </div>

    <div v-else-if="section === 'tools'" id="set-panel-tools" role="tabpanel" aria-labelledby="set-tab-tools"><ToolPaths /></div>

    <div v-else id="set-panel-general" role="tabpanel" aria-labelledby="set-tab-general">
      <SettingsGroup v-slot="{ draft, commit, errorFor }" title="Projects" :load="loadSettings" :save="saveSettings">
        <label for="set-dir">Default projects folder</label>
        <div class="row">
          <input
            id="set-dir" v-model="draft.defaultProjectsDir" class="input mono" autocomplete="off" data-field="defaultProjectsDir"
            :aria-invalid="!!errorFor('defaultProjectsDir')" aria-describedby="set-dir-err"
            @blur="commit('defaultProjectsDir', draft.defaultProjectsDir.trim())" @keydown.enter.prevent="commit('defaultProjectsDir', draft.defaultProjectsDir.trim())"
          />
          <button type="button" class="btn" @click="browseDir(draft, commit)">Browse…</button>
        </div>
        <p id="set-dir-err" class="field-error" v-if="errorFor('defaultProjectsDir')">{{ errorFor("defaultProjectsDir") }}</p>
      </SettingsGroup>

      <SettingsGroup v-slot="{ draft, commit }" title="Notifications" :load="loadNotify" :save="saveNotify">
        <label class="check">
          <input v-model="draft.notifyFinish" type="checkbox" @change="commit('notifyFinish', draft.notifyFinish)" />
          <span><strong>Notify when runs and builds finish</strong> <span class="muted">Only while Studio is in the background. Approval requests always notify.</span></span>
        </label>
      </SettingsGroup>

      <SettingsGroup v-slot="{ draft, commit }" title="Updates" :load="loadUpdates" :save="saveUpdates">
        <label class="check">
          <input v-model="draft.checkUpdates" type="checkbox" @change="commit('checkUpdates', draft.checkUpdates)" />
          <span><strong>Check for new versions automatically</strong> <span class="muted">When Studio starts and once a day. About › Check for updates always works.</span></span>
        </label>
      </SettingsGroup>

      <ExportSettings />
    </div>
  </section>
</template>

<style scoped>
.tabs { display: flex; gap: 16px; border-bottom: 1px solid var(--border); margin-bottom: var(--space); overflow-x: auto; white-space: nowrap; scrollbar-width: thin; scrollbar-color: var(--border-strong) transparent; }
.tab { flex: none; background: none; border: 0; border-bottom: 2px solid transparent; padding: 6px 2px; font: inherit; color: var(--muted); cursor: pointer; }
.tab[aria-selected="true"] { border-bottom-color: var(--accent); color: var(--accent-text); }
.row { display: flex; gap: 8px; }
label { display: block; font-weight: 600; margin-bottom: 4px; }
.check { display: flex; gap: 10px; align-items: flex-start; cursor: pointer; font-weight: 400; margin: 0; }
.check input { margin-top: 3px; }
</style>
