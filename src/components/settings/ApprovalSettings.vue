<script setup lang="ts">
import { onMounted, reactive, ref } from "vue";
import { commands } from "../../bindings";
import { call } from "../../lib/call";

const form = reactive({ autoApprove: false, askForRisky: true });
const saved = ref(false);
const failure = ref<string | null>(null);

onMounted(async () => {
  const r = await call(commands.getApprovalSettings());
  if (r.ok) Object.assign(form, r.data);
});

/** Saves on every change: two switches need no Save button. */
async function save() {
  saved.value = false;
  failure.value = null;
  const r = await call(commands.setApprovalSettings({ ...form }));
  if (r.ok) saved.value = true;
  else failure.value = r.error.message;
}
</script>

<template>
  <div class="card">
    <h2>Approvals</h2>
    <p class="muted">When Claude wants to run a command, edit a file or read one outside the project, Studio asks you first. Auto-approve answers for you in <strong>Make changes</strong> runs. <strong>Plan first</strong> always asks.</p>

    <label class="check">
      <input v-model="form.autoApprove" type="checkbox" @change="save" />
      <span>
        <strong>Auto-approve commands, file edits and reads by default</strong>
        <span class="muted hint">The Auto-approve box under the message starts in this state. You can still change it for each run.</span>
      </span>
    </label>

    <label class="check">
      <input v-model="form.askForRisky" type="checkbox" @change="save" />
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
    <p v-if="!form.askForRisky" class="callout warn">Every command, file edit and read is allowed without asking, including the ones above. A mistaken command, or one planted in a file Claude reads, runs with your user's rights.</p>

    <p v-if="failure" class="callout danger" role="alert">{{ failure }}</p>
    <p aria-live="polite" class="muted">{{ saved ? "Approval settings saved." : "" }}</p>
  </div>
</template>

<style scoped>
.card { background: var(--card); border: 1px solid var(--border); border-radius: var(--radius); padding: var(--space); margin-bottom: var(--space); }
.check { display: flex; gap: 10px; align-items: flex-start; margin: 12px 0 4px; cursor: pointer; }
.check input { margin-top: 3px; }
.hint { display: block; font-size: 13px; margin-top: 2px; }
.risky { margin: 4px 0 12px 28px; padding-left: 16px; font-size: 13px; }
.risky li { margin: 2px 0; }
</style>
