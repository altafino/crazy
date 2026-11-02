import { ref } from "vue";
import { defineStore } from "pinia";

export type View = "projects" | "doctor" | "settings";

export const useNavStore = defineStore("nav", () => {
  const view = ref<View>("projects");
  function go(v: View) {
    view.value = v;
  }
  return { view, go };
});
