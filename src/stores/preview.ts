import { ref } from "vue";
import { defineStore } from "pinia";
import { commands, events } from "../bindings";
import type { CommandError, PreviewStatus } from "../bindings";
import { call } from "../lib/call";
import { DEFAULT_LIMITS, indicator } from "../lib/preview";

export type Log = { jobId: string; lines: string[]; /** Sequence number of lines[0]. */ firstSeq: number; nextSeq: number; truncated: boolean };

/**
 * Preview per project: latest status and the bounded log tail. Listens once (like runs);
 * log events below the tail's `nextSeq` are dropped (they were already in the tail).
 */
export const usePreviewStore = defineStore("preview", () => {
  const status = ref<Record<string, PreviewStatus | null>>({});
  const logs = ref<Record<string, Log | null>>({});
  const error = ref<Record<string, CommandError | null>>({});
  /** Projects whose preview ran in this Studio session; others show "Last preview" (M3-D41). */
  const thisSession = ref<Record<string, boolean>>({});
  let maxLines = DEFAULT_LIMITS.maxLines;
  /** Log batches that arrived before their job's log was known. */
  const early: Record<string, { fromSeq: number; lines: string[] }[]> = {};
  let unlisten: (() => void)[] | null = null;

  function append(log: Log, fromSeq: number, lines: string[]) {
    if (fromSeq < log.nextSeq) return;
    log.lines.push(...lines);
    log.nextSeq = fromSeq + lines.length;
    const over = log.lines.length - maxLines;
    if (over > 0) {
      log.lines.splice(0, over);
      log.firstSeq += over;
    }
  }

  function onLog(projectId: string, jobId: string, fromSeq: number, lines: string[]) {
    const log = logs.value[projectId];
    if (log && log.jobId === jobId) append(log, fromSeq, lines);
    else (early[jobId] ??= []).push({ fromSeq, lines });
  }

  function freshLog(projectId: string, jobId: string) {
    logs.value[projectId] = { jobId, lines: [], firstSeq: 0, nextSeq: 0, truncated: false };
    const held = (early[jobId] ?? []).sort((a, c) => a.fromSeq - c.fromSeq);
    delete early[jobId];
    for (const b of held) append(logs.value[projectId]!, b.fromSeq, b.lines);
  }

  function setStatus(s: PreviewStatus) {
    status.value[s.projectId] = s;
    if (s.live) thisSession.value[s.projectId] = true;
    if (logs.value[s.projectId]?.jobId !== s.jobId) {
      if (s.live) freshLog(s.projectId, s.jobId);
      else void loadLog(s.projectId, s.jobId);
    }
  }

  async function listen() {
    if (unlisten) return;
    unlisten = [];
    unlisten.push(await events.previewStatusEvent.listen(({ payload }) => setStatus(payload.status)));
    unlisten.push(await events.previewLogEvent.listen(({ payload: p }) => onLog(p.projectId, p.jobId, p.fromSeq, p.lines)));
  }

  /** The tail for a job (default: the latest). Replaces what we hold, then replays batches that arrived meanwhile. */
  async function loadLog(projectId: string, jobId: string | null = null) {
    const r = await call(commands.getPreviewLog(projectId, jobId));
    if (!r.ok) return void (error.value[projectId] = r.error);
    if (!r.data) return void (logs.value[projectId] = null);
    const t = r.data;
    const held = (early[t.jobId] ?? []).sort((a, c) => a.fromSeq - c.fromSeq);
    delete early[t.jobId];
    const lines = t.lines.slice(-maxLines);
    logs.value[projectId] = { jobId: t.jobId, lines, firstSeq: t.nextSeq - lines.length, nextSeq: t.nextSeq, truncated: t.logTruncated };
    for (const b of held) append(logs.value[projectId]!, b.fromSeq, b.lines);
  }

  /** Opening the tab: listeners first, then status and log (gaps fill from the tail). */
  async function load(projectId: string) {
    await listen();
    const lim = await call(commands.getPreviewSettings());
    if (lim.ok) maxLines = lim.data.maxLines;
    const r = await call(commands.getPreviewStatus(projectId));
    if (!r.ok) return void (error.value[projectId] = r.error);
    status.value[projectId] = r.data;
    if (r.data) {
      if (r.data.live) thisSession.value[projectId] = true;
      await loadLog(projectId, r.data.jobId);
    }
  }

  async function begin(projectId: string, run: () => ReturnType<typeof commands.startPreview>) {
    error.value[projectId] = null;
    await listen();
    const r = await call(run());
    if (!r.ok) {
      error.value[projectId] = r.error;
      return r;
    }
    setStatus(r.data);
    return r;
  }
  const start = (projectId: string) => begin(projectId, () => commands.startPreview(projectId));
  const restart = (projectId: string) => begin(projectId, () => commands.restartPreview(projectId));
  async function stop(projectId: string) {
    const r = await call(commands.stopPreview(projectId));
    if (!r.ok) error.value[projectId] = r.error;
    return r;
  }

  const indicatorOf = (projectId: string) => indicator(status.value[projectId]);

  return { status, logs, error, thisSession, listen, load, loadLog, start, restart, stop, indicatorOf };
});
