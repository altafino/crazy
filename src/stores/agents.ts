import { ref } from "vue";
import { defineStore } from "pinia";
import { commands } from "../bindings";
import type { AgentProfile, ProposedProfile } from "../bindings";
import { call } from "../lib/call";

export const useAgentsStore = defineStore("agents", () => {
  const profiles = ref<AgentProfile[]>([]);
  const proposal = ref<ProposedProfile | null>(null);
  const loaded = ref(false);

  /** Loads profiles; with none saved, asks the backend for a D33 proposal (never saved here). */
  async function load() {
    const r = await call(commands.listAgentProfiles());
    if (r.ok) profiles.value = r.data;
    loaded.value = true;
    if (r.ok && !r.data.length) {
      const p = await call(commands.proposeAgentProfile());
      proposal.value = p.ok ? p.data : null;
    }
  }

  return { profiles, proposal, loaded, load };
});
