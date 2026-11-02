import { ref } from "vue";
import { defineStore } from "pinia";

export type Toast = { id: number; text: string; action?: { label: string; run: () => void } };

/** One toast at a time; a new one replaces the old (DESIGN.md "Toast", M3-D38). */
export const useToastStore = defineStore("toast", () => {
  const current = ref<Toast | null>(null);
  let next = 1;
  function show(text: string, action?: Toast["action"]) {
    current.value = { id: next++, text, action };
  }
  function dismiss(id?: number) {
    if (id === undefined || current.value?.id === id) current.value = null;
  }
  return { current, show, dismiss };
});
