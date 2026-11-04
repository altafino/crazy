import { ref } from "vue";
import { defineStore } from "pinia";
import { commands } from "../bindings";
import type { RepoStatus, SourceError, SyncOutcome } from "../bindings";
import { src, type Res } from "../components/source/errors";

export type SyncAction = "fetch" | "pull" | "merge" | "push" | "abort";
export const RUNNING_LABEL: Record<SyncAction, string> = { fetch: "Fetching…", pull: "Pulling…", merge: "Merging…", push: "Pushing…", abort: "Aborting…" };

/** Per-project GitHub sync state, shared by the GitHub tab and the tab-label count (M5-DR1). */
export const useSyncStore = defineStore("sync", () => {
  const status = ref<Record<string, RepoStatus | null>>({});
  const failure = ref<Record<string, SourceError | null>>({});
  const running = ref<Record<string, { op: SyncAction; startedAt: number } | null>>({});

  async function refresh(pid: string) {
    try {
      const r = await src(commands.sourceStatus(pid, []));
      status.value[pid] = r.ok ? r.data : null;
      failure.value[pid] = r.ok ? null : r.error;
    } catch { /* no status available; the tab count just stays hidden */ }
  }

  /** Runs one sync action with the running state; refreshes the status afterwards. */
  async function run(pid: string, op: SyncAction): Promise<Res<SyncOutcome | null>> {
    if (running.value[pid]) return { ok: false, error: { kind: "syncRunning" } };
    running.value[pid] = { op, startedAt: Date.now() };
    try {
      const c = commands;
      return await (op === "fetch" ? src(c.sourceFetch(pid)) : op === "pull" ? src(c.sourcePull(pid))
        : op === "merge" ? src(c.sourceMergeUpstream(pid)) : op === "push" ? src(c.sourcePush(pid)) : src(c.sourceAbortMerge(pid)));
    } finally {
      running.value[pid] = null;
      await refresh(pid);
    }
  }

  const stop = (pid: string) => src(commands.sourceStopSync(pid));

  return { status, failure, running, refresh, run, stop };
});
