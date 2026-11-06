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
  attachmentTypes: vi.fn(() => Promise.resolve([])), attachFiles: vi.fn(), attachPasted: vi.fn(), clipboardFilePaths: vi.fn(() => Promise.resolve([])), getApprovalSettings: vi.fn(() => ok({ autoApprove: false, askForRisky: true })), setApprovalSettings: vi.fn(), removeAttachment: vi.fn(() => ok(null)),
  listRunAttachments: vi.fn(() => ok([])), getAttachmentLimits: vi.fn(), setAttachmentLimits: vi.fn(),
  githubAuthStatus: vi.fn(() => ok({ kind: "signedOut" })), githubListRepos: vi.fn(() => ok([])), githubCreateRepo: vi.fn(),
  projectIcon: vi.fn(() => ok(null)), setProjectIcon: vi.fn(), previewIcon: vi.fn(),
  projectPlan: vi.fn(() => ok(null)), parsePlan: vi.fn(() => Promise.resolve({ summary: "", tasks: [], savedAt: null })), saveProjectPlan: vi.fn(), setPlanTask: vi.fn(),
  getDatabaseLimits: vi.fn(() => ok({ queryTimeLimitSecs: 2 })), setDatabaseLimits: vi.fn(), appdbOverview: vi.fn(), appdbRows: vi.fn(),
  appdbGuard: vi.fn(() => ok({ previewRunning: false, installedRunning: false, appName: "Notes" })), appdbListBackups: vi.fn(() => ok([])),
  appdbBackup: vi.fn(), appdbDeleteBackup: vi.fn(), appdbRestoreBackup: vi.fn(), appdbCopyInstalledToPreview: vi.fn(), appdbResetPreview: vi.fn(),
  appdbRollbackCheck: vi.fn(),
  githubSignIn: vi.fn(), githubSignInStop: vi.fn(), cloneProject: vi.fn(), cloneProjectStop: vi.fn(),
  exportReadiness: vi.fn(() => ok({ docker: "ready", dockerVersion: "29.8.2", detail: null, imagePresent: true, licenseAcceptedAt: null, lastTargets: ["linux", "windows"] })),
  startExport: vi.fn(), listReleaseExports: vi.fn(() => ok([])), exportCacheSize: vi.fn(() => ok({ bytes: 0 })),
  clearExportCaches: vi.fn(() => ok(null)), withdrawExportLicense: vi.fn(() => ok(null)),
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
