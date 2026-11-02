import { ref } from "vue";
import { defineStore } from "pinia";
import { commands } from "../bindings";
import type { CommandError, DoctorReport_Serialize } from "../bindings";
import { call } from "../lib/call";

export const useDoctorStore = defineStore("doctor", () => {
  const report = ref<DoctorReport_Serialize | null>(null);
  const loading = ref(false);
  const error = ref<CommandError | null>(null);

  /** `force=false` returns the session-cached report; `true` re-probes. */
  async function run(force: boolean) {
    loading.value = true;
    error.value = null;
    const r = await call(commands.runDoctor(force));
    if (r.ok) report.value = r.data;
    else error.value = r.error;
    loading.value = false;
    return r;
  }

  return { report, loading, error, run };
});
