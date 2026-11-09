<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { CircleAlert, Ellipsis, RefreshCw, Search } from "lucide-vue-next";
import { commands } from "../bindings";
import type { AppDbOverview, DataGuard, DataSet, ProjectSummary, RowsPage } from "../bindings";
import { call } from "../lib/call";
import { formatSize } from "../lib/format";
import { SET_LABEL, addDatabasePrompt, askAboutDataPrompt, contents, fixDatabasePrompt, num, rowsLabel, setName } from "../lib/appdb";
import { relTime } from "../components/github/time";
import { useBuildsStore } from "../stores/builds";
import { useNavStore } from "../stores/nav";
import { usePreviewStore } from "../stores/preview";
import { useToastStore } from "../stores/toast";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import BackupsDialog from "../components/database/BackupsDialog.vue";
import RowsTable from "../components/database/RowsTable.vue";

const props = defineProps<{ project: ProjectSummary }>();
const nav = useNavStore();
/** The query time limit lives in Settings › Limits (design D2). */
function openLimits() {
  nav.settingsSection = "limits";
  nav.go("settings");
}
const builds = useBuildsStore();
const preview = usePreviewStore();
const toast = useToastStore();

const id = computed(() => props.project.id);
const which = ref<DataSet>("preview");
const overview = ref<AppDbOverview | null>(null);
const guard = ref<DataGuard | null>(null);
const failure = ref<string | null>(null);
const loading = ref(true);
/** The spinner shows only for work over 400 ms (DESIGN.md "States"). */
const slow = ref(false);
const announce = ref("");
const now = ref(Date.now());

const selected = ref<string | null>(null);
const page = ref<RowsPage | null>(null);
const rowsFailure = ref<string | null>(null);
const filterText = ref("");
const applied = ref("");

const busy = ref<"backup" | "copy" | "reset" | null>(null);
const confirming = ref<"copy" | "reset" | null>(null);
const backups = ref(false);

let overviewSeq = 0;
let rowsSeq = 0;
let slowTimer: ReturnType<typeof setTimeout> | undefined;
let filterTimer: ReturnType<typeof setTimeout> | undefined;
let tick: ReturnType<typeof setInterval> | undefined;

async function loadRows(offset: number) {
  const table = selected.value;
  if (!table) return void (page.value = null);
  const seq = ++rowsSeq;
  const r = await call(commands.appdbRows(id.value, which.value, table, offset, applied.value || null));
  if (seq !== rowsSeq) return;
  if (r.ok) {
    page.value = r.data;
    rowsFailure.value = null;
  } else {
    page.value = null;
    rowsFailure.value = r.error.message;
  }
}

async function load() {
  const seq = ++overviewSeq;
  loading.value = true;
  slow.value = false;
  clearTimeout(slowTimer);
  slowTimer = setTimeout(() => (slow.value = true), 400);
  const [o, g] = await Promise.all([call(commands.appdbOverview(id.value, which.value)), call(commands.appdbGuard(id.value))]);
  if (seq !== overviewSeq) return;
  clearTimeout(slowTimer);
  loading.value = false;
  slow.value = false;
  if (g.ok) guard.value = g.data;
  if (!o.ok) {
    failure.value = o.error.message;
    overview.value = null;
    return;
  }
  failure.value = null;
  overview.value = o.data;
  const names = o.data.tables.map((t) => t.name);
  if (!selected.value || !names.includes(selected.value)) {
    selected.value = names[0] ?? null;
    clearFilter(false);
  }
  await loadRows(page.value && selected.value === page.value.table ? page.value.offset : 0);
}

onMounted(() => {
  void load();
  tick = setInterval(() => (now.value = Date.now()), 30_000);
  // Status events keep the "Preview stops" refresh honest; harmless when there is no bridge (tests).
  void preview.load(id.value).catch(() => {});
});
onUnmounted(() => {
  clearTimeout(slowTimer);
  clearTimeout(filterTimer);
  clearInterval(tick);
});
watch(id, () => {
  overview.value = null;
  page.value = null;
  selected.value = null;
  which.value = "preview";
  clearFilter(false);
  void load();
});
// Freshness: re-read when Preview stops (the file just stopped changing).
const previewLive = computed(() => !!preview.status[id.value]?.live);
watch(previewLive, (live, was) => was && !live && void load());

function switchTo(w: DataSet) {
  if (w === which.value) return;
  which.value = w;
  overview.value = null;
  page.value = null;
  selected.value = null;
  clearFilter(false);
  announce.value = `Showing ${setName(w)}`;
  void load();
}
function refresh() {
  announce.value = "Reading data…";
  void load().then(() => (announce.value = "Data refreshed"));
}

// --- tables ---
const tables = computed(() => overview.value?.tables ?? []);
const optId = (i: number) => `db-t-${i}`;
const activeIndex = computed(() => tables.value.findIndex((t) => t.name === selected.value));
function select(name: string) {
  if (name === selected.value) return;
  selected.value = name;
  clearFilter(false);
  void loadRows(0);
}
function onListKey(e: KeyboardEvent) {
  const n = tables.value.length;
  if (!n) return;
  const i = Math.max(0, activeIndex.value);
  const to = e.key === "ArrowDown" ? Math.min(n - 1, i + 1) : e.key === "ArrowUp" ? Math.max(0, i - 1) : e.key === "Home" ? 0 : e.key === "End" ? n - 1 : -1;
  if (to < 0) return;
  e.preventDefault();
  select(tables.value[to]!.name);
}

// --- filter: debounced 300 ms (R9) ---
function clearFilter(reload = true) {
  clearTimeout(filterTimer);
  filterText.value = "";
  applied.value = "";
  if (reload) void loadRows(0);
}
watch(filterText, (v) => {
  clearTimeout(filterTimer);
  if (v.trim() === applied.value) return;
  filterTimer = setTimeout(() => {
    applied.value = v.trim();
    void loadRows(0);
  }, 300);
});

// --- status row ---
const state = computed(() => overview.value?.state ?? null);
const ok = computed(() => state.value === "ok");
const sentence = computed(() => {
  const o = overview.value;
  if (!o) return `Reading ${setName(which.value)}…`;
  const head = SET_LABEL[which.value];
  if (o.state !== "ok") return head;
  const t = o.tables.length;
  return `${head} · SQLite · ${t} ${t === 1 ? "table" : "tables"}${o.sizeBytes != null ? ` · ${formatSize(o.sizeBytes)}` : ""}`;
});
const updated = computed(() => (overview.value?.modifiedAt ? relTime(Date.parse(overview.value.modifiedAt), now.value) : null));
const schema = computed(() => overview.value?.schemaVersion);

// Disabled-with-reason (DR-15): Preview running blocks Copy/Reset, the installed app running blocks its data.
const previewWhy = computed(() => (guard.value?.previewRunning ? "Stop Preview first" : null));
const installedWhy = computed(() => (guard.value?.installedRunning ? `Quit ${guard.value.appName} first` : null));
const why = computed(() => (which.value === "preview" ? previewWhy.value : installedWhy.value ?? previewWhy.value));
const working = computed(() => busy.value !== null);
const busyLabel = { backup: "Backing up…", copy: "Copying…", reset: "Deleting…" } as const;

const menu = ref<HTMLDetailsElement | null>(null);
function pick(fn: () => void) {
  menu.value?.removeAttribute("open");
  fn();
}

const resetBody = computed(() => (state.value === "damaged" ? "The unreadable preview file is deleted" : contents(tables.value)));
async function run(kind: "copy" | "reset") {
  confirming.value = null;
  busy.value = kind;
  const r = await call(kind === "copy" ? commands.appdbCopyInstalledToPreview(id.value) : commands.appdbResetPreview(id.value));
  busy.value = null;
  if (!r.ok) {
    toast.show(r.error.message);
    announce.value = r.error.message;
    return;
  }
  const text = kind === "copy" ? "Preview data replaced" : "Preview data deleted";
  toast.show(text);
  announce.value = text;
  if (which.value === "preview") await load();
}
function onBackupsChanged(text: string) {
  announce.value = text;
  void load();
}
function openBackups() {
  backups.value = true;
  void call(commands.appdbGuard(id.value)).then((g) => g.ok && (guard.value = g.data));
}

// --- empty states (DR-6) ---
const askAdd = () => nav.askClaude({ projectId: id.value, text: addDatabasePrompt() });
const askAbout = () => nav.askClaude({ projectId: id.value, text: askAboutDataPrompt(which.value, selected.value) });
const askFix = () => nav.askClaude({ projectId: id.value, text: fixDatabasePrompt(which.value, overview.value?.path ?? null, overview.value?.error ?? null) });
const launch = () => void builds.launch(id.value);

const emptyRows = computed(() => !!page.value && !page.value.rows.length && !page.value.timedOut && !applied.value && page.value.total === 0);
const noMatch = computed(() => !!page.value && !page.value.rows.length && !page.value.timedOut && !!applied.value);
const tableLabel = (n: string, c: string) => `${n} · ${c}`;
</script>

<template>
  <div class="db">
    <p v-if="failure" class="callout danger" role="alert">{{ failure }}</p>

    <!-- No database in the project (DR-1, DR-2) -->
    <div v-if="state === 'noDatabase'" class="empty">
      <p>This app doesn't store data yet. A database keeps things like scores, notes or settings between runs.</p>
      <button type="button" class="btn primary" @click="askAdd">Ask Claude to add a database</button>
    </div>

    <template v-else>
      <section class="status" aria-label="Database">
        <div class="row">
          <div class="text">
            <p class="sentence">
              {{ sentence }}
              <span v-if="loading && slow" class="spin" aria-hidden="true" />
            </p>
            <p v-if="overview?.path" class="caption muted file">
              <span class="mono path" :title="overview.path"><bdi>{{ overview.path }}</bdi></span>
              <span class="meta"><template v-if="schema != null">&nbsp;· schema version <span class="tnum">{{ schema }}</span></template><template v-if="updated">&nbsp;· updated {{ updated }}</template></span>
              <button type="button" class="btn small icon" aria-label="Refresh data" title="Refresh data" @click="refresh"><RefreshCw :size="14" :stroke-width="1.75" aria-hidden="true" /></button>
            </p>
            <p v-else class="caption muted">
              <button type="button" class="btn small icon" aria-label="Refresh data" title="Refresh data" @click="refresh"><RefreshCw :size="14" :stroke-width="1.75" aria-hidden="true" /></button>
            </p>
          </div>
          <div class="buttons">
            <div class="seg" role="radiogroup" aria-label="Data">
              <label v-for="w in (['preview', 'installed'] as const)" :key="w" :class="{ on: which === w }">
                <input type="radio" name="db-which" :value="w" :checked="which === w" @change="switchTo(w)" />
                {{ SET_LABEL[w] }}
              </label>
            </div>
            <button type="button" class="btn primary" :disabled="working || !overview" @click="openBackups">{{ busy ? busyLabel[busy] : "Back up…" }}</button>
            <details ref="menu" class="menu" @keydown.esc="menu?.removeAttribute('open')">
              <summary class="btn icon" aria-label="More data actions" :aria-disabled="working || !overview"><Ellipsis :size="16" :stroke-width="1.75" aria-hidden="true" /></summary>
              <div class="pop">
                <button type="button" :disabled="working || !!previewWhy" :aria-describedby="previewWhy ? 'db-why' : undefined" @click="pick(() => (confirming = 'copy'))">Copy installed data to Preview…</button>
                <button v-if="which === 'preview'" type="button" class="danger" :disabled="working || !!previewWhy || !(ok || state === 'damaged')" :aria-describedby="previewWhy ? 'db-why' : undefined" @click="pick(() => (confirming = 'reset'))">Reset preview data…</button>
              </div>
            </details>
          </div>
        </div>
        <p v-if="why" id="db-why" class="caption muted">{{ why }}</p>
      </section>
      <p class="sr-only" role="status" aria-live="polite">{{ announce }}</p>

      <!-- Damaged file (DR-6) -->
      <div v-if="state === 'damaged'" class="callout danger damaged" role="alert">
        <CircleAlert :size="18" :stroke-width="1.75" aria-hidden="true" />
        <div class="why">
          <p class="first">Studio can't read this database</p>
          <details v-if="overview?.error" class="details"><summary>Details</summary><pre class="mono">{{ overview.error }}</pre></details>
        </div>
        <button type="button" class="btn small" @click="askFix">Ask Claude to fix</button>
      </div>

      <div v-else-if="state === 'noFile' && which === 'preview'" class="empty">
        <p>No preview data yet. Run the app in Preview to create it.</p>
        <button type="button" class="btn primary" @click="nav.tab = 'preview'">Open Preview</button>
      </div>
      <div v-else-if="state === 'noFile'" class="empty">
        <p>The installed app hasn't created its data yet. Open it once.</p>
        <button type="button" class="btn primary" @click="launch">Launch</button>
      </div>
      <div v-else-if="state === 'notInstalled'" class="empty">
        <p>Not installed. Install a release to see its data.</p>
        <button type="button" class="btn primary" @click="nav.tab = 'releases'">Open Releases</button>
      </div>

      <!-- Tables and rows -->
      <div v-else-if="ok || loading" class="browse" :aria-busy="loading">
        <label class="tsel">
          <span class="sr-only">Table</span>
          <select class="input" aria-label="Table" :value="selected ?? ''" @change="select(($event.target as HTMLSelectElement).value)">
            <option v-for="t in tables" :key="t.name" :value="t.name">{{ tableLabel(t.name, rowsLabel(t)) }}</option>
          </select>
        </label>
        <ul
          class="tlist" role="listbox" aria-label="Tables" tabindex="0"
          :aria-activedescendant="activeIndex >= 0 ? optId(activeIndex) : undefined" @keydown="onListKey"
        >
          <li
            v-for="(t, i) in tables" :id="optId(i)" :key="t.name" role="option" class="opt" :class="{ on: t.name === selected }"
            :aria-selected="t.name === selected" :aria-label="`${t.name}, ${rowsLabel(t)}`" @click="select(t.name)"
          >
            <span class="mono nm">{{ t.name }}</span>
            <span class="cnt tnum">{{ t.manyRows || t.rowCount == null ? "many" : num(t.rowCount) }}</span>
          </li>
        </ul>

        <div class="main">
          <template v-if="selected">
            <div class="bar">
              <label class="filter">
                <span class="sr-only">Filter rows</span>
                <Search :size="14" :stroke-width="1.75" aria-hidden="true" />
                <input v-model="filterText" type="search" class="input" placeholder="Filter rows" autocomplete="off" @keydown.esc.stop="clearFilter()" />
              </label>
              <button type="button" class="link ask" @click="askAbout">Ask Claude about this data</button>
            </div>

            <p v-if="rowsFailure" class="callout danger" role="alert">{{ rowsFailure }}</p>
            <p v-else-if="page?.timedOut" class="muted note">
              <template v-if="applied">Filter took too long; narrow it</template>
              <template v-else>Reading {{ selected }} took too long. <button type="button" class="link limits" @click="openLimits">Raise the limit in Settings › Limits</button></template>
            </p>
            <div v-else-if="emptyRows" class="note"><p class="muted">{{ selected }} has no rows yet.</p></div>
            <div v-else-if="noMatch" class="note">
              <p class="muted">No rows match "{{ applied }}".</p>
              <button type="button" class="btn small" @click="clearFilter()">Clear filter</button>
            </div>
            <RowsTable v-else-if="page" :page="page" @go="loadRows" />
          </template>
          <p v-else class="muted note">This database has no tables yet.</p>
        </div>
      </div>
    </template>

    <ConfirmDialog v-if="confirming === 'reset'" title="Delete preview data?" confirm-label="Delete preview data" @confirm="run('reset')" @cancel="confirming = null">
      <p>{{ resetBody }}.</p>
      <p class="muted">The installed app's data is not touched.</p>
    </ConfirmDialog>
    <ConfirmDialog v-if="confirming === 'copy'" title="Replace preview data with the installed app's data?" confirm-label="Replace preview data" @confirm="run('copy')" @cancel="confirming = null">
      <p>Preview data becomes a copy of the installed app's data. What is in Preview data now is deleted.</p>
      <p class="muted">The installed app's data is never changed by this.</p>
    </ConfirmDialog>
    <BackupsDialog
      v-if="backups" :project-id="id" :which="which" :guard="guard" :can-back-up="ok"
      @close="backups = false" @changed="onBackupsChanged"
    />
  </div>
</template>

<style scoped>
.db { display: flex; flex-direction: column; gap: 12px; flex: 1; min-height: 0; min-width: 0; }
.empty { max-width: 640px; }
.empty p { margin: 0 0 12px; }
.status { display: flex; flex-direction: column; gap: 6px; padding-bottom: 12px; border-bottom: 1px solid var(--border); }
.row { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
.text { flex: 1 1 280px; min-width: 0; }
.sentence { margin: 0; font-weight: 600; }
.caption { margin: 0; font-size: 13px; overflow-wrap: anywhere; }
.caption .btn.icon { margin-left: 6px; vertical-align: middle; }
/* One line: the long path ellipsizes (full path in the tooltip), schema and freshness stay visible. */
.caption.file { display: flex; align-items: center; min-width: 0; white-space: nowrap; }
.caption.file .path { overflow: hidden; text-overflow: ellipsis; min-width: 0; direction: rtl; text-align: left; } /* cut the start, keep the file name */
.caption.file .meta, .caption.file .btn { flex: none; }
.buttons { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; }
.btn.icon { display: inline-flex; padding: 4px 8px; min-height: 32px; align-items: center; }
.btn.primary, .seg label { min-height: 32px; }
.spin { display: inline-block; width: 12px; height: 12px; margin-left: 8px; border: 2px solid var(--border-strong); border-top-color: var(--accent); border-radius: 50%; animation: spin 0.8s linear infinite; vertical-align: -1px; }
@keyframes spin { to { transform: rotate(360deg); } }
@media (prefers-reduced-motion: reduce) { .spin { animation: none; } }
.seg { display: inline-flex; border: 1px solid var(--border-strong); border-radius: 8px; overflow: hidden; }
.seg label { position: relative; padding: 6px 12px; cursor: pointer; display: inline-flex; align-items: center; }
.seg label.on { background: var(--accent-strong); color: var(--on-accent); }
.seg input { position: absolute; opacity: 0; inset: 0; margin: 0; cursor: pointer; }
.seg label:focus-within { outline: 2px solid var(--accent); outline-offset: 2px; }
.menu { position: relative; }
.menu summary { list-style: none; cursor: pointer; }
.menu summary::-webkit-details-marker { display: none; }
.pop { position: absolute; right: 0; top: 100%; z-index: 5; min-width: 240px; background: var(--card); border: 1px solid var(--border-strong); border-radius: 8px; padding: 4px; display: flex; flex-direction: column; margin-top: 4px; }
.pop button { font: inherit; background: none; border: 0; text-align: left; padding: 6px 10px; border-radius: 6px; cursor: pointer; color: inherit; min-height: 32px; }
.pop button:hover:not(:disabled) { background: var(--sidebar); }
.pop button:disabled { opacity: 0.5; cursor: not-allowed; }
.pop .danger { color: var(--danger); }
.callout { margin: 0; }
.damaged { display: flex; gap: 10px; align-items: flex-start; }
.why { flex: 1; min-width: 0; }
.first { margin: 0; font-weight: 600; }
.details pre { margin: 4px 0 0; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 12px; }
.browse { display: grid; grid-template-columns: 220px minmax(0, 1fr); gap: 16px; flex: 1; min-height: 0; }
.tsel { display: none; }
.tlist { list-style: none; margin: 0; padding: 0; overflow: auto; align-self: start; max-height: 100%; border-right: 1px solid var(--border); padding-right: 8px; }
.opt { display: flex; justify-content: space-between; gap: 8px; align-items: center; min-height: 40px; padding: 0 10px; border-radius: 6px; cursor: pointer; }
.opt:hover { background: var(--sidebar); }
.opt.on { background: var(--accent-strong); color: var(--on-accent); }
.nm { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.cnt { flex: none; }
.main { display: flex; flex-direction: column; gap: 8px; min-width: 0; min-height: 0; }
.bar { display: flex; gap: 12px; align-items: center; justify-content: space-between; flex-wrap: wrap; }
.filter { display: flex; align-items: center; gap: 6px; flex: 0 1 280px; margin: 0; }
.filter .input { width: 100%; }
.link { font: inherit; background: none; border: 0; padding: 0; color: var(--accent-text); cursor: pointer; }
.link:hover { text-decoration: underline; }
.note { margin: 0; }
.note p { margin: 0 0 8px; }
/* DR-12: below 900px the list becomes a select and the status controls sit under the sentence. */
@media (max-width: 899px) {
  .browse { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr); }
  .tlist { display: none; }
  .tsel { display: block; }
  .text { flex-basis: 100%; }
  .buttons { width: 100%; }
}
</style>
