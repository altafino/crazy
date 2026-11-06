<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { CircleAlert } from "lucide-vue-next";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { commands } from "../bindings";
import type { ExportTarget, ProjectSummary, ReleaseView, RollbackCheck } from "../bindings";
import { call } from "../lib/call";
import { formatSize } from "../lib/format";
import { fixPrompt } from "../lib/preview";
import { isActive, useRunsStore } from "../stores/runs";
import { useBuildsStore } from "../stores/builds";
import { useNavStore } from "../stores/nav";
import { useToastStore } from "../stores/toast";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import Modal from "../components/Modal.vue";
import UninstallDialog from "../components/UninstallDialog.vue";
import LogView from "../components/preview/LogView.vue";
import BuildDialog from "../components/releases/BuildDialog.vue";
import ExportDialog from "../components/releases/ExportDialog.vue";
import ReleaseStepper from "../components/releases/ReleaseStepper.vue";
import ReleaseTable from "../components/releases/ReleaseTable.vue";
import { ACTION_LABELS, activeId, buildSteps, elapsed, exportFixPrompt, failureText, firstLine, highestVersion, installCandidate, kept, TARGET_SHORT, targetList } from "../components/releases/helpers";
import type { RowAction } from "../components/releases/helpers";

const props = defineProps<{ project: ProjectSummary }>();
const nav = useNavStore();
const builds = useBuildsStore();
const runs = useRunsStore();
const toast = useToastStore();

const id = computed(() => props.project.id);
const name = computed(() => props.project.name);
const loaded = ref(false);
async function load() {
  loaded.value = false;
  await builds.load(id.value);
  loaded.value = true;
}
onMounted(load);
watch(id, load);

const status = computed(() => builds.status[id.value] ?? null);
const log = computed(() => builds.logs[id.value] ?? null);
const releases = computed(() => builds.releases[id.value] ?? []);
const install = computed(() => builds.install[id.value] ?? null);
const error = computed(() => builds.error[id.value] ?? null);
const building = computed(() => !!status.value?.live);
const keptReleases = computed(() => kept(releases.value));
const active = computed(() => activeId(install.value));
const activeVersion = computed(() => (install.value?.kind === "installed" ? install.value.version : null));
const candidate = computed(() => installCandidate(releases.value, active.value));

// M4-DR23: Build is off while a Make-changes run is live; the backend refuses too.
const runLive = computed(() => (runs.sessions[id.value] ?? []).some((s) => s.mode === "changes" && isActive(s)));
const buildBlock = computed(() => (runLive.value ? "Claude is changing this project. You can build when the run ends." : null));

const now = ref(Date.now());
let tick: ReturnType<typeof setInterval> | undefined;
onMounted(() => (tick = setInterval(() => (now.value = Date.now()), 1000)));
onUnmounted(() => clearInterval(tick));

// --- build dialog and the Chat handoff (M4-DR13) ---
const dialog = ref(false);
function openDialog() {
  if (!buildBlock.value && !building.value) dialog.value = true;
}
watch(
  () => nav.buildDialogProject,
  (p) => {
    if (p !== id.value) return;
    nav.buildDialogProject = null;
    openDialog();
  },
  { immediate: true },
);

// --- failure callout (M4-DR7) ---
const failed = computed(() => (status.value && !status.value.live && status.value.state === "failed" && !status.value.export ? status.value : null));
const preflight = computed(() => failed.value?.errorKind === "preflight");
const askFix = () =>
  nav.askClaude({ projectId: id.value, text: fixPrompt(firstLine(failed.value?.errorMessage), log.value?.lines ?? []).replace("preview build", "release build") });
const cancelled = computed(() => status.value?.state === "cancelled");

// --- exports for other systems (F22) ---
const exp = computed(() => status.value?.export ?? null);
const exportingNow = computed(() => building.value && !!exp.value);
const exportTitle = computed(() => (exp.value ? `Exporting ${name.value} ${exp.value.version} for ${targetList(exp.value.targets)}` : ""));
const exportStep = computed(() => buildSteps(status.value?.phase, exp.value).find((s) => s.state === "active")?.label ?? "");
const exportBlock = "You can build when the export ends.";
/** One danger callout per target that failed; a job refused before any target (Docker) gets one for the whole export. */
type ExportFailure = { target: ExportTarget | null; message: string };
const exportFailures = computed<ExportFailure[]>(() => {
  const s = status.value;
  if (!s || s.live || !s.export) return [];
  const bad = s.export.results.filter((r) => !r.ok).map((r) => ({ target: r.target as ExportTarget | null, message: firstLine(r.message) || firstLine(s.errorMessage) }));
  return bad.length || s.state !== "failed" ? bad : [{ target: null, message: failureText(s) }];
});
const exportStopped = computed(() => !!exp.value && cancelled.value);
const failureTail = computed(() => (log.value?.lines ?? []).slice(-20).join("\n"));
const askExportFix = (f: ExportFailure) =>
  nav.askClaude({
    projectId: id.value,
    text: exportFixPrompt(f.target ?? exp.value!.targets[0]!, name.value, exp.value!.version, f.message, log.value?.lines ?? []),
  });

const exporting = ref<ReleaseView | null>(null);
const exportsOf = (r: ReleaseView) => builds.exports[id.value]?.find((e) => e.releaseId === r.id);
const existingTargets = (r: ReleaseView) => (exportsOf(r)?.targets.filter((t) => t.files.length).map((t) => t.target) ?? []) as ExportTarget[];
function openExport(r: ReleaseView) {
  if (!buildBlock.value && !building.value) exporting.value = r;
}
async function revealDir(dir: string) {
  try {
    await revealItemInDir(dir);
  } catch {
    toast.show("Could not open the file manager.");
  }
}

// --- status row: one sentence and one primary action per state (M4-DR5, M4-DR12) ---
type Row = { sentence: string; caption?: string; primary: { label: string; run: () => void } | null; launch: boolean; build: boolean };
const buildFirst = { label: "Build first release…", run: () => openDialog() };
const row = computed<Row>(() => {
  const s = install.value;
  const cand = candidate.value;
  const installCand = cand ? { label: `Install ${cand.version}`, run: () => void request(cand, "install") } : null;
  if (s?.kind === "launcherMissing") {
    return { sentence: "Launcher missing", caption: `GNOME can't find ${name.value}'s launcher entry.`, primary: { label: "Repair", run: () => void builds.repair(id.value) }, launch: false, build: true };
  }
  if (s?.kind === "installed") {
    return {
      sentence: `GNOME starts ${name.value} ${s.version}`,
      caption: cand ? `A newer tested build exists: ${cand.version}.` : undefined,
      primary: installCand, launch: true, build: true,
    };
  }
  if (s?.kind === "unknownRelease") {
    return { sentence: `GNOME starts ${name.value}${s.version ? ` ${s.version}` : ""}, which Studio has no kept release for.`, primary: installCand, launch: true, build: true };
  }
  if (!keptReleases.value.length) return { sentence: `${name.value} has no releases yet.`, caption: "Build one to install it in GNOME.", primary: buildFirst, launch: false, build: false };
  return { sentence: `${name.value} is built but not installed.`, primary: installCand, launch: false, build: true };
});
const showBuildPrimary = computed(() => !row.value.primary || row.value.primary === buildFirst);

// --- actions ---
const busy = ref(false);
async function act(r: ReleaseView, _kind: Exclude<RowAction, null>) {
  busy.value = true;
  await builds.activate(id.value, r.id, name.value);
  busy.value = false;
}

// --- data safety around a switch (DR-7, DR-9) ---
type Switch = { r: ReleaseView; kind: Exclude<RowAction, null> };
const activeRelease = computed(() => releases.value.find((r) => r.id === active.value) ?? null);
const schemaChange = (r: ReleaseView) => {
  const a = activeRelease.value?.schemaVersion;
  return a != null && r.schemaVersion != null && a !== r.schemaVersion ? { from: a, to: r.schemaVersion } : null;
};
/** Releases that change the schema ask first (the backup note); one older than the data asks about restoring. */
const switching = ref<Switch | null>(null);
const older = ref<(Switch & { check: RollbackCheck }) | null>(null);
async function request(r: ReleaseView, kind: Exclude<RowAction, null>) {
  if (r.schemaVersion != null) {
    const c = await call(commands.appdbRollbackCheck(id.value, r.id));
    if (c.ok && c.data.olderThanData) return void (older.value = { r, kind, check: c.data });
    if (schemaChange(r)) return void (switching.value = { r, kind });
  }
  await act(r, kind);
}
async function confirmSwitch() {
  const s = switching.value;
  switching.value = null;
  if (s) await act(s.r, s.kind);
}
/** Switch, then put back the data saved before the newer release (DR-7). */
async function resolveOlder(restore: boolean) {
  const o = older.value;
  older.value = null;
  if (!o) return;
  await act(o.r, o.kind);
  const b = o.check.backup;
  if (!restore || !b || builds.error[id.value]) return;
  const r = await call(commands.appdbRestoreBackup(id.value, b.id));
  toast.show(r.ok ? "Installed app data replaced" : r.error.message);
}
const errorLines = computed(() => {
  const [first = "", ...rest] = (error.value?.message ?? "").split("\n");
  return { first, rest: rest.join("\n").trim() };
});
const stop = () => builds.stop(id.value);
const launch = () => builds.launch(id.value);
async function reveal(r: ReleaseView) {
  try {
    await revealItemInDir(r.appimagePath);
  } catch {
    toast.show("Could not open the file manager.");
  }
}

const deleting = ref<ReleaseView | null>(null);
async function confirmDelete() {
  const r = deleting.value;
  deleting.value = null;
  if (r) await builds.removeRelease(id.value, r.id);
}
const uninstalling = ref(false);
async function confirmUninstall() {
  uninstalling.value = false;
  await builds.uninstall(id.value);
}

const logOpen = ref(false);
watch(id, () => (logOpen.value = false));
const title = computed(() => (status.value ? `Building ${status.value.version ?? ""}`.trim() : "Building"));
const time = computed(() => elapsed(status.value?.startedAt ?? null, now.value));
const highest = computed(() => highestVersion(releases.value));
</script>

<template>
  <div class="releases">
    <div v-if="error" class="callout danger" role="alert">
      <p class="msg1">{{ errorLines.first }}</p>
      <details v-if="errorLines.rest" class="details"><summary>Details</summary><pre class="mono">{{ errorLines.rest }}</pre></details>
    </div>

    <!-- Not available: imported folders (like Preview). -->
    <div v-if="!project.supportsOperations" class="empty">
      <h2>Releases aren't available for this project</h2>
      <p class="muted">{{ project.name }} is an imported folder. Studio can only build projects it created from the Tauri template.</p>
    </div>

    <template v-else-if="loaded">
      <!-- Exporting a release for other systems (F22, design 4) -->
      <section v-if="exportingNow && exp" class="status" aria-labelledby="exp-sentence">
        <div class="head">
          <div class="msg">
            <p id="exp-sentence" class="sentence">{{ exportTitle }} <span class="muted tnum">· {{ time }}</span></p>
            <p v-if="exp.firstRun" class="caption muted">First export: downloading the build environment (about 3 GB). Later exports skip this step.</p>
            <p id="exp-block" class="caption muted">{{ exportBlock }}</p>
          </div>
          <div class="btns">
            <button type="button" class="btn small" @click="stop">Stop</button>
            <button type="button" class="btn small" disabled aria-describedby="exp-block">Build new release…</button>
          </div>
        </div>
        <p class="sr-only" aria-live="polite">{{ exportTitle }}{{ exportStep ? `. ${exportStep}` : "" }}</p>
        <ReleaseStepper :phase="status?.phase" :export-job="exp" />
        <LogView v-if="log" label="Export log" :lines="log.lines" :first-seq="log.firstSeq" :truncated="log.truncated" />
      </section>

      <!-- Building (M4-DR4) -->
      <section v-else-if="building" class="status" aria-live="polite">
        <div class="head">
          <div class="msg">
            <p class="sentence">{{ title }} <span class="muted tnum">· {{ time }}</span></p>
            <p class="caption muted">The app opens for 5 seconds during the test launch. Leave it alone; it closes by itself.</p>
          </div>
          <button type="button" class="btn small" @click="stop">Stop</button>
        </div>
        <ReleaseStepper :phase="status?.phase" />
        <LogView v-if="log" label="Build log" :lines="log.lines" :first-seq="log.firstSeq" :truncated="log.truncated" />
      </section>

      <!-- Idle -->
      <template v-else>
        <section class="status">
          <div class="head">
            <div class="msg">
              <p class="sentence">{{ row.sentence }}</p>
              <p v-if="row.caption" class="caption muted">{{ row.caption }}</p>
              <p v-if="buildBlock" class="caption muted">{{ buildBlock }}</p>
            </div>
            <div class="btns">
              <button v-if="row.primary" type="button" class="btn small primary" :disabled="busy || (showBuildPrimary && !!buildBlock)" @click="row.primary.run()">{{ row.primary.label }}</button>
              <button v-if="row.launch" type="button" class="btn small" @click="launch">Launch</button>
              <button v-if="row.build" type="button" class="btn small" :disabled="!!buildBlock" @click="openDialog">Build new release…</button>
            </div>
          </div>
        </section>

        <template v-for="f in exportFailures" :key="f.target ?? 'all'">
          <div class="callout danger build" role="alert">
            <CircleAlert :size="18" :stroke-width="1.75" aria-hidden="true" />
            <div class="why">
              <p class="msg1"><strong>{{ f.target ? `${TARGET_SHORT[f.target]} export failed` : "Export failed" }}</strong></p>
              <code class="mono first">{{ f.message || "See the log for details." }}</code>
              <details v-if="failureTail" class="details"><summary>Details</summary><pre class="mono">{{ failureTail }}</pre></details>
            </div>
            <button v-if="status?.errorKind === 'preflight'" type="button" class="btn small" @click="nav.go('doctor')">Open Doctor</button>
            <button v-else type="button" class="btn small" @click="askExportFix(f)">Ask Claude to fix</button>
          </div>
        </template>
        <div v-if="failed" class="callout danger build" role="alert">
          <CircleAlert :size="18" :stroke-width="1.75" aria-hidden="true" />
          <div class="why">
            <code class="mono first">{{ failureText(failed) }}</code>
            <p v-if="activeVersion" class="caption">GNOME still starts {{ activeVersion }}.</p>
          </div>
          <button v-if="preflight" type="button" class="btn small" @click="nav.go('doctor')">Open Doctor</button>
          <button v-else type="button" class="btn small" @click="askFix">Ask Claude to fix</button>
        </div>
        <p v-else-if="exportStopped" class="muted note">The last export was stopped.</p>
        <p v-else-if="cancelled" class="muted note">The last build was stopped.</p>

        <button v-if="log && status?.hasLog" type="button" class="link logtoggle" :aria-expanded="logOpen" @click="logOpen = !logOpen">Build log</button>
        <LogView v-if="logOpen && log" label="Build log" :lines="log.lines" :first-seq="log.firstSeq" :truncated="log.truncated" />
      </template>

      <ReleaseTable
        v-if="releases.length" :releases="releases" :install="install" :busy="busy"
        :exports="builds.exports[id]" :export-blocked="building || !!buildBlock" :export-done="builds.exportDone[id]"
        @action="request" @delete="(r) => (deleting = r)" @reveal="reveal" @export="openExport" @reveal-dir="revealDir"
      />

      <p v-if="install && install.kind !== 'notInstalled'" class="uninstall">
        <button type="button" class="link danger" @click="uninstalling = true">Uninstall from GNOME…</button>
      </p>
    </template>

    <BuildDialog v-if="dialog" :project-id="id" :highest="highest" @close="dialog = false" />
    <ExportDialog
      v-if="exporting" :project-id="id" :release-id="exporting.id" :app-name="name" :version="exporting.version" :existing="existingTargets(exporting)"
      @close="exporting = null"
    />
    <ConfirmDialog v-if="deleting" :title="`Delete ${name} ${deleting.version}?`" confirm-label="Delete" @confirm="confirmDelete" @cancel="deleting = null">
      <p>This removes the stored AppImage<template v-if="deleting.sizeBytes != null"> ({{ formatSize(deleting.sizeBytes) }})</template>.</p>
      <p class="muted">It can't be reinstalled without building it again. Your project files are not touched.</p>
    </ConfirmDialog>
    <ConfirmDialog
      v-if="switching" :title="`${ACTION_LABELS[switching.kind]} ${switching.r.version}?`" :confirm-label="ACTION_LABELS[switching.kind]"
      @confirm="confirmSwitch" @cancel="switching = null"
    >
      <p>Database changes: schema {{ schemaChange(switching.r)?.from }} → {{ schemaChange(switching.r)?.to }}.</p>
      <p>Studio backs up the app's data first (before {{ switching.r.version }}).</p>
    </ConfirmDialog>
    <Modal v-if="older" :title="`${ACTION_LABELS[older.kind]} ${older.r.version}?`" @close="older = null">
      <p>Version {{ older.r.version }} is older than your data (schema {{ older.check.dataSchemaVersion }} vs {{ older.check.targetSchemaVersion }}). It may not read it correctly.</p>
      <p class="muted">Studio backs up the app's data first.</p>
      <div class="older-actions">
        <button type="button" class="btn" @click="older = null">Cancel</button>
        <button type="button" class="btn" @click="resolveOlder(false)">Keep current data</button>
        <button v-if="older.check.backup" type="button" class="btn primary" @click="resolveOlder(true)">Restore data from {{ older.check.backup.label ?? "the backup" }}</button>
      </div>
    </Modal>
    <UninstallDialog
      v-if="uninstalling" :app-name="name" :version="activeVersion" :releases-kept="keptReleases.length"
      @confirm="confirmUninstall" @cancel="uninstalling = false"
    />
  </div>
</template>

<style scoped>
.releases { display: flex; flex-direction: column; gap: 12px; flex: 1; min-height: 0; overflow-y: auto; }
.empty { max-width: 640px; }
.empty h2 { margin: 0 0 8px; }
.status { display: flex; flex-direction: column; gap: 12px; }
.head { display: flex; gap: 12px; align-items: center; justify-content: space-between; flex-wrap: wrap; }
.msg { min-width: 0; }
.sentence { margin: 0; font-weight: 600; }
.caption { margin: 2px 0 0; font-size: 13px; }
.btns { display: flex; gap: 8px; flex-wrap: wrap; }
.note { margin: 0; font-size: 13px; }
.callout.build { display: flex; gap: 10px; align-items: center; margin: 0; }
.why { flex: 1; min-width: 0; }
.first { overflow-wrap: anywhere; white-space: pre-wrap; }
.link { font: inherit; background: none; border: 0; padding: 0; color: var(--accent-text); cursor: pointer; align-self: flex-start; }
.link:hover { text-decoration: underline; }
.link.danger { color: var(--danger); }
.msg1 { margin: 0; }
.details pre { margin: 4px 0 0; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 12px; }
.older-actions { display: flex; justify-content: flex-end; gap: 8px; flex-wrap: wrap; margin-top: var(--space); }
.uninstall { margin: 8px 0 0; }
</style>
