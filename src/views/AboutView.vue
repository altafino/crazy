<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { getTauriVersion, getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ChevronRight } from "lucide-vue-next";
import changelog from "../../CHANGELOG.md?raw";
import { releaseNotes } from "../lib/changelog";
import Markdown from "../components/chat/Markdown.vue";
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

const blocks = computed(() => releaseNotes(changelog, version.value).blocks);
const BADGES = { this: ["ok", "This version"], unreleased: ["info", "Not released yet"] } as const;
const RELEASES_URL = "https://github.com/altafino/crazy/releases";
const openReleases = () => void openUrl(RELEASES_URL).catch(() => {});

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

    <div class="rn-head">
      <h2 id="release-notes" tabindex="-1">Release notes</h2>
      <a href="#release-notes" class="rn-link" @click.prevent="openReleases">All releases on GitHub</a>
    </div>
    <details v-for="b in blocks" :key="b.title" class="rn" :open="b.open">
      <summary>
        <ChevronRight class="chev" :size="16" aria-hidden="true" />
        <h3 class="tnum">{{ b.dateText ? `${b.title} · ${b.dateText}` : b.title }}</h3>
        <span v-if="b.badge" class="badge" :class="BADGES[b.badge][0]">{{ BADGES[b.badge][1] }}</span>
      </summary>
      <div class="rn-body">
        <Markdown v-if="b.intro" :text="b.intro" />
        <template v-for="g in b.groups" :key="g.heading">
          <h4>{{ g.heading }}</h4>
          <Markdown :text="g.markdown" />
        </template>
      </div>
    </details>
  </section>
</template>

<style scoped>
.lead { max-width: 60ch; margin-top: 0; }
.facts { display: grid; grid-template-columns: max-content 1fr; gap: 8px 24px; margin: 24px 0; }
dt { color: var(--muted); }
dd { margin: 0; }
.actions { display: flex; gap: 8px; }
.rn-head { display: flex; flex-wrap: wrap; align-items: baseline; justify-content: space-between; gap: 4px 16px; margin-top: 32px; }
.rn-head h2 { margin: 0; }
.rn-link { color: var(--accent-text); }
.rn { border-top: 1px solid var(--border); }
.rn:last-child { border-bottom: 1px solid var(--border); }
.rn summary { display: flex; align-items: center; gap: 8px; padding: 12px 0; cursor: pointer; list-style: none; }
.rn summary::-webkit-details-marker { display: none; }
.rn h3 { margin: 0; font-size: 1em; font-weight: 600; }
.chev { flex: none; transition: transform 0.15s; }
.rn[open] .chev { transform: rotate(90deg); }
.rn-body { max-width: 70ch; padding: 0 0 16px 24px; }
.rn-body h4 { margin: 16px 0 8px; font-size: 1em; font-weight: 600; }
.rn-body h4:first-child { margin-top: 0; }
@media (max-width: 899px) { .rn-head { display: block; } }
@media (prefers-reduced-motion: reduce) { .chev { transition: none; } }
</style>
