import { convertFileSrc } from "@tauri-apps/api/core";
import { dataDir, join } from "@tauri-apps/api/path";
import { open } from "@tauri-apps/plugin-dialog";
import type { AttachError, AttachmentKind, AttachmentLimits_Serialize, AttachmentTypeInfo, RunAttachment } from "../bindings";
import { formatSize } from "./format";

/** One chip, in the composer (draft) or under a prompt in the timeline (sent). */
export type Chip = {
  key: string;
  name: string;
  kind: AttachmentKind | null;
  state: "adding" | "converting" | "ready" | "error" | "removed";
  /** Adding chips stay hidden for 300 ms so quick copies never flicker. */
  visible: boolean;
  sizeBytes?: number;
  /** Staged id, passed to startRun. */
  id?: string;
  error?: string;
  /** Absolute path of the stored file (opener) and its asset URL (preview). */
  path?: string | null;
  src?: string | null;
};

/** "very-long-name….png": middle truncation keeps the extension readable. */
export function middleTruncate(name: string, max = 24): string {
  if (name.length <= max) return name;
  const tail = Math.min(10, Math.floor((max - 1) / 2));
  return `${name.slice(0, max - 1 - tail)}…${name.slice(name.length - tail)}`;
}

const dotless = (e: string) => e.replace(/^\./, "").toLowerCase();
export const extensionOf = (name: string) => (name.includes(".") ? dotless(name.slice(name.lastIndexOf("."))) : "");
export const baseName = (p: string) => p.split(/[\\/]/).pop() || p;

export function kindOfName(name: string, types: AttachmentTypeInfo[]): AttachmentKind | null {
  const ext = extensionOf(name);
  return types.find((t) => t.extensions.some((e) => dotless(e) === ext))?.kind ?? null;
}

/** The picker filter comes from the backend's list, never a hard-coded one. */
export const pickerFilters = (types: AttachmentTypeInfo[]) => [
  { name: "Supported files", extensions: types.flatMap((t) => t.extensions.map(dotless)) },
];

/** Resolves to null when cancelled. */
export async function pickFiles(types: AttachmentTypeInfo[]): Promise<string[] | null> {
  const r = await open({ multiple: true, directory: false, filters: pickerFilters(types) });
  return Array.isArray(r) ? r : typeof r === "string" ? [r] : null;
}

const KIND_PLURAL: Record<AttachmentKind, string> = { image: "images", pdf: "PDFs", text: "text files", docx: "Word files" };

/** Exact DESIGN.md reason text; `kind` is the file's own type, for "Too large: images up to 20 MB". */
export function errorText(e: AttachError, kind: AttachmentKind | null): string {
  switch (e.reason) {
    case "unsupported": return "This file type isn't supported yet";
    case "tooLarge": return `Too large: ${kind ? KIND_PLURAL[kind] : "files"} up to ${e.mb} MB`;
    case "tooManyFiles": return `Limit: ${e.max} files per prompt`;
    case "totalTooLarge": return `Too large: ${e.mb} MB per prompt in total`;
    case "secretFile": return "Secret-looking files can't be attached";
    case "pandocMissing": return "Install pandoc to attach Word files";
    case "conversionTimedOut": return "Conversion took too long";
    case "conversionFailed": return "Couldn't convert this file";
    case "copyFailed": return "Couldn't copy this file";
    case "fileMissing": return "This file is no longer there";
  }
}

/** The muted detail after the name; null for errors (their reason replaces it). */
export function chipDetail(c: Chip): string | null {
  if (c.state === "adding") return "Adding…";
  if (c.state === "converting") return "Converting…";
  if (c.state === "removed") return "File removed";
  if (c.state === "ready") return c.kind === "docx" && c.id ? "converted" : formatSize(c.sizeBytes ?? 0);
  return null;
}

/** DR-D3 accessible names: "screenshot.png, 1.2 MB", "requirements.docx, converting", ... */
export function chipLabel(c: Chip): string {
  if (c.state === "error") return `${c.name}, can't attach: ${c.error}`;
  const d = c.state === "adding" || c.state === "converting" ? c.state : c.state === "removed" ? "file removed" : chipDetail(c);
  return `${c.name}, ${d}`;
}

/** Run's disabled reason (DR-D4): an error wins over a conversion still running. */
export function runBlockReason(chips: Chip[]): string | null {
  const errors = chips.filter((c) => c.state === "error").length;
  if (errors) return `Remove ${errors} file${errors === 1 ? "" : "s"} that can't be attached`;
  const waiting = chips.filter((c) => c.state === "adding" || c.state === "converting").length;
  if (waiting) return `Waiting for ${waiting} file${waiting === 1 ? "" : "s"} to finish converting`;
  return null;
}

/** Stored files live under the data dir; the asset protocol and opener are scoped to it. */
export async function stagedPath(draftId: string, storedPath: string): Promise<string | null> {
  try {
    return await join(await dataDir(), "ai-app-studio", "attachments", "staging", draftId, storedPath);
  } catch {
    return null;
  }
}
export function assetSrc(path: string | null | undefined): string | null {
  try {
    return path ? convertFileSrc(path) : null;
  } catch {
    return null;
  }
}

export function chipOfRun(a: RunAttachment): Chip {
  return {
    key: `${a.runId}:${a.ordinal}`, name: a.name, kind: a.kind, sizeBytes: a.sizeBytes, visible: true,
    state: a.removed ? "removed" : "ready", path: a.path, src: a.kind === "image" && !a.removed ? assetSrc(a.path) : null,
  };
}

/** Settings > Attachments: the backend enforces these bounds; the form mirrors them. */
type Limits = AttachmentLimits_Serialize;
export const ATTACH_RANGES: Record<keyof Limits, { min: number; max: number; label: string }> = {
  maxFiles: { min: 1, max: 50, label: "Files per prompt" },
  maxImageMb: { min: 1, max: 256, label: "Image size (MB)" },
  maxPdfMb: { min: 1, max: 256, label: "PDF size (MB)" },
  maxTextMb: { min: 1, max: 256, label: "Text and code file size (MB)" },
  maxDocxMb: { min: 1, max: 256, label: "Word file size (MB)" },
  maxDocxConvertedMb: { min: 1, max: 256, label: "Converted Word text size (MB)" },
  maxTotalMb: { min: 1, max: 1024, label: "All files of one prompt (MB)" },
  pandocTimeoutSecs: { min: 10, max: 600, label: "Word conversion time limit (seconds)" },
};
export const DEFAULT_ATTACH_LIMITS: Limits = {
  maxFiles: 10, maxImageMb: 20, maxPdfMb: 32, maxTextMb: 2, maxDocxMb: 20, maxDocxConvertedMb: 2, maxTotalMb: 64, pandocTimeoutSecs: 60,
};

/** Field errors for out-of-range or non-integer limits; empty when valid. */
export function validateAttachLimits(l: Record<keyof Limits, unknown>): Partial<Record<keyof Limits, string>> {
  const out: Partial<Record<keyof Limits, string>> = {};
  for (const k of Object.keys(ATTACH_RANGES) as (keyof Limits)[]) {
    const { min, max } = ATTACH_RANGES[k];
    const v = l[k];
    if (typeof v !== "number" || !Number.isInteger(v) || v < min || v > max) out[k] = `Enter a whole number from ${min.toLocaleString("en-US")} to ${max.toLocaleString("en-US")}.`;
  }
  return out;
}
