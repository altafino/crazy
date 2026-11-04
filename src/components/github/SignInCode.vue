<script setup lang="ts">
import { computed, onUnmounted, ref } from "vue";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useGithubStore } from "../../stores/github";

/** M5-DR4: the sign-in code inline, with Open GitHub, Copy code, Stop and an elapsed caption. */
const gh = useGithubStore();
const copied = ref(false);
const now = ref(Date.now());
const tick = setInterval(() => (now.value = Date.now()), 1000);
onUnmounted(() => clearInterval(tick));

const elapsed = computed(() => (gh.signInStartedAt ? Math.max(0, Math.round((now.value - gh.signInStartedAt) / 1000)) : 0));

async function copy() {
  if (!gh.signInCode) return;
  await navigator.clipboard.writeText(gh.signInCode.code).catch(() => {});
  copied.value = true;
}
</script>

<template>
  <section class="signin" aria-label="Sign in to GitHub">
    <p class="label">Enter this code on GitHub</p>
    <template v-if="gh.signInCode">
      <output class="code" aria-label="Sign-in code">{{ gh.signInCode.code }}</output>
      <div class="actions">
        <button type="button" class="btn primary" @click="openUrl(gh.signInCode.url).catch(() => {})">Open GitHub</button>
        <button type="button" class="btn" @click="copy">Copy code</button>
        <button type="button" class="btn" @click="gh.stopSignIn()">Stop</button>
      </div>
    </template>
    <div v-else class="actions"><span class="caption">Getting a code…</span><button type="button" class="btn" @click="gh.stopSignIn()">Stop</button></div>
    <p class="caption">Waiting for GitHub · {{ elapsed }} s</p>
    <p class="sr-only" aria-live="polite">{{ copied ? "Code copied" : "" }}</p>
  </section>
</template>

<style scoped>
.signin { display: flex; flex-direction: column; gap: 6px; align-items: flex-start; }
.label { margin: 0; font-weight: 600; }
.code { font-family: var(--mono, monospace); font-size: 18px; user-select: text; letter-spacing: 0.05em; }
.actions { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
.caption { margin: 0; color: var(--muted); font-size: 13px; }
</style>
