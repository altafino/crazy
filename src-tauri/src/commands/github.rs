//! GitHub commands (PRD M5 step 6): sign-in, repository list and creation, clone. Thin
//! wrappers over `GhService`. Errors stay typed (`GhError`, `CloneError`) so the UI switches
//! on `kind`. The git remote operations (fetch, pull, push, ...) live in `source.rs` next to
//! the other Source commands: they share `RepoService`, its per-call construction and `dir_of`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;
use tokio::sync::watch;

use super::AppState;
use super::source::{gh_binary, repo};
use crate::domain::ImportOutcome;
use crate::services::diagnostics::spawn::run_capped;
use crate::services::github::{
    AuthState, GhError, GhService, RemoteCreated, RepoSummary, Visibility,
};
use crate::services::projects::ProjectError;

const SIGN_IN_KEY: &str = "signin";
const CONFIG_TIMEOUT: Duration = Duration::from_secs(5);
/// How many of the user's repositories the picker loads.
const REPO_LIMIT: u32 = 100;

// ------------------------------------------------------------------ events

/// `gh auth login` printed its one-time code. Sent once per sign-in, before
/// `github_sign_in` resolves; the UI shows the code and offers to open `url`.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GithubSignInCodeEvent {
    pub code: String,
    pub url: String,
}

// ------------------------------------------------------------------ errors

/// Why a clone did not produce a project: GitHub or the folder (`gh`, with the typed
/// `GhError` inside) or the import that follows a clone (`project`, a plain sentence).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum CloneError {
    Gh { error: GhError },
    Project { message: String },
}

impl From<ProjectError> for CloneError {
    fn from(e: ProjectError) -> Self {
        match e {
            ProjectError::Github(error) => Self::Gh { error },
            other => Self::Project {
                message: other.to_string(),
            },
        }
    }
}

// ----------------------------------------------------------- running operations

/// Cancel senders of the running sign-in and clones, so a second command can stop them.
/// Keys: `signin`, `clone:<spec>`.
#[derive(Clone, Default)]
pub(super) struct Cancels(Arc<Mutex<HashMap<String, watch::Sender<bool>>>>);

/// Frees the key when the operation ends, however it ends.
pub(super) struct CancelSlot {
    map: Cancels,
    key: String,
}

impl Drop for CancelSlot {
    fn drop(&mut self) {
        if let Ok(mut m) = self.map.0.lock() {
            m.remove(&self.key);
        }
    }
}

impl Cancels {
    /// `None` when `key` already runs.
    fn start(&self, key: &str) -> Option<(watch::Receiver<bool>, CancelSlot)> {
        let mut m = self.0.lock().ok()?;
        if m.contains_key(key) {
            return None;
        }
        let (tx, rx) = watch::channel(false);
        m.insert(key.to_owned(), tx);
        let slot = CancelSlot {
            map: self.clone(),
            key: key.to_owned(),
        };
        Some((rx, slot))
    }

    /// False when nothing runs under `key`.
    fn stop(&self, key: &str) -> bool {
        self.0
            .lock()
            .ok()
            .and_then(|m| m.get(key).map(|tx| tx.send_replace(true)))
            .is_some()
    }
}

fn already_running(what: &str) -> GhError {
    GhError::Gh {
        message: format!("{what} is already running."),
    }
}

// ------------------------------------------------------------------ plumbing

/// Whether git finds a `core.sshCommand` (checked in `dir` when given, else globally). Then
/// Studio leaves ssh alone instead of forcing batch mode. Asked on every call: one cheap
/// `git config`, so a changed config is seen without a restart.
async fn has_user_ssh(st: &AppState, dir: Option<&std::path::Path>) -> bool {
    let program = st
        .doctor
        .resolve_binary(
            "git",
            st.settings().tool_overrides.get("git").map(String::as_str),
        )
        .unwrap_or_else(|| "git".into());
    let mut args: Vec<String> = Vec::new();
    if let Some(d) = dir {
        args.extend(["-C".to_owned(), d.display().to_string()]);
    }
    args.extend(["config", "--get", "core.sshCommand"].map(str::to_owned));
    run_capped(&program, &args, &st.doctor.spawn_env(), CONFIG_TIMEOUT)
        .await
        .is_ok_and(|o| o.status.success() && !o.stdout.trim().is_empty())
}

async fn gh_service(st: &AppState, dir: Option<&std::path::Path>) -> GhService {
    let git = st
        .doctor
        .resolve_binary(
            "git",
            st.settings().tool_overrides.get("git").map(String::as_str),
        )
        .unwrap_or_else(|| "git".into());
    GhService::new(gh_binary(st), git, st.doctor.spawn_env(), st.db.clone())
        .with_user_ssh_config(has_user_ssh(st, dir).await)
}

// ---------------------------------------------------------------- commands

/// Sign-in state for github.com. Never fails: gh missing or unreachable are states.
#[tauri::command]
#[specta::specta]
pub(crate) async fn github_auth_status(state: State<'_, AppState>) -> Result<AuthState, GhError> {
    Ok(gh_service(&state, None).await.auth_status().await)
}

/// Runs `gh auth login --web`. The one-time code arrives as `GithubSignInCodeEvent`; this
/// resolves with the final `AuthState` once the user finished in the browser. Fails with
/// `stopped` after `github_sign_in_stop`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn github_sign_in(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<AuthState, GhError> {
    let (rx, _slot) = state
        .gh_cancels
        .start(SIGN_IN_KEY)
        .ok_or_else(|| already_running("Sign-in"))?;
    let gh = gh_service(&state, None).await;
    gh.login(
        move |c| {
            let ev = GithubSignInCodeEvent {
                code: c.code,
                url: c.url,
            };
            if let Err(e) = ev.emit(&app) {
                eprintln!("could not emit sign-in code: {e}");
            }
        },
        rx,
    )
    .await
}

/// Cancels the running sign-in. `false` when none runs.
#[tauri::command]
#[specta::specta]
pub(crate) async fn github_sign_in_stop(state: State<'_, AppState>) -> Result<bool, GhError> {
    Ok(state.gh_cancels.stop(SIGN_IN_KEY))
}

/// The user's repositories, newest first.
#[tauri::command]
#[specta::specta]
pub(crate) async fn github_list_repos(
    state: State<'_, AppState>,
) -> Result<Vec<RepoSummary>, GhError> {
    gh_service(&state, None).await.list_repos(REPO_LIMIT).await
}

/// Creates the GitHub repository for an existing project and pushes it.
///
/// Separate from `create_project` on purpose (D3/DR13): the wizard creates the project first
/// and the UI calls this only afterwards, when the user ticked "Create on GitHub". A GitHub
/// failure here can therefore never undo or block the project. The result is recorded in
/// `git_sync` either way (see `source_last_sync`).
#[tauri::command]
#[specta::specta]
pub(crate) async fn github_create_repo(
    state: State<'_, AppState>,
    project_id: String,
    name: String,
    is_private: bool,
    description: Option<String>,
) -> Result<RemoteCreated, GhError> {
    let dir = state
        .db
        .get_project(&project_id)
        .map_err(|e| GhError::Gh {
            message: e.to_string(),
        })?
        .map(|r| PathBuf::from(r.workspace_path))
        .ok_or_else(|| GhError::Gh {
            message: "Project not found.".into(),
        })?;
    let visibility = if is_private {
        Visibility::Private
    } else {
        Visibility::Public
    };
    gh_service(&state, Some(&dir))
        .await
        .create_repo(
            &repo(&state),
            &project_id,
            &dir,
            &name,
            visibility,
            description.as_deref().filter(|d| !d.trim().is_empty()),
        )
        .await
}

/// Clones `spec` (`owner/name` or a GitHub address) into `<parent>/<name>` and imports it.
/// `parent` defaults to the default projects folder. The outcome is the import's, so a
/// clone that is already in the library comes back as `needsRelink` / `needsCopyConfirm`.
#[tauri::command]
#[specta::specta]
pub(crate) async fn clone_project(
    state: State<'_, AppState>,
    spec: String,
    parent: Option<String>,
) -> Result<ImportOutcome, CloneError> {
    let key = clone_key(&spec);
    let (rx, _slot) = state.gh_cancels.start(&key).ok_or_else(|| CloneError::Gh {
        error: already_running("This clone"),
    })?;
    let parent = PathBuf::from(parent.unwrap_or_else(|| state.settings().default_projects_dir));
    let gh = gh_service(&state, None).await;
    Ok(state
        .projects()
        .clone_and_import(&gh, spec.trim(), &parent, rx)
        .await?)
}

/// Cancels the running clone of `spec` (the same text `clone_project` got). Only the temp
/// folder is removed. `false` when none runs.
#[tauri::command]
#[specta::specta]
pub(crate) async fn clone_project_stop(
    state: State<'_, AppState>,
    spec: String,
) -> Result<bool, GhError> {
    Ok(state.gh_cancels.stop(&clone_key(&spec)))
}

fn clone_key(spec: &str) -> String {
    format!("clone:{}", spec.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_reaches_the_receiver_and_the_slot_frees_the_key() {
        let c = Cancels::default();
        let (rx, slot) = c.start(SIGN_IN_KEY).unwrap();
        assert!(c.start(SIGN_IN_KEY).is_none(), "one sign-in at a time");
        assert!(c.start("clone:a/b").is_some(), "other keys are independent");
        assert!(!*rx.borrow());
        assert!(c.clone().stop(SIGN_IN_KEY), "stop through a shared handle");
        assert!(*rx.borrow());
        assert!(!c.stop("clone:never"));
        drop(slot);
        assert!(
            !c.stop(SIGN_IN_KEY),
            "finished operations cannot be stopped"
        );
        assert!(c.start(SIGN_IN_KEY).is_some());
    }

    #[test]
    fn clone_keys_ignore_surrounding_spaces() {
        assert_eq!(clone_key(" a/b "), clone_key("a/b"));
    }

    #[test]
    fn project_errors_keep_the_gh_error_typed() {
        let e: CloneError = ProjectError::Github(GhError::NameTaken).into();
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "gh");
        assert_eq!(v["error"]["kind"], "nameTaken");
        let e: CloneError = ProjectError::NotFound.into();
        assert_eq!(serde_json::to_value(&e).unwrap()["kind"], "project");
    }
}
