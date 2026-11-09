<script setup lang="ts">
import { useNavStore } from "../stores/nav";

/** Sidebar "Help": how Studio works, one screen, with buttons to the places it names. */
const nav = useNavStore();
const approvals = () => {
  nav.settingsSection = "approvals";
  nav.go("settings");
};

const TABS: [string, string][] = [
  ["Chat", "Tell Claude what to build or change. Plan first asks for a plan and changes nothing; Make changes edits the code. Each run ends with a summary and what to do next."],
  ["Plan", "The project's task list. Start a task to hand it to Claude, and tick it off when it's done."],
  ["Changes", "Every file a run changed, as a diff, with undo."],
  ["Preview", "Run the app to try it, with its log beside it."],
  ["Source", "The project's git history. Pick files, write a message and commit."],
  ["Releases", "Build an AppImage, install it to the GNOME launcher, and roll back to an earlier release. Export… makes builds for other Linux systems and Windows (needs Docker)."],
  ["GitHub", "Sign in, publish the project, and push or pull."],
  ["Database", "Look at the data your app stores, and back it up or restore it. Studio never edits it; ask Claude for changes."],
];
</script>

<template>
  <section aria-labelledby="help-h" class="help">
    <div class="view-head">
      <h1 id="help-h">Help</h1>
    </div>
    <p class="lead">AI App Studio turns a description into a real app on this computer. Claude writes ordinary source code in a folder you own, and Studio tests, packages and installs it.</p>

    <h2>Getting started</h2>
    <ol class="steps">
      <li>
        Set up Claude in Settings, AI Agents.
        <button type="button" class="link agents" @click="nav.openAgentSettings()">Open AI Agents settings</button>
      </li>
      <li>
        Check that the tools Studio needs are installed.
        <button type="button" class="link doctor" @click="nav.go('doctor')">Open Environment Doctor</button>
      </li>
      <li>On Projects, choose <strong>New Project</strong>, name it and say what it should do. Claude turns that into a plan.</li>
      <li>Try it in Preview, ask for changes in Chat, then build and install it from Releases. The installed app runs without Studio.</li>
    </ol>

    <h2>Project tabs</h2>
    <dl id="help-tabs" class="tabs-list">
      <template v-for="[name, text] in TABS" :key="name">
        <dt>{{ name }}</dt>
        <dd>{{ text }}</dd>
      </template>
    </dl>

    <h2>Approvals</h2>
    <p>
      Before Claude runs a command, edits a file or reads one outside the project, Studio shows you exactly what it wants to do and waits.
      Auto-approve answers for you in Make changes runs; Plan first always asks. Risky actions can still ask with Auto-approve on.
      <button type="button" class="link approvals" @click="approvals">Approval settings</button>
    </p>

    <h2>ArtCraft apps</h2>
    <p>
      With ArtCraft apps installed, Claude can make images, sprites, sounds and more for your app. Choose which ones with the ArtCraft button under the message.
      What they make is saved in the project's <code class="mono">assets/</code> folder.
    </p>

    <h2>Keyboard</h2>
    <dl class="keys">
      <dt><kbd>Ctrl</kbd> + <kbd>Enter</kbd></dt><dd>Send the message in Chat</dd>
      <dt><kbd>Esc</kbd></dt><dd>Back to Projects from a project, or close a popup</dd>
      <dt><kbd>←</kbd> <kbd>→</kbd></dt><dd>Move between tabs</dd>
    </dl>

    <h2>When something goes wrong</h2>
    <ul>
      <li>A build or run fails: the error has <strong>Ask Claude to fix</strong>, which hands Claude the error.</li>
      <li>A tool is missing or not found: the Environment Doctor shows what Studio sees and how to fix it.</li>
      <li>Reporting a bug: copy the build details from <button type="button" class="link about" @click="nav.go('about')">About</button> into the report.</li>
    </ul>
  </section>
</template>

<style scoped>
.help { max-width: 760px; }
.lead { max-width: 60ch; margin-top: 0; }
h2 { margin-top: 24px; }
.steps, ul { padding-left: 20px; }
.steps li, ul li { margin-bottom: 8px; }
.tabs-list, .keys { display: grid; grid-template-columns: max-content 1fr; gap: 8px 24px; margin: 0; }
dt { font-weight: 600; }
dd { margin: 0; }
kbd { font: 13px var(--mono); border: 1px solid var(--border); border-radius: 4px; padding: 1px 6px; background: var(--card); }
.link { font: inherit; background: none; border: 0; padding: 0; color: var(--accent-text); cursor: pointer; }
.link:hover { text-decoration: underline; }
.steps .link { display: block; min-height: 32px; }
</style>
