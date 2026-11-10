import { reactive } from "vue";
import { commands, events } from "../bindings";
import type { ProjectShots } from "../bindings";
import { call } from "./call";

/** Project id -> its screenshots. Shared, so a new shot shows on every view at once. */
const cache = reactive<Record<string, ProjectShots>>({});
const loading = new Set<string>();
let listening = false;

function load(id: string) {
  loading.add(id);
  // Inside a promise: screenshots are extra, a failing call must never break a card.
  void call(Promise.resolve().then(() => commands.projectScreenshots(id))).then((r) => {
    if (r.ok) cache[id] = r.data as ProjectShots;
    loading.delete(id);
  });
}

/** The cached shots; the first call for a project loads them. */
export function shotsOf(id: string): ProjectShots | null {
  if (!listening) {
    listening = true;
    void Promise.resolve()
      .then(() => events.screenshotsChangedEvent.listen(({ payload }) => load(payload.projectId)))
      .catch(() => (listening = false));
  }
  if (!(id in cache) && !loading.has(id)) load(id);
  return cache[id] ?? null;
}

/** Reloads now (the active release may have changed). */
export function reloadShots(id: string) {
  if (!loading.has(id)) load(id);
}
