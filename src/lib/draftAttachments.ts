import { computed, markRaw, ref } from "vue";
import { defineStore } from "pinia";
import { commands } from "../bindings";
import type { AttachResult_Serialize, AttachmentKind, AttachmentTypeInfo } from "../bindings";
import { assetSrc, baseName, errorText, kindOfName, pickFiles, stagedPath } from "./attachments";
import type { Chip } from "./attachments";
import { call } from "./call";

/** One project's composer draft (prompt text and chips). It lives in a store so switching
 *  tabs or projects keeps it; a restart drops it (staged files are swept after 24 h). */
const useDrafts = defineStore("drafts", () => ({ byProject: markRaw(new Map<string, Draft>()) }));
type Draft = ReturnType<typeof createDraft>;

export function useDraftAttachments(projectId: string): Draft {
  const { byProject } = useDrafts();
  let d = byProject.get(projectId);
  if (!d) byProject.set(projectId, (d = createDraft()));
  d.announce.value = ""; // don't repeat an old announcement when the tab comes back
  return d;
}

/** The chips of one composer draft: attach, paste, remove, and what Run needs to send them. */
function createDraft() {
  const prompt = ref("");
  /** The composer's Auto-approve choice; null = follow Settings > Approvals. */
  const autoApprove = ref<boolean | null>(null);
  const draftId = ref(crypto.randomUUID());
  const chips = ref<Chip[]>([]);
  /** Read by the status strip (aria-live) once per change. */
  const announce = ref("");
  let types: AttachmentTypeInfo[] | null = null;

  async function loadTypes() {
    types ??= await commands.attachmentTypes();
    return types;
  }

  async function settle(chip: Chip, r: AttachResult_Serialize, kind: AttachmentKind | null, draft: string) {
    const s = r.staged;
    const path = s ? await stagedPath(draft, s.storedPath) : null;
    const done: Chip = s
      ? { key: chip.key, name: s.name, kind: s.kind, state: "ready", visible: true, sizeBytes: s.sizeBytes, id: s.id, path, src: s.kind === "image" ? assetSrc(path) : null }
      : { key: chip.key, name: r.name, kind, state: "error", visible: true, error: errorText(r.error!, kind) };
    const at = chips.value.findIndex((c) => c.key === chip.key);
    if (at < 0) return; // removed while it settled
    chips.value[at] = done;
    announce.value = s ? `${s.name} ${s.kind === "docx" ? "converted" : "attached"}` : `${r.name} can't attach: ${done.error}`;
  }

  function pending(name: string, kind: AttachmentKind | null): Chip {
    const docx = kind === "docx";
    const chip: Chip = { key: crypto.randomUUID(), name, kind, state: docx ? "converting" : "adding", visible: docx };
    chips.value.push(chip);
    // Quick copies never flash an "Adding…" chip: show it only after 300 ms.
    if (!docx) setTimeout(() => { const c = chips.value.find((x) => x.key === chip.key); if (c) c.visible = true; }, 300);
    return chips.value[chips.value.length - 1]!;
  }

  async function attachPaths(paths: string[]) {
    const known = await loadTypes();
    const draft = draftId.value;
    const rows = paths.map((p) => ({ chip: pending(baseName(p), kindOfName(baseName(p), known)), name: baseName(p) }));
    const r = await call(commands.attachFiles(draft, paths));
    for (const [i, row] of rows.entries()) {
      const kind = row.chip.kind;
      const res = r.ok ? r.data[i] : undefined;
      await settle(row.chip, res ?? { name: row.name, staged: null, error: { reason: "copyFailed", detail: r.ok ? "" : r.error.message } }, kind, draft);
    }
  }

  async function attachFiles() {
    const picked = await pickFiles(await loadTypes());
    if (picked?.length) await attachPaths(picked);
  }

  /** Ctrl+V image: the raw bytes go to the backend, which applies the image limit. */
  async function attachPasted(file: File) {
    const draft = draftId.value;
    const name = file.name || `Pasted image ${new Date().toTimeString().slice(0, 8).replace(/:/g, "-")}.png`;
    const chip = pending(name, "image");
    const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
    const r = await call(commands.attachPasted(draft, name, bytes));
    await settle(chip, r.ok ? r.data : { name, staged: null, error: { reason: "copyFailed", detail: r.error.message } }, "image", draft);
  }

  function remove(key: string) {
    const at = chips.value.findIndex((c) => c.key === key);
    if (at < 0) return;
    const [c] = chips.value.splice(at, 1);
    if (c?.id) void call(commands.removeAttachment(draftId.value, c.id));
  }

  /** After a successful Run: the chips moved into the conversation; start a fresh draft. */
  function reset() {
    chips.value = [];
    draftId.value = crypto.randomUUID();
  }

  const ids = computed(() => chips.value.filter((c) => c.state === "ready").map((c) => c.id!));
  return { prompt, autoApprove, draftId, chips, announce, ids, attachPaths, attachFiles, attachPasted, remove, reset };
}
