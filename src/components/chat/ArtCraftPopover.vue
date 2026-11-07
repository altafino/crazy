<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { Shapes } from "lucide-vue-next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { commands } from "../../bindings";
import type { CraftAppsView, InstallKind } from "../../bindings";
import { useNavStore } from "../../stores/nav";
import { useToastStore } from "../../stores/toast";

/** Composer "ArtCraft · N" button and its popover (design D18, D21, D23, D26): which ArtCraft
 * apps Claude may use in this project. Every change saves at once. */
const props = defineProps<{ projectId: string; projectName: string; disabled?: boolean }>();

const nav = useNavStore();
const toast = useToastStore();
const view = ref<CraftAppsView | null>(null);
const open = ref(false);
const btn = ref<HTMLButtonElement | null>(null);
const pop = ref<HTMLElement | null>(null);

const KIND: Record<InstallKind, string> = { package: "Package", flatpak: "Flatpak", appImage: "AppImage" };
const ticked = computed(() => view.value?.apps.filter((a) => a.ticked).map((a) => a.id) ?? []);
const label = computed(() => (view.value?.enabled ? `ArtCraft · ${ticked.value.length}` : "ArtCraft"));

async function load(id: string) {
  const r = await commands.craftApps(id);
  if (r.status === "ok" && id === props.projectId) view.value = r.data;
}
onMounted(() => void load(props.projectId));
watch(
  () => props.projectId,
  (id) => {
    open.value = false;
    view.value = null;
    void load(id);
  },
);

async function save(apps: string[]) {
  const r = await commands.setCraftApps(props.projectId, apps);
  if (r.status === "ok") view.value = r.data;
  else toast.show(`Couldn't save ArtCraft apps: ${r.error.message}`);
}
function toggleApp(id: string, on: boolean) {
  void save(on ? [...ticked.value, id] : ticked.value.filter((a) => a !== id));
}
function toggleAll() {
  const v = view.value!;
  // ponytail: no game apps installed → the first usable one, so the switch can still turn on.
  const first = v.apps.filter((a) => !a.missing).slice(0, 1).map((a) => a.id);
  void save(v.enabled ? [] : v.defaults.length ? v.defaults : first);
}

async function toggle() {
  open.value = !open.value;
  if (!open.value) return;
  await nextTick();
  pop.value?.querySelector<HTMLElement>("[role='switch']")?.focus();
}
function close(refocus = false) {
  open.value = false;
  if (refocus) btn.value?.focus();
}
function onKey(e: KeyboardEvent) {
  if (e.key !== "Escape") return;
  e.preventDefault(); // not "back to Projects" (ProjectView)
  close(true);
}
function onOutside(e: MouseEvent) {
  const t = e.target as Node;
  if (open.value && !pop.value?.contains(t) && !btn.value?.contains(t)) close();
}
onMounted(() => window.addEventListener("mousedown", onOutside));
onUnmounted(() => window.removeEventListener("mousedown", onOutside));

const getIt = () => openUrl("https://getartcraft.com/apps").catch(() => {});
function toDoctor() {
  close();
  nav.go("doctor");
}
</script>

<template>
  <span v-if="view?.apps.length" class="wrap">
    <button
      ref="btn" type="button" class="btn craft" aria-haspopup="dialog" :aria-expanded="open" :disabled="disabled"
      @click="toggle"
    >
      <Shapes :size="16" :stroke-width="1.75" aria-hidden="true" /><span class="tnum">{{ label }}</span>
    </button>
    <div v-if="open" ref="pop" class="pop" role="dialog" :aria-label="`ArtCraft apps for ${projectName}`" @keydown="onKey">
      <label class="switch-l">
        <button type="button" class="switch" role="switch" :aria-checked="view.enabled" @click="toggleAll"><span class="knob" /></button>
        <span>Use ArtCraft apps in this project</span>
      </label>
      <fieldset :class="{ off: !view.enabled }">
        <legend class="caption">Apps Claude can use</legend>
        <label v-for="a in view.apps" :key="a.id" class="app">
          <input type="checkbox" :checked="a.ticked" @change="toggleApp(a.id, ($event.target as HTMLInputElement).checked)" />
          <span>
            <span class="name">{{ a.name }}</span>
            <span class="caption">{{ a.purpose }}<template v-if="a.kind"> · {{ KIND[a.kind] }} {{ a.version }}</template><template v-if="a.asksEachAction"> · asks before each action</template></span>
          </span>
        </label>
      </fieldset>
      <ul v-if="view.notInstalled.length" class="get-list">
        <li v-for="a in view.notInstalled" :key="a.id">
          <span><span class="name">{{ a.name }}</span> <span class="caption">— {{ a.purpose }}</span></span>
          <button type="button" class="link get" @click="getIt">Get it</button>
        </li>
      </ul>
      <p v-if="view.notWired.length" class="caption">Found but not usable yet: {{ view.notWired.join(", ") }}</p>
      <p class="caption">
        Claude saves what it makes in assets/. Studio's undo doesn't cover files over 50 MB or files git ignores.
        <button type="button" class="link doctor" @click="toDoctor">Details in Doctor</button>
      </p>
    </div>
  </span>
</template>

<style scoped>
.wrap { position: relative; display: inline-flex; }
.craft { display: inline-flex; align-items: center; gap: 6px; }
/* D26: opens above its button; same surface and shadow as the ⋮ menu, with a caret. */
.pop { position: absolute; bottom: calc(100% + 10px); left: 0; z-index: 10; width: min(360px, calc(100vw - 32px)); max-height: 60vh; overflow: auto; background: var(--card); border: 1px solid var(--border); border-radius: 10px; padding: 16px; box-shadow: 0 4px 16px rgb(0 0 0 / 0.2); display: flex; flex-direction: column; gap: 12px; }
.wrap:has(.pop)::after { content: ""; position: absolute; bottom: calc(100% + 4px); left: 20px; width: 10px; height: 10px; background: var(--card); border-right: 1px solid var(--border); border-bottom: 1px solid var(--border); transform: rotate(45deg); z-index: 11; }
.switch-l { font-size: 15px; }
fieldset { border: 0; margin: 0; padding: 0; display: flex; flex-direction: column; }
fieldset.off { opacity: 0.6; }
legend { padding: 0; margin-bottom: 4px; }
.app { display: flex; gap: 8px; align-items: flex-start; min-height: 40px; padding: 4px 0; cursor: pointer; font-weight: 400; }
.app input { margin-top: 3px; }
.app > span { display: flex; flex-direction: column; }
.name { font-weight: 600; }
.caption { color: var(--muted); font-size: 13px; margin: 0; }
.get-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 4px; }
.get-list li { display: flex; justify-content: space-between; align-items: center; gap: 8px; min-height: 32px; }
.link { font: inherit; background: none; border: 0; padding: 0; min-height: 32px; color: var(--accent-text); cursor: pointer; }
.link:hover { text-decoration: underline; }
</style>
