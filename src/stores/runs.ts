import { computed, ref } from "vue";
import { defineStore } from "pinia";
import { commands, events } from "../bindings";
import type { AgentEvent, ApprovalChoice_Deserialize, CommandError, SessionMode, SessionSummary } from "../bindings";
import { call } from "../lib/call";

export const ACTIVE_STATES = ["queued", "running", "waitingForApproval"] as const;
export const isActive = (s: SessionSummary) => (ACTIVE_STATES as readonly string[]).includes(s.jobState);

/**
 * Runs per project: the session list, each session's events (log replay merged with live
 * events, deduped by `seq`), and which projects wait for an approval (D54 badges).
 */
export const useRunsStore = defineStore("runs", () => {
  const sessions = ref<Record<string, SessionSummary[]>>({});
  const eventsBySession = ref<Record<string, AgentEvent[]>>({});
  /** Selected run per project, shared by the Chat and Changes tabs (D47). */
  const selected = ref<Record<string, string | null>>({});
  const error = ref<CommandError | null>(null);
  /** jobId -> sessionId, filled from session lists. */
  const jobToSession: Record<string, string> = {};
  /** Live events that arrived before their session was known. */
  const pending: Record<string, AgentEvent[]> = {};
  let unlisten: (() => void) | null = null;

  function merge(sessionId: string, incoming: AgentEvent[]) {
    const list = eventsBySession.value[sessionId] ?? [];
    const seen = new Set(list.map((e) => e.seq));
    const added = incoming.filter((e) => !seen.has(e.seq));
    if (!added.length && eventsBySession.value[sessionId]) return;
    eventsBySession.value[sessionId] = [...list, ...added].sort((a, b) => a.seq - b.seq);
  }

  function index(projectId: string, list: SessionSummary[]) {
    sessions.value[projectId] = list;
    for (const s of list) {
      jobToSession[s.jobId] = s.id;
      const early = pending[s.jobId];
      if (early) {
        delete pending[s.jobId];
        merge(s.id, early);
      }
    }
    if (!(projectId in selected.value) || !list.some((s) => s.id === selected.value[projectId])) {
      selected.value[projectId] = list[0]?.id ?? null;
    }
  }

  async function loadSessions(projectId: string) {
    const r = await call(commands.listSessions(projectId));
    if (r.ok) index(projectId, r.data);
    else error.value = r.error;
    return r;
  }

  /** Subscribe first, then replay; duplicates collapse on `seq`. */
  async function loadEvents(sessionId: string) {
    await listen();
    const r = await call(commands.getSessionEvents(sessionId));
    if (r.ok) merge(sessionId, r.data);
    else error.value = r.error;
    return r;
  }

  function projectOfJob(jobId: string): string | undefined {
    for (const [pid, list] of Object.entries(sessions.value)) if (list.some((s) => s.jobId === jobId)) return pid;
  }

  async function listen() {
    if (unlisten) return;
    unlisten = await events.agentRunEvent.listen(async ({ payload }) => {
      const { jobId, event } = payload;
      const sid = jobToSession[jobId];
      if (sid) merge(sid, [event]);
      else (pending[jobId] ??= []).push(event);
      // Status-changing events refresh the job state shown in pickers and badges.
      if (["approvalNeeded", "approvalAnswered", "completed", "error"].includes(event.kind) || !sid) {
        const pid = projectOfJob(jobId);
        if (pid) await loadSessions(pid);
      }
      onEvent?.(jobId, event);
    });
  }
  /** Hook for side effects such as desktop notifications (set by the chat lane). */
  let onEvent: ((jobId: string, e: AgentEvent) => void) | null = null;
  function setOnEvent(fn: typeof onEvent) {
    onEvent = fn;
  }

  async function start(projectId: string, prompt: string, mode: SessionMode, profileId: string | null) {
    error.value = null;
    await listen();
    const r = await call(commands.startRun(projectId, prompt, mode, profileId, null, null));
    if (!r.ok) {
      error.value = r.error;
      return r;
    }
    await loadSessions(projectId);
    const sid = jobToSession[r.data];
    if (sid) selected.value[projectId] = sid;
    return r;
  }

  async function cancel(jobId: string) {
    const r = await call(commands.cancelRun(jobId));
    if (!r.ok) error.value = r.error;
    return r;
  }

  async function answer(jobId: string, requestId: string, choice: ApprovalChoice_Deserialize) {
    const r = await call(commands.answerApproval(jobId, requestId, choice));
    if (!r.ok) error.value = r.error;
    return r;
  }

  async function allowAndRetry(projectId: string, sessionId: string, rule: string) {
    const r = await call(commands.allowAndRetry(sessionId, rule));
    if (!r.ok) {
      error.value = r.error;
      return r;
    }
    await loadSessions(projectId);
    const sid = jobToSession[r.data];
    if (sid) selected.value[projectId] = sid;
    return r;
  }

  /** "Make these changes" (D53): a Make changes run resuming the plan session. */
  async function continueRun(projectId: string, sessionId: string) {
    const r = await call(commands.continuePlan(sessionId));
    if (!r.ok) {
      error.value = r.error;
      return r;
    }
    await loadSessions(projectId);
    const sid = jobToSession[r.data];
    if (sid) selected.value[projectId] = sid;
    return r;
  }

  function selectedSession(projectId: string): SessionSummary | null {
    const id = selected.value[projectId];
    return sessions.value[projectId]?.find((s) => s.id === id) ?? null;
  }

  /** Projects with a run waiting for approval (badges on card, tab and sidebar). */
  const waitingProjects = computed(() =>
    Object.entries(sessions.value)
      .filter(([, list]) => list.some((s) => s.jobState === "waitingForApproval"))
      .map(([pid]) => pid),
  );

  return {
    sessions, eventsBySession, selected, error, waitingProjects,
    loadSessions, loadEvents, listen, setOnEvent, start, cancel, answer, allowAndRetry, continueRun, selectedSession,
  };
});
