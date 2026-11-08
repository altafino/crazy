<script setup lang="ts">
import { computed, onMounted } from "vue";
import { CircleHelp, FolderOpen, Info, PackageCheck, Settings, Stethoscope } from "lucide-vue-next";
import { useNavStore, type View } from "./stores/nav";
import { usePreviewStore } from "./stores/preview";
import { useRunsStore } from "./stores/runs";
import { useSettingsStore } from "./stores/settings";
import { installApprovalNotifications } from "./lib/notify";
import DashboardView from "./views/DashboardView.vue";
import InstalledAppsView from "./views/InstalledAppsView.vue";
import DoctorView from "./views/DoctorView.vue";
import ProjectView from "./views/ProjectView.vue";
import SettingsView from "./views/SettingsView.vue";
import HelpView from "./views/HelpView.vue";
import AboutView from "./views/AboutView.vue";
import Toast from "./components/Toast.vue";

const nav = useNavStore();
const runs = useRunsStore();
const settings = useSettingsStore();
const preview = usePreviewStore();
onMounted(() => {
  void settings.load();
  void preview.listen(); // once, so badges and the tab dot follow previews of every project
});
installApprovalNotifications();

type Item = { view: Exclude<View, "project">; label: string; icon: typeof FolderOpen };
const items: Item[] = [
  { view: "projects", label: "Projects", icon: FolderOpen },
  { view: "installed", label: "Installed Apps", icon: PackageCheck },
  { view: "doctor", label: "Environment Doctor", icon: Stethoscope },
  { view: "settings", label: "Settings", icon: Settings },
  // Pinned to the bottom of the sidebar (.foot).
  { view: "help", label: "Help", icon: CircleHelp },
  { view: "about", label: "About", icon: Info },
];
// Inside a project, Projects stays the current section (D46).
const current = computed(() => (nav.view === "project" ? "projects" : nav.view));
const waiting = computed(() => runs.waitingProjects.length);
</script>

<template>
  <div class="shell">
    <nav class="sidebar" aria-label="Main">
      <p class="brand">AI App Studio</p>
      <button
        v-for="i in items" :key="i.view" type="button" class="nav-item" :class="{ foot: i.view === 'help' }" :title="i.label" :aria-label="i.label"
        :aria-current="current === i.view ? 'page' : undefined" @click="nav.go(i.view)"
      >
        <component :is="i.icon" :size="18" :stroke-width="1.75" aria-hidden="true" />
        <span class="label">{{ i.label }}</span>
        <span v-if="i.view === 'projects' && waiting" class="badge warn count" :aria-label="`${waiting} waiting for approval`">{{ waiting }}</span>
      </button>
    </nav>
    <!-- Keyed by view so each view opens at its top, not at the last view's scroll position. -->
    <main :key="nav.view" class="content">
      <DashboardView v-if="nav.view === 'projects'" />
      <ProjectView v-else-if="nav.view === 'project'" />
      <InstalledAppsView v-else-if="nav.view === 'installed'" />
      <DoctorView v-else-if="nav.view === 'doctor'" />
      <HelpView v-else-if="nav.view === 'help'" />
      <AboutView v-else-if="nav.view === 'about'" />
      <SettingsView v-else />
    </main>
    <Toast />
  </div>
</template>

<style scoped>
/* The content area scrolls, not the window, so the sidebar (and Help/About at its foot) never moves. */
.shell { display: grid; grid-template-columns: 220px 1fr; height: 100vh; }
.sidebar { background: var(--sidebar); padding: var(--space) 12px; display: flex; flex-direction: column; gap: 4px; border-right: 1px solid var(--border); overflow-y: auto; }
.brand { font-weight: 700; margin: 4px 8px 16px; }
.nav-item { font: inherit; text-align: left; padding: 8px 12px; border: 0; border-radius: 8px; background: none; color: var(--text); cursor: pointer; display: flex; align-items: center; gap: 12px; min-height: 40px; }
.nav-item:hover { background: rgb(128 128 128 / 0.15); }
.nav-item[aria-current="page"] { background: var(--info-bg); color: var(--accent-text); font-weight: 600; }
.count { margin-left: auto; }
.foot { margin-top: auto; }
.content { padding: 24px 32px; width: 100%; min-width: 0; overflow-y: auto; }

/* D58: below 1100px the sidebar becomes a 56px icon rail. */
@media (max-width: 1099px) {
  .shell { grid-template-columns: 56px 1fr; }
  .sidebar { padding: var(--space) 8px; align-items: center; }
  .brand, .label { display: none; }
  .nav-item { width: 40px; height: 40px; padding: 0; justify-content: center; position: relative; }
  .count { position: absolute; top: -2px; right: -4px; padding: 0 6px; }
  .content { padding: 16px 20px; }
}
</style>
