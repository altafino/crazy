<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from "vue";
import { ChevronDown, ChevronUp, Ellipsis } from "lucide-vue-next";
import type { ExportFile, ExportTargetView, ReleaseExports, ReleaseView } from "../../bindings";
import { formatSize } from "../../lib/format";
import { useToastStore } from "../../stores/toast";
import { ACTION_LABELS, activeId as activeOf, exportTime, FILE_PURPOSE, firstLine, kept, previousId, rowAction, TARGET_CAPTION, TARGET_LABELS, TARGET_SHORT } from "./helpers";
import type { RowAction } from "./helpers";
import type { InstallState } from "../../bindings";

const props = defineProps<{
  releases: ReleaseView[]; install: InstallState | null; busy?: boolean;
  /** Exports per release (F22); `n` changes each time an export ends, which opens that release's area. */
  exports?: ReleaseExports[]; exportBlocked?: boolean; exportDone?: { releaseId: string; n: number } | null;
}>();
const emit = defineEmits<{
  action: [r: ReleaseView, a: Exclude<RowAction, null>]; delete: [r: ReleaseView]; reveal: [r: ReleaseView];
  export: [r: ReleaseView]; revealDir: [dir: string];
}>();
const toast = useToastStore();

const showRemoved = ref(false);
const details = ref(new Set<string>());
const active = computed(() => activeOf(props.install));
const installed = computed(() => props.install?.kind === "installed");
const activeRow = computed(() => props.releases.find((r) => r.id === active.value) ?? null);
const previous = computed(() => previousId(props.releases, active.value));
const removed = computed(() => props.releases.filter((r) => r.filesRemoved));
const rows = computed(() => [...kept(props.releases), ...(showRemoved.value ? removed.value : [])]);

/** DR-9: "Database changes: schema 3 → 4" when this release's schema differs from the active one's. */
const schemaNote = (r: ReleaseView) => {
  const a = activeRow.value?.schemaVersion;
  return a != null && r.schemaVersion != null && r.id !== active.value && a !== r.schemaVersion ? `Database changes: schema ${a} → ${r.schemaVersion}` : null;
};
const when = (iso: string) => new Date(iso).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
const action = (r: ReleaseView) => rowAction(r, active.value, installed.value, activeRow.value?.createdAt ?? null);
const toggle = (id: string) => (details.value.has(id) ? details.value.delete(id) : details.value.add(id));
const smokeText = { passed: "Passed", failed: "Failed", unverified: "Unverified" } as const;
const smokeTone = { passed: "ok", failed: "danger", unverified: "warn" } as const;

// --- exports for other systems (F22) ---
const openExports = ref(new Set<string>());
const exportsOf = (r: ReleaseView) => props.exports?.find((e) => e.releaseId === r.id) ?? null;
const fileCount = (e: ReleaseExports) => e.targets.reduce((n, t) => n + t.files.length, 0);
const toggleLabel = (e: ReleaseExports) => {
  const n = fileCount(e);
  return n ? `Exports · ${n} ${n === 1 ? "file" : "files"}` : "Exports · export failed";
};
const toggleExports = (id: string) => (openExports.value.has(id) ? openExports.value.delete(id) : openExports.value.add(id));
watch(
  () => props.exportDone,
  (d) => d && openExports.value.add(d.releaseId),
  { immediate: true },
);
const heading = (t: ExportTargetView) =>
  [TARGET_LABELS[t.target], t.target === "linux" ? "built on the Ubuntu 22.04 base" : null, t.builtAt ? exportTime(t.builtAt) : null].filter(Boolean).join(" · ");

const copied = ref<string | null>(null);
const announce = ref("");
let copyTimer: ReturnType<typeof setTimeout> | undefined;
onUnmounted(() => clearTimeout(copyTimer));
async function copy(f: ExportFile) {
  try {
    await navigator.clipboard.writeText(f.sha256);
  } catch {
    return void toast.show("Could not copy the checksum.");
  }
  copied.value = f.path;
  announce.value = `SHA-256 of ${f.name} copied`;
  clearTimeout(copyTimer);
  copyTimer = setTimeout(() => (copied.value = null), 2000);
}
/** Middle truncation: the stem shrinks, the last characters (the extension) stay. */
const splitName = (n: string) => ({ head: n.slice(0, Math.max(0, n.length - 12)), tail: n.slice(-12) });

/** The ⋯ menu is a native <details>; picking an item closes it. */
function pick(e: Event, fn: () => void) {
  (e.target as HTMLElement).closest("details")?.removeAttribute("open");
  fn();
}
</script>

<template>
  <div class="wrap">
    <table class="releases">
      <caption class="sr-only">Releases</caption>
      <thead>
        <tr>
          <th scope="col">Version</th>
          <th scope="col" class="col-built">Built</th>
          <th scope="col" class="col-source">Source</th>
          <th scope="col">Test</th>
          <th scope="col">Status</th>
          <th scope="col"><span class="sr-only">Actions</span></th>
        </tr>
      </thead>
      <tbody>
        <template v-for="r in rows" :key="r.id">
        <tr :class="{ gone: r.filesRemoved, open: openExports.has(r.id) && exportsOf(r) }" :data-release="r.version">
          <td>
            <span class="mono ver">{{ r.version }}</span>
            <button
              v-if="exportsOf(r) && !r.filesRemoved" type="button" class="link toggle caption" :aria-expanded="openExports.has(r.id)"
              :aria-controls="`exports-${r.id}`" @click="toggleExports(r.id)"
            >
              {{ toggleLabel(exportsOf(r)!) }}
              <component :is="openExports.has(r.id) ? ChevronUp : ChevronDown" :size="14" :stroke-width="1.75" aria-hidden="true" />
            </button>
            <p v-if="schemaNote(r)" class="caption muted">{{ schemaNote(r) }}</p>
            <p v-if="r.notes" class="caption muted notes">{{ r.notes }}</p>
            <p v-if="r.smoke === 'failed' && r.smokeDetail" class="caption crash">{{ firstLine(r.smokeDetail) }}</p>
            <pre v-if="details.has(r.id) && r.smokeDetail" class="mono detail">{{ r.smokeDetail }}</pre>
            <p class="caption muted narrow-only">{{ when(r.createdAt) }} · <span class="mono">{{ r.sourceCommit?.slice(0, 7) ?? "no commit" }}</span></p>
          </td>
          <td class="col-built tnum">{{ when(r.createdAt) }}<span v-if="r.sizeBytes != null" class="caption muted"> · {{ formatSize(r.sizeBytes) }}</span></td>
          <td class="col-source">
            <span class="mono">{{ r.sourceCommit?.slice(0, 7) ?? "—" }}</span>
            <span v-if="r.dirtySource" class="badge warn">uncommitted edits</span>
          </td>
          <td><span class="badge" :class="smokeTone[r.smoke]">{{ smokeText[r.smoke] }}</span></td>
          <td>
            <span v-if="r.filesRemoved" class="muted">Files removed</span>
            <span v-else-if="r.id === active" class="badge">Active</span>
            <span v-else-if="r.id === previous" class="muted">Previous</span>
            <span v-else class="muted">Built</span>
          </td>
          <td class="acts">
            <template v-if="!r.filesRemoved">
              <template v-if="r.smoke === 'failed'">
                <button v-if="r.smokeDetail" type="button" class="btn small" :aria-expanded="details.has(r.id)" @click="toggle(r.id)">Details</button>
                <button type="button" class="btn small destructive" :disabled="busy" @click="emit('delete', r)">Delete</button>
              </template>
              <template v-else>
                <button v-if="action(r)" type="button" class="btn small" :disabled="busy" @click="emit('action', r, action(r)!)">{{ ACTION_LABELS[action(r)!] }}</button>
                <button v-else-if="r.id === active" type="button" class="btn small" :disabled="exportBlocked" @click="emit('export', r)">Export…</button>
                <details class="menu">
                  <summary class="btn small icon" :aria-label="`More actions for ${r.version}`"><Ellipsis :size="16" :stroke-width="1.75" aria-hidden="true" /></summary>
                  <div class="pop">
                    <button type="button" @click="pick($event, () => emit('reveal', r))">Show in Files</button>
                    <button type="button" :disabled="exportBlocked" @click="pick($event, () => emit('export', r))">Export for other systems…</button>
                    <button v-if="r.smokeDetail" type="button" @click="pick($event, () => toggle(r.id))">Details</button>
                    <button v-if="r.id !== active" type="button" class="danger" @click="pick($event, () => emit('delete', r))">Delete</button>
                  </div>
                </details>
              </template>
            </template>
          </td>
        </tr>
        <tr v-if="openExports.has(r.id) && exportsOf(r) && !r.filesRemoved" class="exp">
          <td :id="`exports-${r.id}`" colspan="6">
            <template v-for="t in exportsOf(r)!.targets" :key="t.target">
              <section v-if="t.files.length" class="grp" :aria-labelledby="`exp-${r.id}-${t.target}`">
                <h3 :id="`exp-${r.id}-${t.target}`" class="gh">{{ heading(t) }} <span class="badge warn">Unverified</span></h3>
                <p class="caption muted">{{ TARGET_CAPTION[t.target] }}</p>
                <ul class="files">
                  <li v-for="f in t.files" :key="f.path" class="file">
                    <span class="fname mono" :title="f.name"><span class="head">{{ splitName(f.name).head }}</span><span class="tail">{{ splitName(f.name).tail }}</span></span>
                    <span class="purpose caption muted">{{ FILE_PURPOSE[f.kind] }}</span>
                    <span class="fsize caption tnum">{{ f.sizeBytes != null ? formatSize(f.sizeBytes) : "" }}</span>
                    <button
                      type="button" class="btn small copy" :aria-label="copied === f.path ? undefined : `Copy SHA-256 of ${f.name}`"
                      @click="copy(f)"
                    >{{ copied === f.path ? "Copied" : "Copy SHA-256" }}</button>
                  </li>
                </ul>
              </section>
              <p v-if="t.failed" class="failed caption">{{ TARGET_SHORT[t.target] }}: export failed · {{ exportTime(t.failed.at) }}</p>
            </template>
            <div class="exp-acts">
              <button type="button" class="btn small" @click="emit('revealDir', exportsOf(r)!.dir)">Show in Files</button>
              <button type="button" class="btn small" :disabled="exportBlocked" @click="emit('export', r)">Export again…</button>
            </div>
          </td>
        </tr>
        </template>
      </tbody>
    </table>
    <p class="sr-only" aria-live="polite">{{ announce }}</p>
    <button v-if="removed.length" type="button" class="link" :aria-expanded="showRemoved" @click="showRemoved = !showRemoved">
      {{ showRemoved ? "Hide" : "Show" }} {{ removed.length }} removed {{ removed.length === 1 ? "build" : "builds" }}
    </button>
  </div>
</template>

<style scoped>
.wrap { display: flex; flex-direction: column; gap: 8px; align-items: flex-start; }
table { width: 100%; border-collapse: collapse; }
th { text-align: left; font-size: 13px; font-weight: 400; color: var(--muted); padding: 4px 12px; border-bottom: 1px solid var(--border); }
td { padding: 6px 12px; border-bottom: 1px solid var(--border); vertical-align: middle; min-height: 48px; height: 48px; }
.gone { color: var(--muted); }
.ver { font-weight: 600; }
.caption { font-size: 13px; margin: 0; }
.notes { max-width: 40ch; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.crash { color: var(--danger); overflow-wrap: anywhere; }
.detail { white-space: pre-wrap; overflow-wrap: anywhere; font-size: 12px; background: var(--sidebar); padding: 6px 10px; border-radius: 8px; margin: 4px 0 0; }
.col-source { white-space: nowrap; }
.col-source .badge { margin-left: 6px; }
.narrow-only { display: none; }
.toggle { display: inline-flex; align-items: center; gap: 2px; min-height: 32px; text-align: left; }
tr.open > td { border-bottom: 0; }
.exp > td { background: var(--sidebar); height: auto; padding: 8px 12px 12px; }
.grp { margin: 0 0 12px; }
.gh { font-size: inherit; font-weight: 600; margin: 0; line-height: 1.5; }
.grp > .caption { margin: 0 0 4px; }
.files { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; }
.file { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 3fr) auto auto; gap: 4px 12px; align-items: center; min-height: 32px; }
.fname { display: flex; min-width: 0; white-space: nowrap; }
.fname .head { overflow: hidden; text-overflow: ellipsis; min-width: 0; flex: 0 1 auto; }
.fname .tail { flex: none; }
.fsize { text-align: right; }
.failed { margin: 0 0 12px; color: var(--danger); }
.exp-acts { display: flex; gap: 8px; flex-wrap: wrap; }
.acts { text-align: right; white-space: nowrap; }
.acts > * + * { margin-left: 8px; }
.menu { display: inline-block; position: relative; vertical-align: middle; }
.menu summary { list-style: none; cursor: pointer; display: inline-flex; padding: 2px 8px; }
.menu summary::-webkit-details-marker { display: none; }
.pop { position: absolute; right: 0; top: 100%; z-index: 5; min-width: 150px; background: var(--card); border: 1px solid var(--border-strong); border-radius: 8px; padding: 4px; display: flex; flex-direction: column; text-align: left; }
.pop button { font: inherit; background: none; border: 0; text-align: left; padding: 6px 10px; border-radius: 6px; cursor: pointer; color: inherit; min-height: 32px; }
.pop button:hover { background: var(--sidebar); }
.pop .danger { color: var(--danger); }
.link { font: inherit; background: none; border: 0; padding: 0; color: var(--accent-text); cursor: pointer; }
.link:hover { text-decoration: underline; }
/* M4-DR17: two-line rows below 900px; the built/source columns fold into a caption. */
@media (max-width: 900px) {
  .col-built, .col-source { display: none; }
  .narrow-only { display: block; }
  /* Two lines per file: name + size, then purpose + copy (design 13). */
  .file { grid-template-columns: minmax(0, 1fr) auto; grid-template-areas: "name size" "purpose copy"; }
  .fname { grid-area: name; }
  .fsize { grid-area: size; }
  .purpose { grid-area: purpose; }
  .copy { grid-area: copy; justify-self: end; }
}
</style>
