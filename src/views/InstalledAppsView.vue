<script setup lang="ts">
import { onMounted, ref } from "vue";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { MoreHorizontal } from "lucide-vue-next";
import type { InstalledApp_Serialize as App } from "../bindings";
import UninstallDialog from "../components/UninstallDialog.vue";
import { useInstalledStore } from "../stores/installed";
import { useNavStore } from "../stores/nav";

const installed = useInstalledStore();
const nav = useNavStore();
const menuFor = ref<string | null>(null);
const confirming = ref<App | null>(null);
const localError = ref<string | null>(null);

onMounted(() => installed.refresh());

const name = (a: App) => a.projectName ?? "Removed project";
const version = (a: App) => ("version" in a.state ? (a.state.version ?? null) : null);

function when(iso: string) {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const time = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return d.toDateString() === new Date().toDateString() ? `today ${time}` : `${d.toLocaleDateString()} ${time}`;
}

function caption(a: App) {
  const v = version(a);
  return `${v ? `Active ${v}` : "Active release unknown"} · installed ${when(a.installedAt)} · ${a.releasesKept} ${a.releasesKept === 1 ? "release" : "releases"} kept`;
}

async function reveal(a: App) {
  menuFor.value = null;
  localError.value = null;
  try {
    // opener:default allows reveal, not arbitrary openPath (capabilities stay minimal).
    await revealItemInDir(a.folder);
  } catch (e) {
    localError.value = `Could not open folder: ${e instanceof Error ? e.message : String(e)}`;
  }
}

function askUninstall(a: App) {
  menuFor.value = null;
  confirming.value = a;
}

async function doUninstall() {
  const a = confirming.value!;
  confirming.value = null;
  await installed.uninstall(a.projectId, name(a));
}
</script>

<template>
  <section aria-labelledby="inst-h">
    <div class="view-head">
      <div>
        <h1 id="inst-h">Installed Apps</h1>
        <p class="muted">Apps Studio has added to GNOME. They run without Studio.</p>
      </div>
    </div>

    <div role="alert">
      <p v-if="installed.error" class="callout danger">{{ installed.error.message }}</p>
      <p v-if="localError" class="callout danger">{{ localError }}</p>
    </div>

    <p v-if="installed.loading && !installed.apps.length" class="muted">Loading…</p>

    <div v-else-if="!installed.apps.length" class="empty">
      <h2>No installed apps yet</h2>
      <p class="muted">Build a release in a project's Releases tab, then choose Install. The app shows up here and in GNOME.</p>
      <div class="empty-actions">
        <button type="button" class="btn primary" @click="nav.go('projects')">Go to Projects</button>
      </div>
    </div>

    <template v-else>
      <ul class="list" @keydown.esc="menuFor = null">
        <li v-for="a in installed.apps" :key="a.projectId" class="row" :data-project-id="a.projectId">
          <img v-if="a.icon" class="icon-img" :src="a.icon" alt="" aria-hidden="true" width="40" height="40" />
          <span v-else class="icon" aria-hidden="true">{{ name(a).charAt(0).toUpperCase() }}</span>
          <div class="info">
            <span class="name">{{ name(a) }}</span>
            <span class="muted caption">{{ caption(a) }}</span>
            <span class="flags">
              <span v-if="a.unverified" class="badge warn">Unverified release</span>
              <template v-if="a.state.kind === 'launcherMissing'">
                <span class="badge danger">Launcher missing</span>
                <span class="muted caption">The launcher entry was removed outside Studio</span>
              </template>
              <span v-else-if="a.state.kind === 'unknownRelease'" class="muted caption">
                The active version has no kept release file. Open the project's Releases tab to activate one.
              </span>
            </span>
          </div>
          <div class="actions">
            <button v-if="a.projectName" type="button" class="link" @click="nav.openProject(a.projectId)">Open project</button>
            <button v-if="a.state.kind === 'launcherMissing'" type="button" class="btn small" @click="installed.repair(a.projectId)">Repair</button>
            <button v-else-if="a.state.kind === 'installed'" type="button" class="btn small" @click="installed.launch(a.projectId)">Launch</button>
            <div class="menu-wrap">
              <button
                type="button" class="btn small" :aria-label="`More actions for ${name(a)}`" aria-haspopup="menu"
                :aria-expanded="menuFor === a.projectId" @click="menuFor = menuFor === a.projectId ? null : a.projectId"
              >
                <MoreHorizontal :size="16" aria-hidden="true" />
              </button>
              <div v-if="menuFor === a.projectId" class="menu" role="menu" :aria-label="`${name(a)} actions`">
                <button type="button" role="menuitem" @click="reveal(a)">Show in Files</button>
                <button type="button" role="menuitem" @click="askUninstall(a)">Uninstall…</button>
              </div>
            </div>
          </div>
        </li>
      </ul>
      <p class="muted foot">New apps can take a few seconds to appear in GNOME Activities.</p>
    </template>

    <UninstallDialog
      v-if="confirming" :app-name="name(confirming)" :version="version(confirming)" :releases-kept="confirming.releasesKept"
      @confirm="doUninstall" @cancel="confirming = null"
    />
  </section>
</template>

<style scoped>
.list { list-style: none; margin: 0; padding: 0; }
.row { display: flex; align-items: center; gap: 12px; min-height: 64px; padding: 8px 0; border-bottom: 1px solid var(--border); }
/* The launcher's own icon, as GNOME shows it: no rounding on top. */
.icon-img { width: 40px; height: 40px; flex: none; object-fit: contain; }
.icon { width: 40px; height: 40px; flex: none; border-radius: 10px; background: var(--info-bg); color: var(--accent-text); display: grid; place-items: center; font-weight: 700; }
.info { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
.name { font-weight: 600; }
.flags { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
.flags:empty { display: none; }
.caption { font-size: 0.875em; }
.actions { display: flex; align-items: center; gap: 8px; flex: none; }
.link { font: inherit; background: none; border: 0; padding: 4px; color: var(--accent-text); cursor: pointer; }
.link:hover { text-decoration: underline; }
.menu-wrap { position: relative; }
.menu { position: absolute; right: 0; top: 100%; z-index: 10; min-width: 160px; background: var(--bg, canvas); border: 1px solid var(--border); border-radius: 8px; padding: 4px; display: flex; flex-direction: column; box-shadow: 0 4px 16px rgb(0 0 0 / 0.2); }
.menu button { font: inherit; text-align: left; background: none; border: 0; border-radius: 6px; padding: 8px 12px; color: var(--text); cursor: pointer; min-height: 36px; }
.menu button:hover, .menu button:focus-visible { background: rgb(128 128 128 / 0.15); }
.foot { margin-top: var(--space); }
.empty { text-align: center; padding: 64px 16px; }
.empty-actions { display: flex; justify-content: center; gap: 8px; margin-top: var(--space); }
</style>
