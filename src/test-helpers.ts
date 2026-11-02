import { vi } from "vitest";
import type { ProjectSummary } from "./bindings";

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
});
