<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { openUrl } from "@tauri-apps/plugin-opener";
import { commands } from "../bindings";
import type { CommitInfo, ProjectSummary, RemoteCreated, SourceError, SyncRecord_Serialize } from "../bindings";
import { isActive, useRunsStore } from "../stores/runs";
import { useNavStore } from "../stores/nav";
import { useGithubStore } from "../stores/github";
import { RUNNING_LABEL, useSyncStore, type SyncAction } from "../stores/sync";
import { src, syncErrorDetails, syncErrorText, syncFix } from "../components/source/errors";
import SignInCode from "../components/github/SignInCode.vue";
import CommitList from "../components/github/CommitList.vue";
import CreateRepoDialog from "../components/github/CreateRepoDialog.vue";
import { relTime } from "../components/github/time";

const props = defineProps<{ project: ProjectSummary }>();
const pid = props.project.id;
const nav = useNavStore();
const gh = useGithubStore();
const sync = useSyncStore();
const runs = useRunsStore();

type Btn = { id: string; label: string; primary?: boolean; disabled?: boolean; why?: string; run: () => void };

const loaded = ref(false);
const outgoing = ref<CommitInfo[]>([]);
const incoming = ref<CommitInfo[]>([]);
const lastSync = ref<SyncRecord_Serialize | null>(null);
const problem = ref<{ error: SourceError; retry: () => void } | null>(null);
const announce = ref("");
const showCreate = ref(false);
/** Visibility is only known for a repository created in this session (RemoteInfo has no such field). */
const created = ref<RemoteCreated | null>(null);
const now = ref(Date.now());
const tick = setInterval(() => (now.value = Date.now()), 1000);

const st = computed(() => sync.status[pid] ?? null);
const stFail = computed(() => sync.failure[pid] ?? null);
const remote = computed(() => st.value?.remote ?? null);
const run = computed(() => sync.running[pid] ?? null);
const liveRun = computed(() => (runs.sessions[pid] ?? []).some((s) => s.mode === "changes" && isActive(s)));
const dirty = computed(() => (st.value?.entries.length ?? 0) > 0);
const ahead = computed(() => remote.value?.ahead ?? 0);
const behind = computed(() => remote.value?.behind ?? 0);
const plural = (n: number) => `${n} ${n === 1 ? "commit" : "commits"}`;
const repoUrl = computed(() => (remote.value?.github ? `https://github.com/${remote.value.github.owner}/${remote.value.github.name}` : null));
const login = computed(() => gh.login());
const elapsed = computed(() => (run.value ? Math.max(0, Math.round((now.value - run.value.startedAt) / 1000)) : 0));

const blockWhy = computed(() => (liveRun.value ? "Wait for the Make-changes run / release build to finish." : dirty.value ? "Commit or restore your changes first." : ""));

async function refreshAll() {
  await sync.refresh(pid);
  const r = remote.value;
  if (r?.upstream && (r.ahead || r.behind)) {
    const c = await src(commands.sourceSyncCommits(pid));
    outgoing.value = c.ok ? c.data.outgoing : [];
    incoming.value = c.ok ? c.data.incoming : [];
  } else outgoing.value = incoming.value = [];
  const l = await src(commands.sourceLastSync(pid));
  lastSync.value = l.ok ? l.data : null;
}

onMounted(async () => {
  window.addEventListener("focus", refreshAll);
  await Promise.all([refreshAll(), gh.check()]); // M5-D15: check when the tab opens
  loaded.value = true;
});
onUnmounted(() => {
  clearInterval(tick);
  window.removeEventListener("focus", refreshAll);
});

async function act(op: SyncAction) {
  problem.value = null;
  const r = await sync.run(pid, op);
  if (!r.ok) {
    problem.value = { error: r.error, retry: () => void act(op) };
    if (r.error.kind === "authRequired") void gh.check(); // M5-D15: after an auth failure
  } else {
    const n = r.data?.commits ?? 0;
    const o = r.data;
    announce.value =
      op === "abort" ? "Merge aborted"
      : op === "fetch" ? (n ? `GitHub has ${plural(n)} you don't have` : "Nothing new on GitHub")
      : o?.conflicted ? "Merge stopped: some files need your attention"
      : `${op === "push" ? "Pushed" : op === "pull" ? "Pulled" : "Merged"} ${plural(n)}`;
  }
  await refreshAll();
}

async function signIn() {
  await gh.signIn();
  await gh.check(); // M5-D15: after sign-in
  problem.value = null;
}
const stop = () => void sync.stop(pid);

function onCreated(r: RemoteCreated) {
  showCreate.value = false;
  created.value = r;
  announce.value = `Created ${r.nameWithOwner}`;
  void refreshAll();
}
async function onAuthLost() {
  showCreate.value = false;
  await gh.check();
}

const open = (u: string | null) => u && openUrl(u).catch(() => {});

/** M5-DR3: the state table. One sentence, one primary action. */
const row = computed<{ key: string; sentence: string; caption?: string; buttons: Btn[]; details?: string } | null>(() => {
  if (!loaded.value) return null;
  const a = gh.auth;
  const r = remote.value;
  const needsGh = !r || !!r.github;
  if (needsGh && a?.kind === "ghMissing") return { key: "gh-missing", sentence: "GitHub CLI is not installed", buttons: [{ id: "doctor", label: "Open Doctor", primary: true, run: () => nav.go("doctor") }] };
  if (needsGh && (a?.kind === "signedOut" || a?.kind === "tokenInvalid")) {
    return { key: "signed-out", sentence: "Sign in to GitHub", caption: "Studio uses your GitHub CLI login and stores no token", buttons: [{ id: "signin", label: "Sign in", primary: true, run: () => void signIn() }] };
  }
  if (needsGh && (a?.kind === "unreachable" || gh.error?.kind === "unreachable")) {
    const m = a?.kind === "unreachable" ? a.message : gh.error?.kind === "unreachable" ? gh.error.message : "";
    return { key: "offline", sentence: "GitHub can't be reached", buttons: [{ id: "recheck", label: "Check again", primary: true, run: () => void gh.check() }], details: m };
  }
  const f = stFail.value;
  const toSource: Btn = { id: "source", label: "Open Source tab", primary: true, run: () => (nav.tab = "source") };
  if (f?.kind === "notARepo" || st.value?.head.kind === "unborn") {
    return { key: "no-git", sentence: f ? "This project isn't using Git yet" : "Commit first", caption: "GitHub needs at least one commit to publish.", buttons: [toSource] };
  }
  if (f?.kind === "alreadyInRepo" || (st.value?.location.isSubfolder && !r)) {
    const top = f?.kind === "alreadyInRepo" ? f.top : st.value?.location.top;
    return { key: "subfolder", sentence: "This project is inside a larger repository", caption: `Create the GitHub repository at ${top} instead.`, buttons: [] };
  }
  if (f || !st.value) return { key: "status-failed", sentence: "Couldn't read the repository", buttons: [{ id: "retry", label: "Try again", primary: true, run: () => void refreshAll() }], details: f ? syncErrorDetails(f) : "" };
  if (st.value.merging) {
    return { key: "merging", sentence: "Merge in progress", buttons: [toSource, { id: "abort", label: "Abort merge", run: () => void act("abort") }] };
  }
  if (!r) return { key: "no-remote", sentence: "Not on GitHub yet", buttons: [{ id: "create", label: "Create GitHub repository…", primary: true, run: () => (showCreate.value = true) }] };

  const head = st.value.head.kind === "branch" ? st.value.head.name : "";
  const bits = [r.github ? `${r.github.owner}/${r.github.name}` : r.url];
  if (created.value && r.github && created.value.nameWithOwner === `${r.github.owner}/${r.github.name}`) bits.push(created.value.visibility);
  if (head) bits.push(`branch ${head}`);
  bits.push(r.fetchedAt ? `fetched ${relTime(Date.parse(r.fetchedAt))}` : "never fetched");
  const caption = bits.join(" · ");
  const fetch: Btn = { id: "fetch", label: "Fetch", run: () => void act("fetch") };
  const why = blockWhy.value;
  const openGh: Btn[] = repoUrl.value ? [{ id: "open", label: "Open on GitHub", run: () => void open(repoUrl.value) }] : [];

  if (!r.upstream) return { key: "never-pushed", sentence: head ? `Branch ${head} is not on GitHub yet` : "This branch is not on GitHub yet", caption, buttons: [{ id: "push", label: "Push branch", primary: true, run: () => void act("push") }, fetch] };
  if (r.ahead && r.behind) return { key: "diverged", sentence: "Your project and GitHub both have new commits", caption, buttons: [{ id: "merge", label: "Merge GitHub's changes", primary: true, disabled: !!why, why, run: () => void act("merge") }, fetch] };
  if (r.behind) return { key: "behind", sentence: `GitHub has ${plural(r.behind)} you don't have`, caption, buttons: [{ id: "pull", label: `Pull ${plural(r.behind)}`, primary: true, disabled: !!why, why, run: () => void act("pull") }, fetch] };
  if (r.ahead) return { key: "ahead", sentence: `${plural(r.ahead)} to push to GitHub`, caption, buttons: [{ id: "push", label: `Push ${plural(r.ahead)}`, primary: true, run: () => void act("push") }, fetch] };
  return { key: "synced", sentence: "Up to date with GitHub", caption, buttons: [{ ...fetch, primary: true }, ...openGh] };
});

/** While an operation runs, Stop is the only enabled button. */
const buttons = computed<Btn[]>(() => {
  const r = row.value;
  if (!r) return [];
  if (!run.value) return r.buttons;
  return [{ id: "stop", label: "Stop", primary: true, run: stop }, ...r.buttons.map((b) => ({ ...b, primary: false, disabled: true, why: undefined }))];
});
const sentence = computed(() => (run.value ? `${RUNNING_LABEL[run.value.op]} ${elapsed.value} s` : row.value?.sentence ?? ""));
const whyText = computed(() => buttons.value.find((b) => b.why)?.why ?? "");

const fixLabel: Record<string, string> = { signIn: "Sign in", pull: "Pull", merge: "Merge GitHub's changes", retry: "Try again", openSource: "Open Source tab" };
function fix() {
  const p = problem.value;
  if (!p) return;
  switch (syncFix(p.error)) {
    case "signIn": return void signIn();
    case "pull": return void act("pull");
    case "merge": return void act("merge");
    case "retry": return p.retry();
    case "openSource": nav.tab = "source";
  }
}

watch(() => gh.auth?.kind, (k, was) => k === "signedIn" && was && void refreshAll());
const lastLine = computed(() => (lastSync.value ? `Last sync: ${lastSync.value.operation}${lastSync.value.ok ? "" : " failed"} · ${relTime(Date.parse(lastSync.value.finishedAt))}` : ""));
</script>

<template>
  <div class="gh">
    <p v-if="!loaded" class="muted">Loading…</p>

    <section v-else-if="gh.signingIn" class="status"><SignInCode /></section>

    <template v-else-if="row">
      <section class="status" aria-label="GitHub sync">
        <div class="row">
          <div class="text">
            <p class="sentence">{{ sentence }}</p>
            <p v-if="row.caption" class="caption muted">{{ row.caption }}</p>
          </div>
          <div class="buttons">
            <button
              v-for="b in buttons" :key="b.id" type="button" class="btn" :class="{ primary: b.primary }" :disabled="b.disabled"
              :aria-describedby="b.why ? 'gh-why' : undefined" @click="b.run()"
            >{{ b.label }}</button>
          </div>
        </div>
        <p v-if="whyText" id="gh-why" class="caption muted">{{ whyText }}</p>
        <details v-if="row.details" class="details"><summary>Details</summary><pre class="mono">{{ row.details }}</pre></details>
      </section>

      <div v-if="problem" class="callout danger" role="alert">
        <p class="reason">{{ syncErrorText(problem.error) }}</p>
        <button v-if="syncFix(problem.error)" type="button" class="btn small" @click="fix">{{ fixLabel[syncFix(problem.error)!] }}</button>
        <details v-if="syncErrorDetails(problem.error)" class="details"><summary>Details</summary><pre class="mono">{{ syncErrorDetails(problem.error) }}</pre></details>
      </div>

      <CommitList v-if="ahead && remote?.upstream && !st?.merging" heading="Not on GitHub yet" :commits="outgoing" />
      <CommitList v-if="behind && remote?.upstream && !st?.merging" heading="Incoming from GitHub" :commits="incoming" />

      <p v-if="lastLine" class="caption muted">{{ lastLine }}</p>
      <p v-if="login" class="caption muted foot">
        Signed in as {{ login }}
        <button v-if="repoUrl && row.key !== 'synced'" type="button" class="link" @click="open(repoUrl)">Open on GitHub</button>
      </p>
    </template>

    <p class="sr-only" role="status" aria-live="polite">{{ announce }}</p>

    <CreateRepoDialog v-if="showCreate" :project-id="pid" :default-name="project.slug" @close="showCreate = false" @created="onCreated" @auth-lost="onAuthLost" />
  </div>
</template>

<style scoped>
.gh { display: flex; flex-direction: column; gap: 12px; flex: 1; min-height: 0; overflow: auto; }
.status { display: flex; flex-direction: column; gap: 6px; padding: 12px 0; border-bottom: 1px solid var(--border); }
.row { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
.text { flex: 1 1 280px; min-width: 0; }
.sentence { margin: 0; font-weight: 600; }
.caption { margin: 0; font-size: 13px; }
.buttons { display: flex; gap: 8px; flex-wrap: wrap; }
.callout { margin: 0; display: flex; flex-direction: column; gap: 6px; align-items: flex-start; }
.reason { margin: 0; }
.details pre { margin: 4px 0 0; white-space: pre-wrap; font-size: 12px; }
.link { font: inherit; background: none; border: 0; padding: 0 0 0 8px; color: var(--accent-text); cursor: pointer; }
.link:hover { text-decoration: underline; }
@media (max-width: 899px) {
  .text { flex-basis: 100%; }
  .buttons { width: 100%; }
}
</style>
