import { ref } from "vue";
import { defineStore } from "pinia";
import { commands } from "../bindings";
import type { UpdateStatus } from "../bindings";
import { call } from "../lib/call";
import { useNavStore } from "./nav";
import { useToastStore } from "./toast";

const DAY_MS = 24 * 60 * 60 * 1000;

/** Studio's own updates: the check on start, every 24 hours and from About, and the install. */
export const useUpdateStore = defineStore("update", () => {
  const status = ref<UpdateStatus | null>(null);
  const state = ref<"idle" | "checking" | "installing" | "installed">("idle");
  const error = ref<string | null>(null);

  /** Asks GitHub. While a check or a download runs, it answers with what is already known. */
  async function check(): Promise<UpdateStatus | null> {
    if (state.value === "checking" || state.value === "installing") return status.value;
    const before = state.value;
    state.value = "checking";
    error.value = null;
    // Inside a promise, so a failing call becomes a message instead of an exception.
    const r = await call<UpdateStatus>(Promise.resolve().then(() => commands.checkForUpdate()));
    state.value = before;
    if (r.ok) status.value = r.data;
    else error.value = r.error.message;
    return r.ok ? r.data : null;
  }

  let announced = "";
  /** The background check: silent when it fails or finds nothing, and each version is offered once. */
  async function auto() {
    const s = await check();
    error.value = null;
    if (!s?.newer || s.latest === announced || state.value === "installed") return;
    announced = s.latest;
    useToastStore().show(`Version ${s.latest} is available`, { label: "Update", run: () => useNavStore().go("about") });
  }

  let timer: ReturnType<typeof setInterval> | undefined;
  /** The Settings switch: on checks now and every 24 hours while Studio runs; off stops that. */
  const autoOn = ref(false);
  function setAuto(on: boolean) {
    autoOn.value = on;
    if (!on) {
      clearInterval(timer);
      timer = undefined;
    } else if (!timer) {
      void auto();
      timer = setInterval(() => void auto(), DAY_MS);
    }
  }

  async function install() {
    const s = status.value;
    if (!s?.newer || !s.canInstall || state.value !== "idle") return;
    state.value = "installing";
    error.value = null;
    const r = await call(commands.installUpdate(s.latest));
    state.value = r.ok ? "installed" : "idle";
    if (!r.ok) error.value = r.error.message;
  }

  return { status, state, error, autoOn, check, auto, setAuto, install };
});
