<script setup lang="ts">
import { computed, ref } from "vue";
import { Ellipsis } from "lucide-vue-next";
import type { ReleaseView } from "../../bindings";
import { formatSize } from "../../lib/format";
import { ACTION_LABELS, activeId as activeOf, firstLine, kept, previousId, rowAction } from "./helpers";
import type { RowAction } from "./helpers";
import type { InstallState } from "../../bindings";

const props = defineProps<{ releases: ReleaseView[]; install: InstallState | null; busy?: boolean }>();
const emit = defineEmits<{ action: [r: ReleaseView, a: Exclude<RowAction, null>]; delete: [r: ReleaseView]; reveal: [r: ReleaseView] }>();

const showRemoved = ref(false);
const details = ref(new Set<string>());
const active = computed(() => activeOf(props.install));
const installed = computed(() => props.install?.kind === "installed");
const activeRow = computed(() => props.releases.find((r) => r.id === active.value) ?? null);
const previous = computed(() => previousId(props.releases, active.value));
const removed = computed(() => props.releases.filter((r) => r.filesRemoved));
const rows = computed(() => [...kept(props.releases), ...(showRemoved.value ? removed.value : [])]);

const when = (iso: string) => new Date(iso).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
const action = (r: ReleaseView) => rowAction(r, active.value, installed.value, activeRow.value?.createdAt ?? null);
const toggle = (id: string) => (details.value.has(id) ? details.value.delete(id) : details.value.add(id));
const smokeText = { passed: "Passed", failed: "Failed", unverified: "Unverified" } as const;
const smokeTone = { passed: "ok", failed: "danger", unverified: "warn" } as const;

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
        <tr v-for="r in rows" :key="r.id" :class="{ gone: r.filesRemoved }" :data-release="r.version">
          <td>
            <span class="mono ver">{{ r.version }}</span>
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
                <details class="menu">
                  <summary class="btn small icon" :aria-label="`More actions for ${r.version}`"><Ellipsis :size="16" :stroke-width="1.75" aria-hidden="true" /></summary>
                  <div class="pop">
                    <button type="button" @click="pick($event, () => emit('reveal', r))">Show in Files</button>
                    <button v-if="r.smokeDetail" type="button" @click="pick($event, () => toggle(r.id))">Details</button>
                    <button v-if="r.id !== active" type="button" class="danger" @click="pick($event, () => emit('delete', r))">Delete</button>
                  </div>
                </details>
              </template>
            </template>
          </td>
        </tr>
      </tbody>
    </table>
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
}
</style>
