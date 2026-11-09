<script setup lang="ts">
import { commands } from "../../bindings";
import type { ApprovalSettings_Serialize as Approvals } from "../../bindings";
import { call } from "../../lib/call";
import SettingsGroup from "./SettingsGroup.vue";

const load = () => call(commands.getApprovalSettings());
const save = (d: Approvals) => call(commands.setApprovalSettings({ ...d }));
</script>

<template>
  <SettingsGroup v-slot="{ draft, commit }" title="Approvals" :load="load" :save="save">
    <p class="muted">When Claude wants to run a command, edit a file or read one outside the project, Studio asks you first. Auto-approve answers for you in <strong>Make changes</strong> runs. <strong>Plan first</strong> always asks.</p>

    <label class="check">
      <input v-model="draft.autoApprove" type="checkbox" @change="commit('autoApprove', draft.autoApprove)" />
      <span>
        <strong>Auto-approve commands, file edits and reads by default</strong>
        <span class="muted hint">The Auto-approve box under the message starts in this state. You can still change it for each run.</span>
      </span>
    </label>

    <label class="check">
      <input v-model="draft.askForRisky" type="checkbox" @change="commit('askForRisky', draft.askForRisky)" />
      <span>
        <strong>Still ask for risky actions</strong>
        <span class="muted hint">With Auto-approve on, these still show an approval card:</span>
      </span>
    </label>
    <ul class="muted risky">
      <li><code>sudo</code>, <code>su</code> and other commands that run as another user</li>
      <li><code>git push</code>, <code>commit</code>, <code>remote</code>, <code>reset --hard</code>, <code>clean -f</code></li>
      <li><code>rm -r</code> of anything outside the project, your home folder or a variable</li>
      <li>Downloads piped into a shell (<code>curl … | sh</code>), and <code>dd</code>, <code>mkfs</code>, <code>shutdown</code>, <code>reboot</code></li>
      <li>Edits to files outside the project folder or in its <code>.git</code> folder</li>
      <li>Reading hidden files in your home folder (like <code>~/.ssh</code>) and <code>.env</code> files</li>
    </ul>
    <p v-if="!draft.askForRisky" class="callout warn">Every command, file edit and read is allowed without asking, including the ones above. A mistaken command, or one planted in a file Claude reads, runs with your user's rights.</p>
  </SettingsGroup>
</template>

<style scoped>
.check { display: flex; gap: 10px; align-items: flex-start; margin: 12px 0 4px; cursor: pointer; }
.check input { margin-top: 3px; }
.hint { display: block; font-size: 13px; margin-top: 2px; }
.risky { margin: 4px 0 12px 28px; padding-left: 16px; font-size: 13px; }
.risky li { margin: 2px 0; }
</style>
