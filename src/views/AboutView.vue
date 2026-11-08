<script setup lang="ts">
import { onMounted, ref } from "vue";
import { getTauriVersion, getVersion } from "@tauri-apps/api/app";
import { useNavStore } from "../stores/nav";
import { useToastStore } from "../stores/toast";

/** Sidebar "About": which release this is, and a one-click copy of it for bug reports. */
const nav = useNavStore();
const toast = useToastStore();
const UNKNOWN = "Unknown";
const version = ref(UNKNOWN);
const tauri = ref(UNKNOWN);
const commit = __APP_COMMIT__;
const built = new Date(__APP_BUILT__);
const builtText = Number.isNaN(built.getTime()) ? UNKNOWN : built.toLocaleString(undefined, { dateStyle: "long", timeStyle: "short" });

onMounted(async () => {
  [version.value, tauri.value] = await Promise.all([getVersion().catch(() => UNKNOWN), getTauriVersion().catch(() => UNKNOWN)]);
});

async function copy() {
  const text = [`AI App Studio ${version.value} (${commit})`, `Built ${__APP_BUILT__}`, `Tauri ${tauri.value}`, navigator.userAgent].join("\n");
  try {
    await navigator.clipboard.writeText(text);
    toast.show("Details copied.");
  } catch {
    toast.show("Couldn't copy the details.");
  }
}
</script>

<template>
  <section aria-labelledby="about-h">
    <div class="view-head">
      <h1 id="about-h">About AI App Studio</h1>
    </div>
    <p class="lead">Describe an app, let Claude build it from real source code, then test, package and install it on this computer.</p>

    <dl class="facts">
      <dt>Version</dt><dd class="tnum">{{ version }}</dd>
      <dt>Commit</dt><dd class="mono">{{ commit }}</dd>
      <dt>Built</dt><dd>{{ builtText }}</dd>
      <dt>Tauri</dt><dd class="tnum">{{ tauri }}</dd>
    </dl>

    <div class="actions">
      <button type="button" class="btn copy" @click="copy">Copy details</button>
      <button type="button" class="btn help" @click="nav.go('help')">Help</button>
    </div>
    <p class="muted">Paste the details into a bug report so we know which build you're on.</p>
  </section>
</template>

<style scoped>
.lead { max-width: 60ch; margin-top: 0; }
.facts { display: grid; grid-template-columns: max-content 1fr; gap: 8px 24px; margin: 24px 0; }
dt { color: var(--muted); }
dd { margin: 0; }
.actions { display: flex; gap: 8px; }
</style>
