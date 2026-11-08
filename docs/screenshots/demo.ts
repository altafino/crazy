// Screenshot harness: runs the real UI against fixture data (no backend, no real projects).
// Open http://localhost:5199/docs/screenshots/demo.html?view=project&project=pixel-pac&tab=chat
//   ?view=projects|installed|doctor|settings|help|about, or view=project with &project=<slug>&tab=<tab>
// Shots: pixel-pac/chat, budget-buddy/changes+releases, recipe-box/plan, view=doctor, view=projects
import { mockIPC } from "@tauri-apps/api/mocks";
import type {
  AgentEvent_Serialize as Ev, AgentProfile_Serialize, ApprovalSettings_Serialize, DoctorCrafts_Serialize,
  DoctorReport_Serialize, ExportReadiness_Serialize, FileChange_Serialize, FileDiff_Serialize, InstalledApp_Serialize,
  Plan_Serialize, ProjectSummary, ReleaseView_Serialize, SessionSummary_Serialize, Settings,
} from "../../src/bindings";

const now = Date.now();
const ago = (min: number) => new Date(now - min * 60_000).toISOString();
const agoMs = (min: number) => now - min * 60_000;
const ROOT = "/home/demo/Projects/AIAppStudio";

const proj = (name: string, slug: string): ProjectSummary => ({
  id: `p-${slug}`, name, slug, templateId: "tauri-vue", appId: `dev.demo.${slug.replace(/-/g, "")}`,
  workspacePath: `${ROOT}/${slug}`, archived: false, supportsOperations: true, state: { kind: "ok" },
});
const projects: ProjectSummary[] = [
  proj("Pixel Pac", "pixel-pac"), proj("Budget Buddy", "budget-buddy"), proj("Recipe Box", "recipe-box"),
  proj("Markdown Notes", "markdown-notes"), proj("Pomodoro Timer", "pomodoro-timer"),
];
const pid = (slug: string) => `p-${slug}`;

// ---- sessions & events ----------------------------------------------------------------
const sess = (id: string, slug: string, prompt: string, min: number, over: Partial<SessionSummary_Serialize> = {}): SessionSummary_Serialize => ({
  id, jobId: `job-${id}`, projectId: pid(slug), profileId: "prof-claude", mode: "changes", prompt, resumedFrom: null,
  createdAt: ago(min), canResume: true, hasSnapshot: true, snapshotPruned: false, hasAfter: true, checkpointRef: `refs/studio/checkpoints/${id}`,
  jobState: "succeeded", startedAt: ago(min), completedAt: ago(min - 3), errorKind: null, errorMessage: null, ...over,
});
const sessions: Record<string, SessionSummary_Serialize[]> = {
  [pid("pixel-pac")]: [
    sess("s-pac-3", "pixel-pac", "Make the ghosts smarter: each one should chase Pac-Man differently, and they should turn blue and flee when he eats a power pellet.", 6, { jobState: "waitingForApproval", completedAt: null, hasAfter: false }),
    sess("s-pac-2", "pixel-pac", "Add a high-score table that is saved between launches.", 1500),
    sess("s-pac-1", "pixel-pac", "Create a Pac-Man style maze game with arrow-key controls and a score.", 4300),
  ],
  [pid("budget-buddy")]: [
    sess("s-bb-2", "budget-buddy", "Show a progress bar for each monthly budget category on the overview, and turn it red when I go over.", 95),
    sess("s-bb-1", "budget-buddy", "Add CSV export for all transactions.", 2900),
  ],
  [pid("recipe-box")]: [sess("s-rb-1", "recipe-box", "Let me tag recipes and filter by tag.", 5000)],
  [pid("markdown-notes")]: [sess("s-mn-1", "markdown-notes", "Add a live preview next to the editor.", 8200)],
  [pid("pomodoro-timer")]: [sess("s-pt-1", "pomodoro-timer", "Play a soft chime when a session ends.", 300)],
};

let seq = 0;
const e = {
  status: (text: string, min: number): Ev => ({ kind: "status", seq: seq++, atMs: agoMs(min), text }),
  text: (text: string, min: number): Ev => ({ kind: "text", seq: seq++, atMs: agoMs(min), text }),
  tool: (id: string, name: string, summary: string, input: unknown, out: string, min: number): Ev[] => [
    { kind: "toolCall", seq: seq++, atMs: agoMs(min), id, name, input, summary },
    { kind: "toolResult", seq: seq++, atMs: agoMs(min - 0.1), id, isError: false, output: out },
  ],
};
const pacEvents: Ev[] = [
  e.status("Claude is working", 6),
  e.text("I'll start by looking at how the ghosts are set up today, then give each one its own personality.", 5.9),
  ...e.tool("t1", "Read", "src/game/ghost.ts", { file_path: `${ROOT}/pixel-pac/src/game/ghost.ts` }, "export class Ghost { ... }", 5.8),
  ...e.tool("t2", "Read", "src/game/maze.ts", { file_path: `${ROOT}/pixel-pac/src/game/maze.ts` }, "export const MAZE = [...]", 5.7),
  ...e.tool("t3", "Grep", "powerPellet", { pattern: "powerPellet" }, "src/game/engine.ts:88", 5.6),
  e.text("Plan: four behaviours (chase, ambush, flank, wander), plus a `frightened` mode that lasts 8 seconds after a power pellet. I'll put the targeting logic in `ghost.ts` and keep the engine loop unchanged.", 5.4),
  ...e.tool("t4", "Edit", "src/game/ghost.ts", { file_path: `${ROOT}/pixel-pac/src/game/ghost.ts`, old_string: "target = pacman.tile", new_string: "target = this.personality(pacman, blinky)" }, "File updated", 4.5),
  { kind: "fileChange", seq: seq++, atMs: agoMs(4.5), path: "src/game/ghost.ts" },
  ...e.tool("t5", "Edit", "src/game/engine.ts", { file_path: `${ROOT}/pixel-pac/src/game/engine.ts`, old_string: "ghosts.forEach(g => g.step())", new_string: "ghosts.forEach(g => g.step(frightened))" }, "File updated", 3.8),
  { kind: "fileChange", seq: seq++, atMs: agoMs(3.8), path: "src/game/engine.ts" },
  e.text("Targeting is in place. Next I'll run the type check to make sure the new `frightened` flag is wired through everywhere.", 3.2),
  { kind: "approvalNeeded", seq: seq++, atMs: agoMs(3), requestId: "req-1", tool: "Bash", input: { command: "pnpm exec vue-tsc --noEmit" }, description: "Type-check the project after the ghost changes" },
];
const bbEvents: Ev[] = (seq = 0, [
  e.status("Claude is working", 95),
  e.text("I'll add a `BudgetBar` component and use it on the overview for every category that has a monthly limit.", 94.8),
  ...e.tool("b1", "Read", "src/views/Overview.vue", { file_path: `${ROOT}/budget-buddy/src/views/Overview.vue` }, "...", 94.5),
  ...e.tool("b2", "Write", "src/components/BudgetBar.vue", { file_path: `${ROOT}/budget-buddy/src/components/BudgetBar.vue`, content: "<script setup>…" }, "File created", 93),
  { kind: "fileChange", seq: seq++, atMs: agoMs(93), path: "src/components/BudgetBar.vue" },
  ...e.tool("b3", "Edit", "src/views/Overview.vue", { file_path: `${ROOT}/budget-buddy/src/views/Overview.vue`, old_string: "<li>", new_string: "<BudgetBar>" }, "File updated", 92),
  { kind: "fileChange", seq: seq++, atMs: agoMs(92), path: "src/views/Overview.vue" },
  e.text("Done. Each category now shows spent / limit as a bar. It turns red once spending passes the limit, and the figures are announced to screen readers.", 91),
  { kind: "usage", seq: seq++, atMs: agoMs(91), costUsd: 0.18, durationMs: 212_000 },
  { kind: "completed", seq: seq++, atMs: agoMs(91), outcome: "success", denials: [] },
] as Ev[]);
const eventsFor: Record<string, Ev[]> = { "s-pac-3": pacEvents, "s-bb-2": bbEvents };

// ---- changes ----------------------------------------------------------------------------
const changes: FileChange_Serialize[] = [
  { path: "src/components/BudgetBar.vue", oldPath: null, status: "added", additions: 34, deletions: 0, skippedReason: null },
  { path: "src/views/Overview.vue", oldPath: null, status: "modified", additions: 12, deletions: 4, skippedReason: null },
  { path: "src/style.css", oldPath: null, status: "modified", additions: 6, deletions: 0, skippedReason: null },
];
const L = (kind: "context" | "added" | "removed", text: string) => ({ kind, text });
const fileDiff = (path: string): FileDiff_Serialize => {
  const lines = path.endsWith("BudgetBar.vue")
    ? [L("added", '<script setup lang="ts">'), L("added", 'const props = defineProps<{ label: string; spent: number; limit: number }>();'), L("added", "const pct = computed(() => Math.min(100, (props.spent / props.limit) * 100));"), L("added", "const over = computed(() => props.spent > props.limit);"), L("added", "</script>"), L("added", ""), L("added", "<template>"), L("added", '  <div class="bar" role="meter" :aria-valuenow="spent" :aria-valuemax="limit">'), L("added", '    <span class="fill" :class="{ over }" :style="{ width: pct + \'%\' }" />'), L("added", "    <small>{{ label }}: {{ spent }} of {{ limit }}</small>"), L("added", "  </div>"), L("added", "</template>")]
    : [L("context", "<ul class=\"categories\">"), L("context", "  <li v-for=\"c in categories\" :key=\"c.id\">"), L("removed", "    <span>{{ c.name }}</span>"), L("removed", "    <span>{{ money(c.spent) }}</span>"), L("added", "    <BudgetBar :label=\"c.name\" :spent=\"c.spent\" :limit=\"c.limit\" />"), L("context", "  </li>"), L("context", "</ul>")];
  const added = lines.filter((l) => l.kind !== "removed").length, removed = lines.filter((l) => l.kind === "removed").length;
  return { hunks: [{ oldStart: 18, oldLines: lines.length - lines.filter((l) => l.kind === "added").length, newStart: 18, newLines: added, lines }], binary: false, truncated: false, oldSize: 900, newSize: 1400 };
};

// ---- plan -------------------------------------------------------------------------------
const plan: Plan_Serialize = {
  summary: "Recipe Box: a cosy desktop app to keep, tag and cook from your recipes.",
  savedAt: ago(1400),
  tasks: [
    ["Recipe list and detail view", "Cards with a photo, title and cooking time.", true],
    ["Add and edit recipes", "Form with ingredients and numbered steps.", true],
    ["Tags and filtering", "Tap a tag to narrow the list.", true],
    ["Search by title and ingredient", "Instant results as you type.", false],
    ["Shopping list from selected recipes", "Merge duplicate ingredients and tick them off.", false],
    ["Import and export as JSON", "Back up and share your collection.", false],
    ["Package a release", "Build an AppImage and install it.", false],
  ].map(([text, detail, done]) => ({ text: text as string, detail: detail as string, done: done as boolean })),
};

// ---- releases / installed ----------------------------------------------------------------
const rel = (id: string, v: string, min: number, over: Partial<ReleaseView_Serialize> = {}): ReleaseView_Serialize => ({
  id, version: v, createdAt: ago(min), sizeBytes: 78_400_000, sourceCommit: "a1f3c9e", dirtySource: false, smoke: "passed", smokeDetail: null,
  notes: null, lastActivatedAt: null, filesRemoved: false, schemaVersion: null, appimagePath: `${ROOT}/budget-buddy/.studio/releases/${id}/budget-buddy.AppImage`, ...over,
});
const releases: ReleaseView_Serialize[] = [
  rel("r3", "1.2.0", 80, { notes: "Budget progress bars on the overview", sizeBytes: 81_200_000, sourceCommit: "7be41d0", schemaVersion: 3, lastActivatedAt: ago(70) }),
  rel("r2", "1.1.0", 2900, { notes: "CSV export", sourceCommit: "2c9a8f4", schemaVersion: 2, lastActivatedAt: ago(2800) }),
  rel("r1", "1.0.0", 9800, { notes: "First release", sourceCommit: "91d0be7", schemaVersion: 1, sizeBytes: 76_900_000, lastActivatedAt: ago(9700) }),
];
const inst = (slug: string, name: string, version: string, rid: string): InstalledApp_Serialize => ({
  projectId: pid(slug), projectName: name, state: { kind: "installed", releaseId: rid, version }, installedAt: ago(5000), releasesKept: 3, unverified: false, folder: `/home/demo/.local/share/aiappstudio/apps/${slug}`, icon: null,
});
const installed: InstalledApp_Serialize[] = [inst("budget-buddy", "Budget Buddy", "1.2.0", "r3"), inst("pomodoro-timer", "Pomodoro Timer", "0.3.1", "r9"), inst("markdown-notes", "Markdown Notes", "0.5.0", "r8")];

// ---- doctor ------------------------------------------------------------------------------
const tool = (id: string, name: string, version: string, path: string, source: "shellPath" | "system" | "guiPath" = "shellPath") => ({ id, name, state: "installed" as const, version, path, source });
const doctorReport: DoctorReport_Serialize = {
  shell: { path: "/usr/bin/zsh", aliases: [], status: { state: "ok" } },
  tools: [
    tool("claude", "Claude Code", "2.1.4", "/home/demo/.local/bin/claude"),
    tool("node", "Node.js", "22.11.0", "/usr/bin/node", "system"),
    tool("pnpm", "pnpm", "9.15.0", "/usr/bin/pnpm", "system"),
    tool("rustc", "Rust (rustc)", "1.83.0", "/home/demo/.cargo/bin/rustc"),
    tool("cargo", "Cargo", "1.83.0", "/home/demo/.cargo/bin/cargo"),
    tool("git", "Git", "2.47.1", "/usr/bin/git", "system"),
    tool("webkit", "WebKitGTK 4.1", "2.46.5", "/usr/lib/libwebkit2gtk-4.1.so", "system"),
  ],
  graphics: { info: { nvidia: false, wayland: true, driverVersion: null }, note: null },
  warnings: [],
  checkedAt: Math.floor(now / 1000),
};
const exportReadiness: ExportReadiness_Serialize = { docker: "missing", dockerVersion: null, detail: null, imagePresent: false, licenseAcceptedAt: null, lastTargets: [] };
const craftDoctor: DoctorCrafts_Serialize = {
  rows: [
    { name: "Krita", wired: true, install: "Installed (Flatpak)", also: null, asksEachAction: true },
    { name: "Blender", wired: true, install: "Installed (package)", also: null, asksEachAction: true },
    { name: "Inkscape", wired: true, install: "Installed (package)", also: null, asksEachAction: false },
  ],
  warnings: [],
};

const settings: Settings = { defaultProjectsDir: ROOT, toolOverrides: {} };
const profile: AgentProfile_Serialize = { id: "prof-claude", name: "Claude Code", provider: "claude", executable: "/home/demo/.local/bin/claude", env: [], model: null, inactivitySecs: 600, instructions: "", isDefault: true, enabled: true };
const approvals: ApprovalSettings_Serialize = { autoApprove: false, askForRisky: true };

// ---- IPC --------------------------------------------------------------------------------
const handlers: Record<string, (a: any) => unknown> = {
  list_projects: () => projects,
  project_icon: () => null,
  project_plan: (a) => (a.id === pid("recipe-box") ? plan : null),
  get_settings: () => settings,
  run_doctor: () => doctorReport,
  export_readiness: () => exportReadiness,
  craft_doctor: () => craftDoctor,
  list_agent_profiles: () => [profile],
  get_approval_settings: () => approvals,
  list_sessions: (a) => sessions[a.projectId] ?? [],
  get_session_events: (a) => eventsFor[a.sessionId] ?? [],
  run_diff: () => changes,
  run_file_diff: (a) => fileDiff(a.path),
  list_run_attachments: () => [],
  get_attachment_limits: () => ({ maxFiles: 10, maxImageMb: 10, maxPdfMb: 20, maxTextMb: 2, maxDocxMb: 20, maxDocxConvertedMb: 10, maxTotalMb: 50, pandocTimeoutSecs: 30 }),
  list_releases: (a) => (a.projectId === pid("budget-buddy") ? releases : []),
  get_install_state: (a) => (a.projectId === pid("budget-buddy") ? { kind: "installed", releaseId: "r3", version: "1.2.0" } : { kind: "notInstalled" }),
  list_release_exports: () => [],
  get_build_status: () => null,
  list_installed_apps: () => installed,
  next_build_version: () => "1.3.0",
  get_preview_settings: () => ({ maxLines: 5000, maxLogMb: 10, keepLogs: 5 }),
  get_preview_status: () => null,
  craft_apps: () => ({ enabled: true, apps: [], notInstalled: [], notWired: [], defaults: [] }),
  github_auth_status: () => ({ kind: "signedOut" }),
  "plugin:app|version": () => "0.4.0",
  "plugin:app|tauri_version": () => "2.1.0",
  "plugin:app|name": () => "AI App Studio",
};
mockIPC((cmd, args) => {
  const h = handlers[cmd];
  if (h) return h(args);
  console.info(`[demo] unmocked command: ${cmd}`, args);
  return null;
}, { shouldMockEvents: true });

await import("../../src/main.ts");

// ?view=...&project=<slug>&tab=<tab>
const q = new URLSearchParams(location.search);
const { useNavStore } = await import("../../src/stores/nav");
const app = (document.querySelector("#app") as any).__vue_app__;
const nav = useNavStore(app.config.globalProperties.$pinia);
const view = q.get("view");
if (view === "project") {
  nav.openProject(pid(q.get("project") ?? "pixel-pac"));
  nav.tab = (q.get("tab") ?? "chat") as typeof nav.tab;
} else if (view) nav.go(view as typeof nav.view);

// ?theme=dark: force the prefers-color-scheme: dark rules on (headless browsers can't switch the OS theme).
if (q.get("theme") === "dark") {
  for (const sheet of Array.from(document.styleSheets))
    for (const r of Array.from(sheet.cssRules)) if (r instanceof CSSMediaRule && r.media.mediaText.includes("prefers-color-scheme: dark")) r.media.mediaText = "all";
  document.documentElement.style.colorScheme = "dark";
}
