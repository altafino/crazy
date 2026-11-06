<script setup lang="ts">
import { onMounted, ref } from "vue";
import { commands } from "../bindings";
import type { ExportReadiness, ToolSource } from "../bindings";
import { DOCKER_FIX } from "../components/releases/helpers";
import { call } from "../lib/call";
import { useDoctorStore } from "../stores/doctor";

const doctor = useDoctorStore();
/** Optional tools (design 10): Docker, needed only for exporting apps for other systems. */
const docker = ref<ExportReadiness | null>(null);
const dockerFailure = ref<string | null>(null);
async function checkDocker() {
  const r = await call(commands.exportReadiness());
  docker.value = r.ok ? r.data : null;
  dockerFailure.value = r.ok ? null : r.error.message;
}
onMounted(() => Promise.all([doctor.run(false), checkDocker()]));
const recheck = () => Promise.all([doctor.run(true), checkDocker()]);
const DOCKER_BADGE = { missing: "Not installed", notRunning: "Not running", noPermission: "No permission" } as const;

const SOURCES: Record<ToolSource, string> = {
  override: "override",
  shellPath: "shell PATH",
  guiPath: "GUI PATH",
  system: "system",
};
</script>

<template>
  <section aria-labelledby="doc-h" :aria-busy="doctor.loading">
    <div class="view-head">
      <h1 id="doc-h">Environment Doctor</h1>
      <button type="button" class="btn" :disabled="doctor.loading" :aria-busy="doctor.loading" @click="recheck">
        {{ doctor.loading ? "Checking…" : "Re-check" }}
      </button>
    </div>

    <div role="alert">
      <p v-if="doctor.error" class="callout danger">{{ doctor.error.message }}</p>
    </div>
    <p aria-live="polite" class="sr-only">{{ doctor.loading ? "Checking tools" : "" }}</p>

    <template v-if="doctor.report">
      <div v-if="doctor.report.warnings.length" class="callout warn" role="region" aria-label="Warnings">
        <strong>Warnings</strong>
        <ul>
          <li v-for="w in doctor.report.warnings" :key="w">{{ w }}</li>
        </ul>
      </div>

      <p class="muted legend">
        Source shows where a tool was found: <em>override</em> is your path in Settings, <em>shell PATH</em> is your login
        shell, <em>GUI PATH</em> is what apps started from GNOME see, <em>system</em> is a standard system location.
      </p>

      <div class="table-wrap">
        <table>
          <caption class="sr-only">Development tools</caption>
          <thead>
            <tr><th scope="col">Tool</th><th scope="col">Status</th><th scope="col">Version</th><th scope="col">Path</th><th scope="col">Source</th><th scope="col">Details</th></tr>
          </thead>
          <tbody>
            <tr v-for="t in doctor.report.tools" :key="t.id">
              <th scope="row">{{ t.name }}</th>
              <template v-if="t.state === 'installed'">
                <td><span class="badge ok">Installed</span></td>
                <td>{{ t.version }}</td>
                <td class="mono path" :title="t.path">{{ t.path }}</td>
                <td>{{ SOURCES[t.source] }}</td>
                <td></td>
              </template>
              <template v-else-if="t.state === 'missing'">
                <td><span class="badge danger">Missing</span></td>
                <td></td><td></td><td></td>
                <td>{{ t.hint }}</td>
              </template>
              <template v-else>
                <td><span class="badge warn">Misconfigured</span></td>
                <td></td>
                <td class="mono path" :title="t.path ?? ''">{{ t.path }}</td>
                <td></td>
                <td>{{ t.reason }}</td>
              </template>
            </tr>
          </tbody>
        </table>
      </div>

      <h2 class="sec">Optional tools</h2>
      <div class="table-wrap">
        <table>
          <caption class="sr-only">Optional tools</caption>
          <thead>
            <tr><th scope="col">Tool</th><th scope="col">Status</th><th scope="col">Version</th><th scope="col">Path</th><th scope="col">Source</th><th scope="col">Details</th></tr>
          </thead>
          <tbody>
            <tr>
              <th scope="row">Docker</th>
              <template v-if="docker?.docker === 'ready'">
                <td><span class="badge ok">Installed</span></td>
                <td>{{ docker.dockerVersion }}</td>
                <td></td><td></td><td></td>
              </template>
              <template v-else-if="docker">
                <td><span class="badge warn">{{ DOCKER_BADGE[docker.docker] }}</span></td>
                <td></td><td></td><td></td>
                <td>
                  {{ DOCKER_FIX[docker.docker].text }}<template v-if="DOCKER_FIX[docker.docker].command">{{ " " }}<code class="mono">{{ DOCKER_FIX[docker.docker].command }}</code></template>
                  <span v-if="docker.detail" class="muted raw mono">{{ docker.detail }}</span>
                </td>
              </template>
              <template v-else-if="dockerFailure">
                <td><span class="badge warn">Unknown</span></td>
                <td></td><td></td><td></td>
                <td>{{ dockerFailure }}</td>
              </template>
              <template v-else>
                <td class="muted">Checking…</td><td></td><td></td><td></td><td></td>
              </template>
            </tr>
          </tbody>
        </table>
      </div>
      <p class="muted">Needed only for exporting apps for other systems</p>

      <h2 class="sec">Shell</h2>
      <p>
        Environment capture:
        <span v-if="doctor.report.shell.status.state === 'ok'" class="badge ok">OK</span>
        <span v-else-if="doctor.report.shell.status.state === 'timedOut'" class="badge warn">Timed out</span>
        <span v-else class="badge danger">Failed</span>
        <span v-if="doctor.report.shell.status.state === 'failed'"> {{ doctor.report.shell.status.reason }}</span>
      </p>
      <p v-if="!doctor.report.shell.aliases.length" class="muted">No tracked aliases set environment variables.</p>
      <ul v-else class="aliases">
        <li v-for="a in doctor.report.shell.aliases" :key="a.tool">
          <strong>{{ a.tool }}</strong>
          <code class="def">{{ a.definition }}</code>
          <span class="muted"> sets {{ a.envAssignments.join(", ") }}</span>
        </li>
      </ul>

      <h2 class="sec">Graphics</h2>
      <p>
        <span class="badge" :class="doctor.report.graphics.info.nvidia ? 'warn' : 'ok'">NVIDIA: {{ doctor.report.graphics.info.nvidia ? "yes" : "no" }}</span>
        <span class="badge" :class="doctor.report.graphics.info.wayland ? 'ok' : ''">Wayland: {{ doctor.report.graphics.info.wayland ? "yes" : "no" }}</span>
        <span v-if="doctor.report.graphics.info.driverVersion" class="badge">Driver {{ doctor.report.graphics.info.driverVersion }}</span>
      </p>
      <p v-if="doctor.report.graphics.note" class="muted">{{ doctor.report.graphics.note }}</p>
    </template>
    <p v-else-if="doctor.loading" class="muted">Checking…</p>
  </section>
</template>

<style scoped>
.legend { margin-top: 0; }
.callout ul { margin: 4px 0 0; padding-left: 20px; }
.table-wrap { overflow-x: auto; background: var(--card); border: 1px solid var(--border); border-radius: var(--radius); }
table { border-collapse: collapse; width: 100%; }
th, td { text-align: left; padding: 8px 12px; border-bottom: 1px solid var(--border); vertical-align: top; }
tbody tr:last-child > * { border-bottom: 0; }
.path { max-width: 260px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.sec { margin-top: 24px; }
.aliases { padding-left: 20px; }
.def { margin: 0 8px; }
.badge { margin-right: 6px; }
.raw { display: block; overflow-wrap: anywhere; }
.sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
</style>
