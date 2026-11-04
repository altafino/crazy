import { ref } from "vue";
import { defineStore } from "pinia";
import { commands, events } from "../bindings";
import type { BuildStatus, CommandError, InstallState, InstalledApp, ReleaseView } from "../bindings";
import { call } from "../lib/call";
import { DEFAULT_LIMITS } from "../lib/preview";
import { useToastStore } from "./toast";
import type { Log } from "./preview";

const FIRST_INSTALL = "It can take a few seconds to appear in Activities.";

/**
 * Builds, releases and install state per project (M4). Built like the preview store: status,
 * a bounded log tail and replay of log batches that arrive before their job is known.
 */
export const useBuildsStore = defineStore("builds", () => {
  const status = ref<Record<string, BuildStatus | null>>({});
  const logs = ref<Record<string, Log | null>>({});
  /** Every release row including removed ones; the view filters on `filesRemoved`. */
  const releases = ref<Record<string, ReleaseView[]>>({});
  const install = ref<Record<string, InstallState | null>>({});
  const error = ref<Record<string, CommandError | null>>({});
  const installed = ref<InstalledApp[]>([]);
  const installedLoaded = ref(false);
  const maxLines = DEFAULT_LIMITS.maxLines;
  const early: Record<string, { fromSeq: number; lines: string[] }[]> = {};
  let unlisten: (() => void)[] | null = null;

  const isBuilding = (projectId: string) => !!status.value[projectId]?.live;
  /** The installed version, from the project's own state or the Installed Apps list. */
  function installedVersion(projectId: string): string | null {
    const s = install.value[projectId] ?? installed.value.find((a) => a.projectId === projectId)?.state;
    return s && s.kind === "installed" ? s.version : null;
  }

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
  function replay(projectId: string, jobId: string) {
    const held = (early[jobId] ?? []).sort((a, c) => a.fromSeq - c.fromSeq);
    delete early[jobId];
    for (const b of held) append(logs.value[projectId]!, b.fromSeq, b.lines);
  }
  function onLog(projectId: string, jobId: string, fromSeq: number, lines: string[]) {
    const log = logs.value[projectId];
    if (log && log.jobId === jobId) append(log, fromSeq, lines);
    else (early[jobId] ??= []).push({ fromSeq, lines });
  }

  function setStatus(s: BuildStatus) {
    const was = status.value[s.projectId];
    status.value[s.projectId] = s;
    if (logs.value[s.projectId]?.jobId !== s.jobId) {
      if (s.live) {
        logs.value[s.projectId] = { jobId: s.jobId, lines: [], firstSeq: 0, nextSeq: 0, truncated: false };
        replay(s.projectId, s.jobId);
      } else if (s.hasLog) void loadLog(s.projectId, s.jobId);
    }
    // A finished build may have stored a release; a finished one may have changed what's installed.
    if (was?.live && !s.live) void refresh(s.projectId);
  }

  async function listen() {
    if (unlisten) return;
    unlisten = [];
    try {
      unlisten.push(await events.buildStatusEvent.listen(({ payload }) => setStatus(payload.status)));
      unlisten.push(await events.buildLogEvent.listen(({ payload: p }) => onLog(p.projectId, p.jobId, p.fromSeq, p.lines)));
    } catch {
      unlisten = null; // no event bridge (tests, browser); try again on the next call
    }
  }

  async function loadLog(projectId: string, jobId: string | null = null) {
    const r = await call(commands.getBuildLog(projectId, jobId));
    if (!r.ok) return void (error.value[projectId] = r.error);
    if (!r.data) return void (logs.value[projectId] = null);
    const t = r.data;
    const lines = t.lines.slice(-maxLines);
    logs.value[projectId] = { jobId: t.jobId, lines, firstSeq: t.nextSeq - lines.length, nextSeq: t.nextSeq, truncated: t.logTruncated };
    replay(projectId, t.jobId);
  }

  async function loadReleases(projectId: string) {
    const r = await call(commands.listReleases(projectId, true));
    if (r.ok) releases.value[projectId] = r.data;
    else error.value[projectId] = r.error;
  }
  async function loadInstall(projectId: string) {
    const r = await call(commands.getInstallState(projectId));
    if (r.ok) install.value[projectId] = r.data;
    else error.value[projectId] = r.error;
  }
  const refresh = (projectId: string) => Promise.all([loadReleases(projectId), loadInstall(projectId), loadInstalled()]);

  /** Latest status only (project cards): enough to show "Building…". */
  async function loadStatus(projectId: string) {
    try {
      await listen();
      const r = await call(commands.getBuildStatus(projectId));
      if (r.ok && r.data) setStatus(r.data);
    } catch {
      /* no backend (tests) */
    }
  }

  /** Opening the Releases tab: listeners first, then everything (gaps fill from the tail). */
  async function load(projectId: string) {
    await listen();
    const r = await call(commands.getBuildStatus(projectId));
    if (!r.ok) error.value[projectId] = r.error;
    else {
      status.value[projectId] = r.data;
      if (r.data?.hasLog || r.data?.live) await loadLog(projectId, r.data.jobId);
    }
    await refresh(projectId);
  }

  /** One request at a time: every project card asks on mount. */
  let installedReq: Promise<void> | null = null;
  function loadInstalled() {
    installedReq ??= (async () => {
      try {
        const r = await call(commands.listInstalledApps());
        if (r.ok) installed.value = r.data;
      } catch {
        /* no backend (tests) */
      }
      installedLoaded.value = true;
    })().finally(() => (installedReq = null));
    return installedReq;
  }

  async function nextVersion(projectId: string) {
    return call(commands.nextBuildVersion(projectId));
  }

  async function start(projectId: string, version: string, notes: string | null) {
    error.value[projectId] = null;
    await listen();
    const r = await call(commands.startBuild(projectId, version, notes));
    if (r.ok) setStatus(r.data);
    else error.value[projectId] = r.error;
    return r;
  }
  async function stop(projectId: string) {
    const r = await call(commands.stopBuild(projectId));
    if (!r.ok) error.value[projectId] = r.error;
    return r;
  }

  /**
   * Install, Activate and Roll back are one call. Acts at once, then toasts with Undo, which
   * activates the previous release (no further toast chain beyond the plain confirmation).
   */
  async function activate(projectId: string, releaseId: string, appName: string, undo = true) {
    error.value[projectId] = null;
    const wasInstalled = install.value[projectId]?.kind === "installed";
    const r = await call(commands.activateRelease(projectId, releaseId));
    if (!r.ok) {
      error.value[projectId] = r.error;
      return r;
    }
    install.value[projectId] = r.data.state;
    const version = r.data.state.kind === "installed" ? r.data.state.version : "";
    const prev = r.data.previousReleaseId;
    const text = `GNOME now starts ${appName} ${version}`.trim() + (wasInstalled ? "" : `. ${FIRST_INSTALL}`);
    useToastStore().show(text, undo && prev ? { label: "Undo", run: () => void activate(projectId, prev, appName, false) } : undefined);
    await Promise.all([loadReleases(projectId), loadInstalled()]);
    return r;
  }

  async function removeRelease(projectId: string, releaseId: string) {
    const r = await call(commands.deleteRelease(projectId, releaseId));
    if (r.ok) await loadReleases(projectId);
    else error.value[projectId] = r.error;
    return r;
  }
  async function uninstall(projectId: string) {
    const r = await call(commands.uninstallApp(projectId));
    if (r.ok) await refresh(projectId);
    else error.value[projectId] = r.error;
    return r;
  }
  async function launch(projectId: string) {
    const r = await call(commands.launchInstalled(projectId));
    if (!r.ok) error.value[projectId] = r.error;
    return r;
  }
  async function repair(projectId: string) {
    const r = await call(commands.repairLauncher(projectId));
    if (r.ok) install.value[projectId] = r.data;
    else error.value[projectId] = r.error;
    return r;
  }

  return {
    status, logs, releases, install, error, installed, installedLoaded,
    isBuilding, installedVersion, listen, load, loadStatus, loadLog, loadReleases, loadInstall, loadInstalled, refresh,
    nextVersion, start, stop, activate, removeRelease, uninstall, launch, repair,
  };
});
