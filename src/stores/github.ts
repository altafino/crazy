import { ref } from "vue";
import { defineStore } from "pinia";
import { commands, events } from "../bindings";
import type { AuthState, GhError, GithubSignInCodeEvent, RepoSummary_Serialize } from "../bindings";

/** gh sign-in state shared by the GitHub tab, the Clone dialog and the wizard (M5-D15, M5-DR4). */
export const useGithubStore = defineStore("github", () => {
  const auth = ref<AuthState | null>(null);
  const checking = ref(false);
  /** The one-time code while `gh auth login --web` waits for the browser; null otherwise. */
  const signInCode = ref<GithubSignInCodeEvent | null>(null);
  const signingIn = ref(false);
  const signInStartedAt = ref<number | null>(null);
  const error = ref<GhError | null>(null);
  const repos = ref<RepoSummary_Serialize[] | null>(null);
  const reposLoading = ref(false);
  const reposError = ref<GhError | null>(null);

  const login = () => (auth.value?.kind === "signedIn" ? auth.value.login : null);

  /** M5-D15: called when a GitHub surface opens, after sign-in and after an auth failure. */
  async function check() {
    checking.value = true;
    const r = await commands.githubAuthStatus().catch((e) => ({ status: "error" as const, error: { kind: "gh" as const, message: String(e) } }));
    if (r.status === "ok") {
      auth.value = r.data;
      error.value = null;
    } else if (r.error.kind === "ghMissing") auth.value = { kind: "ghMissing" };
    else error.value = r.error;
    checking.value = false;
    return auth.value;
  }

  /** Resolves with the final state; a Stop resolves with the previous state and no error. */
  async function signIn() {
    if (signingIn.value) return auth.value;
    signingIn.value = true;
    signInCode.value = null;
    signInStartedAt.value = Date.now();
    error.value = null;
    const unlisten = await events.githubSignInCodeEvent.listen(({ payload }) => (signInCode.value = payload));
    try {
      const r = await commands.githubSignIn().catch((e) => ({ status: "error" as const, error: { kind: "gh" as const, message: String(e) } }));
      if (r.status === "ok") auth.value = r.data;
      else if (r.error.kind !== "stopped") error.value = r.error;
    } finally {
      unlisten();
      signingIn.value = false;
      signInCode.value = null;
      signInStartedAt.value = null;
    }
    return auth.value;
  }

  async function stopSignIn() {
    await commands.githubSignInStop().catch(() => {});
  }

  async function loadRepos() {
    reposLoading.value = true;
    reposError.value = null;
    const r = await commands.githubListRepos().catch((e) => ({ status: "error" as const, error: { kind: "gh" as const, message: String(e) } }));
    if (r.status === "ok") repos.value = r.data;
    else reposError.value = r.error;
    reposLoading.value = false;
  }

  return { auth, checking, signInCode, signingIn, signInStartedAt, error, repos, reposLoading, reposError, login, check, signIn, stopSignIn, loadRepos };
});
