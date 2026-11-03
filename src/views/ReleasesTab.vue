<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { CircleAlert } from "lucide-vue-next";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import type { ProjectSummary, ReleaseView } from "../bindings";
import { formatSize } from "../lib/format";
import { fixPrompt } from "../lib/preview";
import { isActive, useRunsStore } from "../stores/runs";
import { useBuildsStore } from "../stores/builds";
import { useNavStore } from "../stores/nav";
import { useToastStore } from "../stores/toast";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import UninstallDialog from "../components/UninstallDialog.vue";
import LogView from "../components/preview/LogView.vue";
import BuildDialog from "../components/releases/BuildDialog.vue";
import ReleaseStepper from "../components/releases/ReleaseStepper.vue";
import ReleaseTable from "../components/releases/ReleaseTable.vue";
import { activeId, elapsed, failureText, firstLine, highestVersion, installCandidate, kept } from "../components/releases/helpers";
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
const failed = computed(() => (status.value && !status.value.live && status.value.state === "failed" ? status.value : null));
const preflight = computed(() => failed.value?.errorKind === "preflight");
const askFix = () =>
  nav.askClaude({ projectId: id.value, text: fixPrompt(firstLine(failed.value?.errorMessage), log.value?.lines ?? []).replace("preview build", "release build") });
const cancelled = computed(() => status.value?.state === "cancelled");

// --- status row: one sentence and one primary action per state (M4-DR5, M4-DR12) ---
type Row = { sentence: string; caption?: string; primary: { label: string; run: () => void } | null; launch: boolean; build: boolean };
const buildFirst = { label: "Build first release…", run: () => openDialog() };
const row = computed<Row>(() => {
  const s = install.value;
  const cand = candidate.value;
  const installCand = cand ? { label: `Install ${cand.version}`, run: () => void act(cand, "install") } : null;
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
    <p v-if="error" class="callout danger" role="alert">{{ error.message }}</p>

    <!-- Not available: imported folders (like Preview). -->
    <div v-if="!project.supportsOperations" class="empty">
      <h2>Releases aren't available for this project</h2>
      <p class="muted">{{ project.name }} is an imported folder. Studio can only build projects it created from the Tauri template.</p>
    </div>

    <template v-else-if="loaded">
      <!-- Building (M4-DR4) -->
      <section v-if="building" class="status" aria-live="polite">
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

        <div v-if="failed" class="callout danger build" role="alert">
          <CircleAlert :size="18" :stroke-width="1.75" aria-hidden="true" />
          <div class="why">
            <code class="mono first">{{ failureText(failed) }}</code>
            <p v-if="activeVersion" class="caption">GNOME still starts {{ activeVersion }}.</p>
          </div>
          <button v-if="preflight" type="button" class="btn small" @click="nav.go('doctor')">Open Doctor</button>
          <button v-else type="button" class="btn small" @click="askFix">Ask Claude to fix</button>
        </div>
        <p v-else-if="cancelled" class="muted note">The last build was stopped.</p>

        <button v-if="log && status?.hasLog" type="button" class="link logtoggle" :aria-expanded="logOpen" @click="logOpen = !logOpen">Build log</button>
        <LogView v-if="logOpen && log" label="Build log" :lines="log.lines" :first-seq="log.firstSeq" :truncated="log.truncated" />
      </template>

      <ReleaseTable
        v-if="releases.length" :releases="releases" :install="install" :busy="busy"
        @action="act" @delete="(r) => (deleting = r)" @reveal="reveal"
      />

      <p v-if="install && install.kind !== 'notInstalled'" class="uninstall">
        <button type="button" class="link danger" @click="uninstalling = true">Uninstall from GNOME…</button>
      </p>
    </template>

    <BuildDialog v-if="dialog" :project-id="id" :highest="highest" @close="dialog = false" />
    <ConfirmDialog v-if="deleting" :title="`Delete ${name} ${deleting.version}?`" confirm-label="Delete" @confirm="confirmDelete" @cancel="deleting = null">
      <p>This removes the stored AppImage<template v-if="deleting.sizeBytes != null"> ({{ formatSize(deleting.sizeBytes) }})</template>.</p>
      <p class="muted">It can't be reinstalled without building it again. Your project files are not touched.</p>
    </ConfirmDialog>
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
.uninstall { margin: 8px 0 0; }
</style>
