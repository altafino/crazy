<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { Loader } from "lucide-vue-next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { commands } from "../../bindings";
import type { CommandError, ExportReadiness, ExportTarget } from "../../bindings";
import { call } from "../../lib/call";
import { useBuildsStore } from "../../stores/builds";
import { useNavStore } from "../../stores/nav";
import Modal from "../Modal.vue";
import { DOCKER_FIX, TARGET_SHORT } from "./helpers";

const LICENSE_URL = "https://go.microsoft.com/fwlink/?LinkId=2086102";
const props = defineProps<{ projectId: string; releaseId: string; appName: string; version: string; existing: ExportTarget[] }>();
const emit = defineEmits<{ close: [] }>();
const builds = useBuildsStore();
const nav = useNavStore();

const linux = ref(false);
const windows = ref(false);
const touched = ref(false);
const accepted = ref(false); // never pre-ticked
const readiness = ref<ExportReadiness | null>(null);
const checkFailure = ref<string | null>(null);
/** The spinner shows only for work over 400 ms (DESIGN.md "States"). */
const slow = ref(false);
const failure = ref<CommandError | null>(null);
const busy = ref(false);
let slowTimer: ReturnType<typeof setTimeout> | undefined;

onMounted(async () => {
  slowTimer = setTimeout(() => (slow.value = true), 400);
  const r = await call(commands.exportReadiness());
  clearTimeout(slowTimer);
  slow.value = false;
  if (!r.ok) return void (checkFailure.value = r.error.message);
  readiness.value = r.data;
  if (!touched.value) {
    linux.value = r.data.lastTargets.includes("linux");
    windows.value = r.data.lastTargets.includes("windows");
  }
});
onUnmounted(() => clearTimeout(slowTimer));

const targets = computed<ExportTarget[]>(() => [...(linux.value ? (["linux"] as const) : []), ...(windows.value ? (["windows"] as const) : [])]);
const needsLicense = computed(() => windows.value && !!readiness.value && !readiness.value.licenseAcceptedAt);
const dockerReady = computed(() => readiness.value?.docker === "ready");
const canExport = computed(() => !busy.value && dockerReady.value && targets.value.length > 0 && (!needsLicense.value || accepted.value));

const fix = computed(() => (readiness.value && readiness.value.docker !== "ready" ? DOCKER_FIX[readiness.value.docker] : null));

function doctor() {
  emit("close");
  nav.go("doctor");
}

async function submit() {
  if (!canExport.value) return;
  busy.value = true;
  failure.value = null;
  const r = await builds.startExport(props.projectId, props.releaseId, targets.value, needsLicense.value && accepted.value);
  busy.value = false;
  if (r.ok) emit("close");
  else failure.value = r.error;
}
const replaces = (t: ExportTarget) => props.existing.includes(t);
</script>

<template>
  <Modal :title="`Export ${appName} ${version}`" @close="emit('close')">
    <form class="form" @submit.prevent="submit">
      <div class="opt">
        <label class="check">
          <input v-model="linux" type="checkbox" :aria-describedby="`ex-linux-d${replaces('linux') ? ' ex-linux-r' : ''}`" @change="touched = true" />
          <span>Other Linux</span>
        </label>
        <p id="ex-linux-d" class="caption muted">AppImage, .deb, .rpm for Ubuntu 22.04 and newer, Debian 12, Fedora, …</p>
        <p v-if="replaces('linux')" id="ex-linux-r" class="caption muted">Replaces the current {{ TARGET_SHORT.linux }} export when it finishes.</p>
      </div>
      <div class="opt">
        <label class="check">
          <input v-model="windows" type="checkbox" :aria-describedby="`ex-windows-d${replaces('windows') ? ' ex-windows-r' : ''}`" @change="touched = true" />
          <span>Windows 11</span>
        </label>
        <p id="ex-windows-d" class="caption muted">setup.exe installer (not tested by Studio, unsigned)</p>
        <p v-if="replaces('windows')" id="ex-windows-r" class="caption muted">Replaces the current {{ TARGET_SHORT.windows }} export when it finishes.</p>
      </div>

      <div v-if="needsLicense" class="opt">
        <label class="check">
          <input v-model="accepted" type="checkbox" required aria-describedby="ex-license-d" />
          <span>I accept Microsoft's license for the Windows build tools (Visual C++ runtime and Windows SDK)</span>
        </label>
        <p id="ex-license-d" class="caption">
          <a :href="LICENSE_URL" class="link" @click.prevent="openUrl(LICENSE_URL).catch(() => {})">Read the license</a>
        </p>
      </div>

      <p v-if="readiness && !readiness.imagePresent" class="caption muted">First export: downloads the build environment (about 3 GB) and takes several minutes.</p>

      <p v-if="!readiness && !checkFailure && slow" class="caption muted checking" role="status"><Loader :size="12" :stroke-width="1.75" aria-hidden="true" /> Checking Docker…</p>
      <div v-if="fix" class="callout warn" role="alert">
        <p class="msg">{{ fix.text }}<template v-if="fix.command">{{ " " }}<code class="mono">{{ fix.command }}</code></template></p>
        <button type="button" class="btn small" @click="doctor">Open Doctor</button>
      </div>
      <p v-if="checkFailure" class="callout warn" role="alert">{{ checkFailure }}</p>
      <p v-if="failure" class="callout danger" role="alert">{{ failure.message }}</p>

      <div class="actions">
        <button type="button" class="btn" @click="emit('close')">Cancel</button>
        <button type="submit" class="btn primary" :disabled="!canExport">Export</button>
      </div>
    </form>
  </Modal>
</template>

<style scoped>
.form { display: flex; flex-direction: column; gap: 12px; max-width: 480px; }
.opt { display: flex; flex-direction: column; gap: 2px; }
.check { display: flex; gap: 8px; align-items: flex-start; min-height: 32px; font-weight: 600; margin: 0; cursor: pointer; }
.check input { margin-top: 5px; flex: none; }
.opt .caption { margin: 0 0 0 24px; }
.caption { font-size: 13px; margin: 0; }
.link { color: var(--accent-text); }
.checking { display: flex; gap: 6px; align-items: center; }
.checking svg { animation: spin 1.6s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
.callout { margin: 0; display: flex; gap: 12px; align-items: center; justify-content: space-between; flex-wrap: wrap; }
.callout.danger { display: block; }
.msg { margin: 0; flex: 1; min-width: 0; }
.callout .btn { min-height: 32px; }
.actions { display: flex; justify-content: flex-end; gap: 8px; }
</style>
