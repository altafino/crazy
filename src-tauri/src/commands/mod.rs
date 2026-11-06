//! Tauri command boundary: thin async wrappers that call services and map errors to
//! user-facing messages. Disk and DB work runs on the blocking pool (PRD 10: no UI blocking).

use crate::domain::{FieldError, ImportOutcome, NewProjectRequest, ProjectSummary};
use crate::persistence::Db;
use crate::platform::paths::StudioPaths;
use crate::services::agents::claude::ClaudeAdapter;
use crate::services::appdb::{AppDb, AppDbError};
use crate::services::builds::{BuildDeps, BuildService, SMOKE_AFTER, TauriAppImageAdapter};
use crate::services::diagnostics::{DiagnosticsService, DoctorReport};
use crate::services::git::checkpoint::CheckpointStore;
use crate::services::git::repo::SyncRegistry;
use crate::services::git::{GitRunner, SnapshotStore};
use crate::services::install::{Installer, XdgLauncher};
use crate::services::jobs::JobSupervisor;
use crate::services::preview::{PreviewService, Toolchain};
use crate::services::projects::plan::{self, Plan};
use crate::services::projects::{GitCmd, ProjectError, ProjectService};
use crate::services::settings::{self, Settings};
use serde::Serialize;
use specta::Type;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

mod agents;
mod appdb;
mod attachments;
mod builds;
mod exports;
mod github;
mod install;
mod preview;
mod source;
pub(crate) use agents::forward_events;
pub(crate) use builds::forward_build_events;
pub(crate) use preview::forward_preview_events;

#[derive(Clone)]
pub(crate) struct AppState {
    db: Arc<Db>,
    paths: Arc<StudioPaths>,
    template_dir: PathBuf,
    home: Option<PathBuf>,
    doctor: Arc<DiagnosticsService>,
    snapshots: Arc<SnapshotStore>,
    jobs: JobSupervisor<ClaudeAdapter>,
    appdb: Arc<AppDb>,
    preview: PreviewService,
    builds: BuildService,
    installer: Arc<Installer>,
    toolchain: Arc<preview::AppToolchain>,
    /// Running git network operations, shared by every per-call `RepoService`.
    syncs: SyncRegistry,
    /// Cancel senders of the running GitHub sign-in and clones.
    gh_cancels: github::Cancels,
}

impl AppState {
    pub(crate) fn new(db: Db, paths: StudioPaths, template_dir: PathBuf) -> Self {
        let db = Arc::new(db);
        let doctor = Arc::new(DiagnosticsService::new());
        let git_override = settings::load(&db, &paths.default_projects_dir)
            .ok()
            .and_then(|s| s.tool_overrides.get("git").cloned());
        let program = doctor
            .resolve_binary("git", git_override.as_deref())
            .unwrap_or_else(|| "git".into());
        let checkpoints = Arc::new(CheckpointStore {
            program: program.clone(),
            env: doctor.spawn_env(),
        });
        let snapshots = Arc::new(SnapshotStore::new(
            &paths.data_dir,
            GitRunner {
                program,
                env: doctor.spawn_env(),
            },
        ));
        let jobs = JobSupervisor::new(
            db.clone(),
            snapshots.clone(),
            checkpoints.clone(),
            ClaudeAdapter::default(),
            paths.data_dir.clone(),
        );
        let appdb = Arc::new(AppDb::new(db.clone(), &paths).with_slots(jobs.slots()));
        let toolchain = Arc::new(preview::AppToolchain {
            doctor: doctor.clone(),
            db: db.clone(),
            default_projects_dir: paths.default_projects_dir.clone(),
        });
        let preview = PreviewService::new(
            db.clone(),
            jobs.slots(),
            toolchain.clone(),
            paths.data_dir.clone(),
        );
        let builds = BuildService::new(BuildDeps {
            db: db.clone(),
            slots: jobs.slots(),
            toolchain: toolchain.clone(),
            adapter: Arc::new(TauriAppImageAdapter),
            snapshots: snapshots.clone(),
            checkpoints,
            paths: paths.clone(),
            smoke_after: SMOKE_AFTER,
        });
        let gio = toolchain
            .resolve_binary("gio")
            .unwrap_or_else(|| "gio".into());
        let installer = Arc::new(Installer::new(
            paths.clone(),
            db.clone(),
            Arc::new(XdgLauncher::new(gio)),
        ));
        Self {
            db,
            paths: Arc::new(paths),
            template_dir,
            home: std::env::var_os("HOME").map(PathBuf::from),
            doctor,
            snapshots,
            jobs,
            appdb,
            preview,
            builds,
            installer,
            toolchain,
            syncs: SyncRegistry::default(),
            gh_cancels: github::Cancels::default(),
        }
    }

    pub(crate) fn builds(&self) -> &BuildService {
        &self.builds
    }

    pub(crate) fn appdb(&self) -> &AppDb {
        &self.appdb
    }

    pub(crate) fn installer(&self) -> &Installer {
        &self.installer
    }

    /// The resolved PATH environment Studio gives every child process.
    pub(crate) fn toolchain_env(&self) -> Vec<(String, String)> {
        self.toolchain.spawn_env()
    }

    pub(crate) fn jobs(&self) -> &JobSupervisor<ClaudeAdapter> {
        &self.jobs
    }

    pub(crate) fn preview(&self) -> &PreviewService {
        &self.preview
    }

    pub(crate) fn settings(&self) -> Settings {
        // A broken settings row must not block project work; fall back to defaults.
        settings::load(&self.db, &self.paths.default_projects_dir).unwrap_or_else(|_| Settings {
            default_projects_dir: self.paths.default_projects_dir.display().to_string(),
            tool_overrides: Default::default(),
        })
    }

    /// Built per call so git always runs with the latest resolved PATH and override.
    pub(crate) fn projects(&self) -> ProjectService {
        let settings = self.settings();
        let program = self
            .doctor
            .resolve_binary(
                "git",
                settings.tool_overrides.get("git").map(String::as_str),
            )
            .unwrap_or_else(|| "git".into());
        ProjectService::new(
            self.db.clone(),
            self.template_dir.clone(),
            self.paths.data_dir.clone(),
            GitCmd {
                program,
                env: self.doctor.spawn_env(),
            },
        )
    }

    pub(crate) async fn doctor_report(&self, force: bool) -> DoctorReport {
        let overrides: HashMap<String, String> =
            self.settings().tool_overrides.into_iter().collect();
        self.doctor.run(&overrides, force).await
    }
}

/// Error shape the UI receives: a message, plus per-field problems for forms.
#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandError {
    message: String,
    field_errors: Vec<FieldError>,
}

impl CommandError {
    fn msg(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            field_errors: vec![],
        }
    }
}

impl From<AppDbError> for CommandError {
    fn from(e: AppDbError) -> Self {
        Self::msg(e.to_string())
    }
}

impl From<ProjectError> for CommandError {
    fn from(e: ProjectError) -> Self {
        match e {
            ProjectError::Validation(field_errors) => Self {
                message: "Please fix the highlighted fields.".into(),
                field_errors,
            },
            other => Self::msg(other.to_string()),
        }
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, CommandError> + Send + 'static,
) -> Result<T, CommandError> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| CommandError::msg(format!("internal error: {e}")))?
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn list_projects(
    state: State<'_, AppState>,
) -> Result<Vec<ProjectSummary>, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.projects().list()?)).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn validate_new_project(
    state: State<'_, AppState>,
    request: NewProjectRequest,
) -> Result<Vec<FieldError>, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.projects().validate_new(&request))).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn create_project(
    state: State<'_, AppState>,
    request: NewProjectRequest,
) -> Result<ProjectSummary, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.projects().create(&request)?)).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn import_project(
    state: State<'_, AppState>,
    path: String,
) -> Result<ImportOutcome, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.projects().import(&PathBuf::from(path))?)).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn relink_project(
    state: State<'_, AppState>,
    id: String,
    path: String,
) -> Result<ProjectSummary, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.projects().relink(&id, &PathBuf::from(path))?)).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn import_project_as_copy(
    state: State<'_, AppState>,
    path: String,
) -> Result<ProjectSummary, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.projects().import_as_copy(&PathBuf::from(path))?)).await
}

/// The project's app icon as a `data:` URL; null when it has none.
#[tauri::command]
#[specta::specta]
pub(crate) async fn project_icon(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<String>, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.projects().icon(&id)?)).await
}

/// The project's plan; null when it has none.
#[tauri::command]
#[specta::specta]
pub(crate) async fn project_plan(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<Plan>, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.projects().read_plan(&id)?)).await
}

/// Parses plan text without saving it (the editor's live checklist preview).
#[tauri::command]
#[specta::specta]
pub(crate) fn parse_plan(text: String) -> Plan {
    plan::parse(&text)
}

/// Replaces the plan file. Refused when `text` has no checklist lines.
#[tauri::command]
#[specta::specta]
pub(crate) async fn save_project_plan(
    state: State<'_, AppState>,
    id: String,
    text: String,
) -> Result<Plan, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.projects().save_plan(&id, &text)?)).await
}

/// Ticks or unticks the first task titled `text` that is not in that state yet.
#[tauri::command]
#[specta::specta]
pub(crate) async fn set_plan_task(
    state: State<'_, AppState>,
    id: String,
    text: String,
    done: bool,
) -> Result<Plan, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.projects().set_plan_task(&id, &text, done)?)).await
}

/// The picture at `path` as it would become an app icon (128 px `data:` URL), for the New Project
/// wizard. Fails with the reason the picture can't be used.
#[tauri::command]
#[specta::specta]
pub(crate) async fn preview_icon(path: String) -> Result<String, CommandError> {
    blocking(move || {
        crate::services::projects::icon::preview(&PathBuf::from(path))
            .map_err(|e| CommandError::msg(e.to_string()))
    })
    .await
}

/// Replaces the app icon with every size generated from the picture at `path` (PNG, JPEG or
/// WebP, at least 256 px). Returns the new icon as a `data:` URL.
#[tauri::command]
#[specta::specta]
pub(crate) async fn set_project_icon(
    state: State<'_, AppState>,
    id: String,
    path: String,
) -> Result<String, CommandError> {
    let st = state.inner().clone();
    blocking(move || Ok(st.projects().set_icon(&id, &PathBuf::from(path))?)).await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn get_settings(state: State<'_, AppState>) -> Result<Settings, CommandError> {
    let st = state.inner().clone();
    blocking(move || {
        settings::load(&st.db, &st.paths.default_projects_dir)
            .map_err(|e| CommandError::msg(e.to_string()))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn save_settings(
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, CommandError> {
    let st = state.inner().clone();
    blocking(move || {
        settings::save(&st.db, settings, st.home.as_deref())
            .map_err(|e| CommandError::msg(e.to_string()))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub(crate) async fn run_doctor(
    state: State<'_, AppState>,
    force: bool,
) -> Result<DoctorReport, CommandError> {
    let st = state.inner().clone();
    Ok(st.doctor_report(force).await)
}

pub(crate) fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            list_projects,
            validate_new_project,
            create_project,
            import_project,
            relink_project,
            import_project_as_copy,
            project_icon,
            set_project_icon,
            preview_icon,
            project_plan,
            parse_plan,
            save_project_plan,
            set_plan_task,
            get_settings,
            save_settings,
            run_doctor,
            agents::list_agent_profiles,
            agents::save_agent_profile,
            agents::delete_agent_profile,
            agents::probe_agent_profile,
            agents::propose_agent_profile,
            agents::start_run,
            agents::cancel_run,
            agents::answer_approval,
            agents::allow_and_retry,
            agents::continue_plan,
            agents::list_sessions,
            agents::get_session_events,
            agents::run_diff,
            agents::run_file_diff,
            agents::revert_run,
            agents::delete_all_snapshots,
            attachments::attachment_types,
            attachments::attach_files,
            attachments::attach_pasted,
            attachments::clipboard_file_paths,
            agents::get_approval_settings,
            agents::set_approval_settings,
            attachments::remove_attachment,
            attachments::list_run_attachments,
            attachments::get_attachment_limits,
            attachments::set_attachment_limits,
            source::source_status,
            source::source_file_diff,
            source::source_commit,
            source::source_get_identity,
            source::source_set_identity,
            source::source_secret_check,
            source::source_list_branches,
            source::source_create_branch,
            source::source_switch_branch,
            source::source_restore_preview,
            source::source_restore,
            source::source_undo_restore,
            source::source_history,
            source::source_init_git,
            source::source_sync_commits,
            source::source_fetch,
            source::source_pull,
            source::source_merge_upstream,
            source::source_push,
            source::source_abort_merge,
            source::source_stop_sync,
            source::source_mark_resolved,
            source::source_last_sync,
            github::github_auth_status,
            github::github_sign_in,
            github::github_sign_in_stop,
            github::github_list_repos,
            github::github_create_repo,
            github::clone_project,
            github::clone_project_stop,
            preview::start_preview,
            preview::stop_preview,
            preview::restart_preview,
            preview::get_preview_status,
            preview::get_preview_log,
            preview::get_preview_settings,
            preview::save_preview_settings,
            builds::start_build,
            builds::stop_build,
            builds::get_build_status,
            builds::get_build_log,
            builds::next_build_version,
            builds::list_releases,
            builds::delete_release,
            exports::export_readiness,
            exports::start_export,
            exports::list_release_exports,
            exports::export_cache_size,
            exports::clear_export_caches,
            exports::withdraw_export_license,
            appdb::get_database_limits,
            appdb::set_database_limits,
            appdb::appdb_overview,
            appdb::appdb_rows,
            appdb::appdb_guard,
            appdb::appdb_list_backups,
            appdb::appdb_backup,
            appdb::appdb_delete_backup,
            appdb::appdb_restore_backup,
            appdb::appdb_copy_installed_to_preview,
            appdb::appdb_reset_preview,
            appdb::appdb_rollback_check,
            install::get_install_state,
            install::activate_release,
            install::uninstall_app,
            install::launch_installed,
            install::repair_launcher,
            install::list_installed_apps,
        ])
        .events(tauri_specta::collect_events![
            agents::AgentRunEvent,
            preview::PreviewStatusEvent,
            preview::PreviewLogEvent,
            builds::BuildStatusEvent,
            builds::BuildLogEvent,
            github::GithubSignInCodeEvent
        ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed src/bindings.ts must match the Rust commands (decision D12).
    /// Regenerate with: UPDATE_BINDINGS=1 cargo test bindings_are_fresh
    #[test]
    fn bindings_are_fresh() {
        let committed = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/bindings.ts");
        let tmp = tempfile::tempdir().unwrap();
        let fresh = tmp.path().join("bindings.ts");
        specta_builder()
            .export(specta_typescript::Typescript::default(), &fresh)
            .unwrap();
        let fresh_text = std::fs::read_to_string(&fresh).unwrap();
        if std::env::var_os("UPDATE_BINDINGS").is_some() {
            std::fs::write(&committed, &fresh_text).unwrap();
            return;
        }
        let committed_text = std::fs::read_to_string(&committed).unwrap_or_default();
        assert!(
            committed_text == fresh_text,
            "src/bindings.ts is stale; run: UPDATE_BINDINGS=1 cargo test bindings_are_fresh"
        );
    }
}
