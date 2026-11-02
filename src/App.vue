<script setup lang="ts">
import { onMounted } from "vue";
import { useNavStore, type View } from "./stores/nav";
import { useSettingsStore } from "./stores/settings";
import DashboardView from "./views/DashboardView.vue";
import DoctorView from "./views/DoctorView.vue";
import SettingsView from "./views/SettingsView.vue";

const nav = useNavStore();
const settings = useSettingsStore();
onMounted(() => settings.load());

const items: { view: View; label: string }[] = [
  { view: "projects", label: "Projects" },
  { view: "doctor", label: "Environment Doctor" },
  { view: "settings", label: "Settings" },
];
</script>

<template>
  <div class="shell">
    <nav class="sidebar" aria-label="Main">
      <p class="brand">AI App Studio</p>
      <button
        v-for="i in items" :key="i.view" type="button" class="nav-item"
        :aria-current="nav.view === i.view ? 'page' : undefined" @click="nav.go(i.view)"
      >{{ i.label }}</button>
    </nav>
    <main class="content">
      <DashboardView v-if="nav.view === 'projects'" />
      <DoctorView v-else-if="nav.view === 'doctor'" />
      <SettingsView v-else />
    </main>
  </div>
</template>

<style scoped>
.shell { display: grid; grid-template-columns: 230px 1fr; min-height: 100vh; }
.sidebar { background: var(--sidebar); padding: var(--space); display: flex; flex-direction: column; gap: 4px; border-right: 1px solid var(--border); }
.brand { font-weight: 700; margin: 4px 8px 16px; }
.nav-item { font: inherit; text-align: left; padding: 8px 12px; border: 0; border-radius: 8px; background: none; color: var(--text); cursor: pointer; }
.nav-item:hover { background: rgb(128 128 128 / 0.15); }
.nav-item[aria-current="page"] { background: rgb(128 128 128 / 0.25); font-weight: 600; }
.content { padding: 24px 32px; max-width: 1100px; width: 100%; }
</style>
