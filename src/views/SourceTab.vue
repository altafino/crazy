<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { RefreshCw } from "lucide-vue-next";
import { commands } from "../bindings";
import type { Outcome } from "../lib/call";
import type { CannotUndo, FileDiff, CommitInfo, FileChange, Identity, LostReason, ProjectSummary, RepoStatus, SecretFinding, SourceError } from "../bindings";
import { isActive, useRunsStore } from "../stores/runs";
import { useNavStore } from "../stores/nav";
import { useToastStore } from "../stores/toast";
import BranchControl from "../components/source/BranchControl.vue";
import IdentityForm from "../components/source/IdentityForm.vue";
import SourceFileList from "../components/source/SourceFileList.vue";
import DiffView from "../components/changes/DiffView.vue";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import { sourceErrorText, src } from "../components/source/errors";

const props = defineProps<{ project: ProjectSummary }>();
const pid = props.project.id;
const runs = useRunsStore();
const nav = useNavStore();
const toast = useToastStore();

const status = ref<RepoStatus | null>(null);
/** Why there is no status: a typed error that selects a secondary state. */
const failure = ref<SourceError | null>(null);
const ticked = ref<string[]>([]);
const active = ref<string | null>(null);
const message = ref("");
const busy = ref(false);
/** Result of the last commit/init/restore attempt, shown near the commit box. */
const problem = ref<SourceError | null>(null);
const needIdentity = ref(false);
const identityPrefill = ref<Partial<Identity>>({});
const secrets = ref<SecretFinding[]>([]);
const initSecrets = ref<string[]>([]);
const restoreAsk = ref<CannotUndo[] | null>(null);
let retry: (() => void) | null = null;
let seq = 0;
const rev = ref(0);

const entries = computed(() => status.value?.entries ?? []);
const tickedSet = computed(() => new Set(ticked.value));
const liveRun = computed(() => (runs.sessions[pid] ?? []).some((s) => s.mode === "changes" && isActive(s)));
const lockReason = "Claude is changing files. This unlocks when the run ends.";
const stagedOutside = computed(() => entries.value.filter((e) => e.staged && !tickedSet.value.has(e.path)).length);
const activeEntry = computed(() => entries.value.find((e) => e.path === active.value) ?? null);
const diffFile = computed<FileChange | null>(() => {
  const e = activeEntry.value;
  return e && { path: e.path, oldPath: e.oldPath, status: e.kind === "deleted" ? "deleted" : e.kind === "renamed" ? "renamed" : "modified", additions: e.additions ?? 0, deletions: e.deletions ?? 0, skippedReason: null };
});
const count = computed(() => ticked.value.length);
const plural = (n: number) => `${n} ${n === 1 ? "file" : "files"}`;
const conflicts = computed(() => status.value?.conflicts ?? 0);
const merging = computed(() => status.value?.merging ?? false);
const branchName = computed(() => (status.value?.head.kind === "branch" ? status.value.head.name : "this branch"));
const mergeMessage = computed(() => `Merge GitHub's changes into ${branchName.value}`);
const conflictPaths = computed(() => entries.value.filter((e) => e.kind === "conflicted").map((e) => e.path));
const ahead = computed(() => status.value?.remote?.ahead ?? 0);
/** A merge commit takes the whole merge, so it needs no ticked files (M5-DR7). */
const canCommit = computed(() => !busy.value && !conflicts.value && !!message.value.trim() && (merging.value || count.value > 0));
const commitWhy = computed(() => (conflicts.value ? `Resolve ${conflicts.value} ${conflicts.value === 1 ? "conflict" : "conflicts"} first` : ""));

async function refresh() {
  const mine = ++seq;
  const r = await src(commands.sourceStatus(pid, ticked.value));
  if (mine !== seq) return; // stale
  if (!r.ok) {
    failure.value = r.error;
    status.value = null;
    return;
  }
  failure.value = null;
  status.value = r.data;
  rev.value++;
  const have = new Set(r.data.entries.map((e) => e.path));
  if (ticked.value.some((p) => !have.has(p))) ticked.value = ticked.value.filter((p) => have.has(p));
  if (!active.value || !have.has(active.value)) active.value = r.data.entries.find((e) => e.kind === "conflicted")?.path ?? r.data.entries[0]?.path ?? null;
  if (!r.data.entries.length && !r.data.merging) message.value = "";
  if (r.data.merging && !message.value.trim()) message.value = mergeMessage.value;
  void loadHistory();
}

/** The Chat tab's "Commit…" handoff: tick exactly those files and draft the message, then clear it. */
async function applyHandoff() {
  const h = nav.commitHandoff;
  if (!h || h.projectId !== pid) return;
  nav.commitHandoff = null;
  await refresh();
  const have = new Set(entries.value.map((e) => e.path));
  ticked.value = h.paths.filter((p) => have.has(p));
  message.value = h.message;
  if (ticked.value[0]) active.value = ticked.value[0];
}

// Refresh on open, on focus, after a run ends and on the button (M3-D21); never on a timer.
onMounted(async () => {
  window.addEventListener("focus", refresh);
  await refresh();
  await applyHandoff();
});
onUnmounted(() => window.removeEventListener("focus", refresh));
watch(liveRun, (now, before) => before && !now && void refresh());
watch(() => nav.commitHandoff, () => void applyHandoff());

// The secret warning only warns (M3-D17).
watch(ticked, async (paths) => {
  problem.value = null;
  needIdentity.value = false;
  if (!paths.length) {
    secrets.value = [];
    return;
  }
  const r = await src(commands.sourceSecretCheck(pid, paths));
  if (paths === ticked.value) secrets.value = r.ok ? r.data : [];
});

async function loadDiff(full: boolean): Promise<Outcome<FileDiff>> {
  const e = activeEntry.value!;
  const r = await src(commands.sourceFileDiff(pid, e.path, e.oldPath ?? null, full));
  return r.ok ? r : { ok: false, error: { message: sourceErrorText(r.error), fieldErrors: [] } };
}

async function commit(identity?: Identity) {
  if (!canCommit.value) return;
  busy.value = true;
  problem.value = null;
  retry = () => void commit();
  try {
    if (identity) {
      const s = await src(commands.sourceSetIdentity(pid, identity));
      if (!s.ok) return void (problem.value = s.error);
    }
    const r = await src(commands.sourceCommit(pid, message.value, ticked.value));
    if (!r.ok) {
      if (r.error.kind === "identityMissing") {
        const cur = await src(commands.sourceGetIdentity(pid));
        identityPrefill.value = cur.ok ? { name: cur.data.name ?? "", email: cur.data.email ?? "" } : {};
        needIdentity.value = true;
      } else problem.value = r.error;
      return;
    }
    needIdentity.value = false;
    const draft = nav.buildDraft?.projectId === pid ? nav.buildDraft : null;
    if (draft) toast.show(`Committed. Build ${draft.version}?`, { label: "Open build dialog", run: () => nav.openBuildDialog(pid) });
    else toast.show(merging.value ? "Merge committed" : `Committed ${plural(r.data.files)}`);
    message.value = "";
    ticked.value = [];
    await refresh();
  } finally {
    busy.value = false;
  }
}

const markAsk = ref<string | null>(null);
const resolved = ref("");
async function markResolved(path: string, force = false) {
  markAsk.value = null;
  problem.value = null;
  const r = await src(commands.sourceMarkResolved(pid, path, force));
  if (r.ok) {
    resolved.value = `${path} marked resolved`;
    await refresh();
  } else if (r.error.kind === "markersRemain") markAsk.value = path;
  else problem.value = r.error;
}
async function abortMerge() {
  problem.value = null;
  const r = await src(commands.sourceAbortMerge(pid));
  if (!r.ok) return void (problem.value = r.error);
  message.value = "";
  toast.show("Merge aborted");
  await refresh();
}
function askClaudeResolve() {
  const list = conflictPaths.value.map((p) => `- ${p}`).join("\n");
  nav.askClaude({
    projectId: pid,
    text: `Resolve the merge conflicts in these files. Keep what both sides intended and remove every conflict marker (<<<<<<<, =======, >>>>>>>):\n${list}`,
  });
}

async function initGit(identity?: Identity, confirmSecrets = false) {
  busy.value = true;
  problem.value = null;
  const r = await src(commands.sourceInitGit(pid, { identity: identity ?? null, confirmSecrets }));
  busy.value = false;
  if (r.ok) {
    needIdentity.value = false;
    initSecrets.value = [];
    toast.show("Git is set up for this project");
    await refresh();
  } else if (r.error.kind === "identityMissing") {
    needIdentity.value = true;
  } else if (r.error.kind === "secretsFound") {
    initSecrets.value = r.error.paths;
  } else problem.value = r.error;
}
const initWithIdentity = (i: Identity) => initGit(i, initSecrets.value.length > 0);

async function askRestore() {
  problem.value = null;
  const r = await src(commands.sourceRestorePreview(pid, ticked.value));
  if (r.ok) restoreAsk.value = r.data;
  else problem.value = r.error;
}
async function restore() {
  restoreAsk.value = null;
  const r = await src(commands.sourceRestore(pid, ticked.value));
  if (!r.ok) return void (problem.value = r.error);
  ticked.value = [];
  toast.show(`Restored ${plural(r.data.restored.length)}`, { label: "Undo", run: () => void undoRestore() });
  await refresh();
}
async function undoRestore() {
  const r = await src(commands.sourceUndoRestore(pid));
  if (r.ok) toast.show("Restore undone");
  else problem.value = r.error;
  await refresh();
}
const LOST: Record<LostReason, string> = { secret: "looks like a secret", tooLarge: "too large to keep a copy", stagedState: "its staged state won't come back" };

// History: collapsed by default, remembered per project (M3-D35).
const histKey = `source.history.${pid}`;
const histOpen = ref(false);
try { histOpen.value = localStorage.getItem(histKey) === "open"; } catch { /* storage may be unavailable */ }
const history = ref<CommitInfo[]>([]);
async function loadHistory() {
  if (!histOpen.value || !status.value) return;
  const r = await src(commands.sourceHistory(pid));
  if (r.ok) history.value = r.data.slice(0, 50);
}
function toggleHistory() {
  histOpen.value = !histOpen.value;
  try { localStorage.setItem(histKey, histOpen.value ? "open" : "closed"); } catch { /* ignore */ }
  void loadHistory();
}
const ago = (t: number) => new Date(t * 1000).toLocaleString();

const blockedWhy = computed(() => (liveRun.value ? lockReason : ""));
const loading = computed(() => !status.value && !failure.value);
const headNote = computed(() => {
  const h = status.value?.head;
  if (h?.kind === "unborn") return `No commits yet. Your first commit creates the branch ${h.name}.`;
  if (h?.kind === "detached") return `You aren't on a branch (detached at ${h.short}). Create a branch to keep new commits.`;
  return "";
});
</script>

<template>
  <div class="source">
    <p v-if="loading" class="muted">Loading…</p>

    <!-- Secondary states without a status -->
    <div v-else-if="failure?.kind === 'notARepo'" class="state">
      <h2>This project isn't using Git yet</h2>
      <p class="muted">Git keeps a history of your project, so you can save versions and go back to them.</p>
      <div v-if="initSecrets.length" class="callout warn" role="alert">
        These files look like secrets and would be saved in the history:
        <ul class="mono"><li v-for="p in initSecrets" :key="p">{{ p }}</li></ul>
        <button type="button" class="btn" :disabled="busy" @click="initGit(undefined, true)">Initialize anyway</button>
      </div>
      <IdentityForm v-if="needIdentity" :busy="busy" submit-label="Save and initialize Git" @submit="initWithIdentity" />
      <p v-if="problem" class="callout danger" role="alert">{{ sourceErrorText(problem) }}</p>
      <button type="button" class="btn primary" :disabled="busy || liveRun" @click="initGit()">Initialize Git</button>
      <p v-if="liveRun" class="muted">{{ lockReason }}</p>
    </div>
    <div v-else-if="failure?.kind === 'alreadyInRepo'" class="state">
      <h2>This folder is already part of a Git repository</h2>
      <p class="muted mono">{{ failure.top }}</p>
      <p class="muted">Studio can't start a second repository inside it. Open the project at that location instead.</p>
    </div>
    <div v-else-if="failure" class="state" role="alert">
      <h2>{{ failure.kind === "statusTimedOut" ? "Git is taking too long" : "Couldn't read the repository" }}</h2>
      <p v-if="failure.kind === 'repoLocked'">Another Git program is using this repository.</p>
      <p v-else class="muted">{{ sourceErrorText(failure) }}</p>
      <button type="button" class="btn" @click="refresh">Retry</button>
    </div>

    <template v-else-if="status">
      <div class="bar">
        <BranchControl :project-id="pid" :head="status.head" :locked="liveRun" :lock-reason="lockReason" @changed="refresh" />
        <span class="spacer" />
        <button type="button" class="btn small" @click="refresh"><RefreshCw :size="14" aria-hidden="true" /> Refresh</button>
      </div>
      <p v-if="headNote" class="muted note">{{ headNote }}</p>

      <div v-if="merging" class="callout warn merge" role="status">
        <span>Merging GitHub's changes into {{ branchName }}. {{ conflicts ? `${plural(conflicts)} ${conflicts === 1 ? "has" : "have"} conflicts.` : "No conflicts left." }}</span>
        <button v-if="conflicts" type="button" class="link" @click="askClaudeResolve">Ask Claude to resolve</button>
        <button type="button" class="btn small" @click="abortMerge">Abort merge</button>
      </div>
      <p class="sr-only" aria-live="polite">{{ resolved }}</p>

      <div v-if="!entries.length && !merging" class="state" role="status">
        <h2>Nothing to commit</h2>
        <p v-if="status.lastCommit" class="muted">
          Last commit: <strong>{{ status.lastCommit.subject }}</strong> <span class="mono">{{ status.lastCommit.short }}</span>, {{ ago(status.lastCommit.time) }}.
        </p>
        <p v-if="ahead > 0" class="muted">
          {{ ahead }} {{ ahead === 1 ? "commit" : "commits" }} not on GitHub yet ·
          <button type="button" class="link" @click="nav.tab = 'github'">Open GitHub tab</button>
        </p>
        <button v-if="status.undoRestoreAvailable" type="button" class="btn" :disabled="liveRun" :title="blockedWhy" @click="undoRestore">Undo last restore</button>
      </div>

      <div v-if="entries.length || merging" class="split">
        <div class="left">
          <div class="card list"><SourceFileList v-model:ticked="ticked" :entries="entries" :active="active" :total="status.total" :truncated="status.truncated" :merging="merging" @select="active = $event" @resolve="markResolved($event)" /></div>
          <form class="commit" @submit.prevent="commit()">
            <label for="commit-msg">Commit message</label>
            <textarea id="commit-msg" v-model="message" class="input" rows="3" @keydown.ctrl.enter.prevent="commit()" />
            <p v-if="stagedOutside" class="muted note">{{ plural(stagedOutside) }} staged outside your selection stay staged and are not committed.</p>
            <p v-if="conflicts && !merging" class="callout warn" role="alert">{{ plural(conflicts) }} with conflicts. Resolve them in your editor, then Refresh. Commit is blocked until then.</p>
            <div v-if="secrets.length" class="callout warn" role="status">
              Possible secrets in the selected files. You can still commit.
              <ul class="mono"><li v-for="s in secrets" :key="s.path + s.line">{{ s.path }}{{ s.line ? `:${s.line}` : "" }} ({{ s.kind }}{{ s.masked ? `, ${s.masked}` : "" }})</li></ul>
            </div>
            <IdentityForm v-if="needIdentity" :initial="identityPrefill" :busy="busy" submit-label="Save and commit" @submit="commit($event)" />
            <div class="errs" aria-live="assertive">
              <div v-if="problem?.kind === 'hookRejected'" class="callout danger">
                A Git hook stopped the commit.<pre class="mono">{{ problem.output }}</pre>
              </div>
              <div v-else-if="problem?.kind === 'repoLocked'" class="callout danger">
                Another Git program is using this repository. <button type="button" class="btn small" @click="retry?.()">Retry</button>
              </div>
              <p v-else-if="problem" class="callout danger">{{ sourceErrorText(problem) }}</p>
            </div>
            <div class="actions">
              <button v-if="!merging" type="button" class="btn destructive small" :disabled="!count || liveRun" :title="blockedWhy" @click="askRestore">Restore…</button>
              <button v-if="status.undoRestoreAvailable" type="button" class="btn small" :disabled="liveRun" :title="blockedWhy" @click="undoRestore">Undo last restore</button>
              <span class="spacer" />
              <button type="submit" class="btn primary" :disabled="!canCommit" :aria-describedby="commitWhy ? 'commit-why' : undefined">{{ merging ? "Commit merge" : `Commit ${plural(count)}` }}</button>
            </div>
            <p v-if="commitWhy" id="commit-why" class="muted note">{{ commitWhy }}</p>
            <p v-if="liveRun" class="muted note">{{ lockReason }}</p>
          </form>
        </div>
        <div class="card diff"><DiffView v-if="diffFile" :key="diffFile.path" :session-id="pid" :file="diffFile" :loader="loadDiff" :reload-key="String(rev)" /></div>
      </div>

      <section class="history">
        <button type="button" class="btn small" :aria-expanded="histOpen" @click="toggleHistory">History</button>
        <ul v-if="histOpen" class="hist">
          <li v-for="c in history" :key="c.hash"><span class="mono">{{ c.short }}</span> {{ c.subject }} <span class="muted">{{ c.author }}, {{ ago(c.time) }}</span></li>
          <li v-if="!history.length" class="muted">No commits yet.</li>
        </ul>
      </section>
    </template>

    <ConfirmDialog v-if="markAsk" title="Mark resolved anyway?" confirm-label="Mark resolved" @cancel="markAsk = null" @confirm="markResolved(markAsk!, true)">
      <p><span class="mono">{{ markAsk }}</span> still has conflict markers. Mark it resolved anyway?</p>
    </ConfirmDialog>

    <ConfirmDialog v-if="restoreAsk" title="Restore files?" confirm-label="Restore" @cancel="restoreAsk = null" @confirm="restore">
      <p>Your changes to {{ plural(count) }} will be replaced with the last committed version. You can undo this afterwards, except for:</p>
      <ul v-if="restoreAsk.length" class="mono">
        <li v-for="c in restoreAsk" :key="c.path">{{ c.path }} <span class="muted">({{ LOST[c.reason] }})</span></li>
      </ul>
      <p v-else class="muted">Everything here can be undone.</p>
    </ConfirmDialog>
  </div>
</template>

<style scoped>
.source { display: flex; flex-direction: column; gap: 12px; flex: 1; min-height: 0; }
.bar { display: flex; align-items: flex-start; gap: 12px; }
.spacer { flex: 1; }
.merge { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; margin: 0; }
.merge > span { flex: 1; }
.link { background: none; border: 0; padding: 0; color: inherit; text-decoration: underline; cursor: pointer; font: inherit; }
.note { margin: 0; font-size: 0.9em; }
.state { padding: var(--space) 0; display: flex; flex-direction: column; gap: 8px; align-items: flex-start; }
.state h2 { margin: 0; font-size: 1.1em; }
.state p { margin: 0; }
.split { display: grid; grid-template-columns: 380px minmax(0, 1fr); gap: 12px; flex: 1; min-height: 440px; } /* below 440px the page scrolls instead of the file list shrinking to a few rows */
.left { display: flex; flex-direction: column; gap: 12px; min-height: 0; }
.list { flex: 1; min-height: 120px; }
.card { border: 1px solid var(--border); border-radius: var(--radius); background: var(--card); min-height: 0; overflow: hidden; }
.commit { display: flex; flex-direction: column; gap: 6px; }
.commit .callout { margin: 0; }
.commit ul, .commit pre { margin: 4px 0 0; padding-left: 18px; white-space: pre-wrap; }
.commit pre { padding: 0; max-height: 160px; overflow: auto; }
.actions { display: flex; gap: 8px; align-items: center; }
.history { display: flex; flex-direction: column; gap: 6px; align-items: flex-start; }
.hist { list-style: none; margin: 0; padding: 0; max-height: 220px; overflow: auto; width: 100%; }
@media (max-width: 899px) {
  /* Stacked: natural height and the page scrolls, so the commit form never overlaps the diff. */
  .split { flex: none; min-height: 0; grid-template-columns: minmax(0, 1fr); grid-template-rows: auto 320px; }
  .left { max-height: none; }
  .list { flex: none; max-height: 40vh; }
}
@media (max-height: 800px) {
  .commit textarea { height: calc(2lh + 14px); }
}
</style>
