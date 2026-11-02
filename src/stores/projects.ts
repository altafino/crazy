import { ref } from "vue";
import { defineStore } from "pinia";
import { commands } from "../bindings";
import type { CommandError, FieldError, ImportOutcome, NewProjectRequest_Deserialize, ProjectSummary } from "../bindings";
import { call, type Outcome, type Raw } from "../lib/call";

export type PendingImport = Extract<ImportOutcome, { kind: "needsRelink" | "needsCopyConfirm" }>;

export const useProjectsStore = defineStore("projects", () => {
  const projects = ref<ProjectSummary[]>([]);
  const loading = ref(false);
  /** Last failed action; carries `fieldErrors` for inline display. */
  const error = ref<CommandError | null>(null);
  /** Polite status line (import results, copy feedback). */
  const notice = ref("");
  /** Import awaiting the user's confirmation. */
  const pendingImport = ref<PendingImport | null>(null);

  async function track<T>(p: Promise<Raw<T>>) {
    error.value = null;
    const r = await call(p);
    if (!r.ok) error.value = r.error;
    return r;
  }

  async function refresh() {
    loading.value = true;
    const r = await track(commands.listProjects());
    if (r.ok) projects.value = r.data;
    loading.value = false;
    return r;
  }

  /** Validation errors are expected while typing, so they do not touch `error`. */
  function validate(req: NewProjectRequest_Deserialize): Promise<Outcome<FieldError[]>> {
    return call(commands.validateNewProject(req));
  }

  async function create(req: NewProjectRequest_Deserialize) {
    const r = await track(commands.createProject(req));
    if (r.ok) {
      notice.value = `Created "${r.data.name}".`;
      await refresh();
    }
    return r;
  }

  async function relink(id: string, path: string) {
    const r = await track(commands.relinkProject(id, path));
    if (r.ok) {
      notice.value = `Relinked "${r.data.name}".`;
      await refresh();
    }
    return r;
  }

  async function importAsCopy(path: string) {
    const r = await track(commands.importProjectAsCopy(path));
    if (r.ok) {
      notice.value = `Imported "${r.data.name}" as a copy.`;
      await refresh();
    }
    return r;
  }

  /** Step one of Import: asks the backend, then either finishes or queues a confirmation. */
  async function startImport(path: string) {
    const r = await track(commands.importProject(path));
    if (!r.ok) return r;
    const o = r.data;
    switch (o.kind) {
      case "imported":
        notice.value = `Imported "${o.project.name}".`;
        await refresh();
        break;
      case "alreadyInLibrary":
        notice.value = `"${o.project.name}" is already in your library.`;
        break;
      case "needsRelink":
      case "needsCopyConfirm":
        pendingImport.value = o;
        break;
    }
    return r;
  }

  async function confirmImport() {
    const p = pendingImport.value;
    if (!p) return;
    pendingImport.value = null;
    if (p.kind === "needsRelink") await relink(p.existing.id, p.newPath);
    else await importAsCopy(p.newPath);
  }

  function cancelImport() {
    pendingImport.value = null;
  }

  return { projects, loading, error, notice, pendingImport, refresh, validate, create, relink, importAsCopy, startImport, confirmImport, cancelImport };
});
