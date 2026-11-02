import { vi } from "vitest";
import type { AgentEvent, ProjectSummary, SessionSummary } from "./bindings";

export const ok = <T>(data: T) => Promise.resolve({ status: "ok" as const, data });
export const err = (message: string, fieldErrors: { field: string; message: string }[] = []) =>
  Promise.resolve({ status: "error" as const, error: { message, fieldErrors } });

export const project = (over: Partial<ProjectSummary> = {}): ProjectSummary => ({
  id: "p1", name: "Notes", slug: "notes", templateId: "tauri-vue", appId: "dev.studio.notes",
  workspacePath: "/home/u/Projects/notes", archived: false, supportsOperations: true, state: { kind: "ok" }, ...over,
});

export const mockCommands = () => ({
  listProjects: vi.fn(), validateNewProject: vi.fn(), createProject: vi.fn(), importProject: vi.fn(),
  relinkProject: vi.fn(), importProjectAsCopy: vi.fn(), getSettings: vi.fn(), saveSettings: vi.fn(), runDoctor: vi.fn(),
  listAgentProfiles: vi.fn(), saveAgentProfile: vi.fn(), deleteAgentProfile: vi.fn(), probeAgentProfile: vi.fn(),
  proposeAgentProfile: vi.fn(), startRun: vi.fn(), cancelRun: vi.fn(), answerApproval: vi.fn(), allowAndRetry: vi.fn(), continuePlan: vi.fn(),
  listSessions: vi.fn(), getSessionEvents: vi.fn(), runDiff: vi.fn(), runFileDiff: vi.fn(), revertRun: vi.fn(),
  deleteAllSnapshots: vi.fn(),
});

export const session = (over: Partial<SessionSummary> = {}): SessionSummary => ({
  id: "s1", jobId: "j1", projectId: "p1", profileId: null, mode: "changes", prompt: "Add a dark theme",
  resumedFrom: null, createdAt: "2026-11-02T14:32:00Z", canResume: true, hasSnapshot: true, snapshotPruned: false, hasAfter: true,
  jobState: "succeeded", startedAt: "2026-11-02T14:32:00Z", completedAt: "2026-11-02T14:35:00Z",
  errorKind: null, errorMessage: null, ...over,
});

/** Captures the agent-run-event listener so a test can push live events. */
export function mockEvents() {
  let handler: ((e: { payload: { jobId: string; event: AgentEvent } }) => unknown) | null = null;
  const events = {
    agentRunEvent: {
      listen: vi.fn(async (fn: typeof handler) => {
        handler = fn;
        return () => (handler = null);
      }),
    },
  };
  const emit = (jobId: string, event: AgentEvent) => handler?.({ payload: { jobId, event } });
  return { events, emit };
}
