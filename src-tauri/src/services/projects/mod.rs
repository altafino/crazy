//! ProjectService: create, import, list, relink. Sole writer of project rows and
//! `.ai-app-studio/project.json`.

pub(crate) mod tauri;
mod template;
#[cfg(test)]
mod tests;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use crate::domain::{
    FieldError, ImportOutcome, MetaError, NewProjectRequest, ProjectMeta, ProjectState,
    ProjectSummary, TemplateId,
};
use crate::persistence::{Db, DbError, NewProject, PendingOp, ProjectRow};
use crate::security::{self, Invalid};

const TEMP_PREFIX: &str = ".ai-app-studio-creating-";
const TAURI_CONF: &str = "src-tauri/tauri.conf.json";

#[derive(Debug, thiserror::Error)]
pub(crate) enum ProjectError {
    #[error("{}", .0.iter().map(|e| e.message.as_str()).collect::<Vec<_>>().join("; "))]
    Validation(Vec<FieldError>),
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Meta(#[from] MetaError),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("git failed: {0}")]
    Git(String),
    #[error("template problem: {0}")]
    Template(String),
    #[error("{0}")]
    BadFolder(String),
    #[error("project not found")]
    NotFound,
}

/// How to run git. `env` is the complete environment (inherited env is cleared),
/// so AppImage launcher variables never reach git (decision D17).
pub(crate) struct GitCmd {
    pub program: PathBuf,
    pub env: Vec<(String, String)>,
}

impl GitCmd {
    fn run(&self, dir: &Path, args: &[&str]) -> Result<(), ProjectError> {
        let out = Command::new(&self.program)
            .args(args)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .current_dir(dir)
            .output()
            .map_err(|e| ProjectError::Git(format!("{}: {e}", self.program.display())))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(ProjectError::Git(
                String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            ))
        }
    }
}

pub(crate) struct ProjectService {
    db: Arc<Db>,
    template_dir: PathBuf,
    studio_data_dir: PathBuf,
    git: GitCmd,
}

struct Plan {
    name: String,
    slug: String,
    identifier: String,
    parent: PathBuf,
    target: PathBuf,
}

fn field(field: &str, e: Invalid) -> FieldError {
    FieldError {
        field: field.into(),
        message: e.0,
    }
}

impl ProjectService {
    pub(crate) fn new(
        db: Arc<Db>,
        template_dir: PathBuf,
        studio_data_dir: PathBuf,
        git: GitCmd,
    ) -> Self {
        Self {
            db,
            template_dir,
            studio_data_dir,
            git,
        }
    }

    /// All problems with a create request at once; empty when it is acceptable.
    pub(crate) fn validate_new(&self, req: &NewProjectRequest) -> Vec<FieldError> {
        self.plan(req).err().unwrap_or_default()
    }

    fn plan(&self, req: &NewProjectRequest) -> Result<Plan, Vec<FieldError>> {
        let mut errors = Vec::new();
        let name = security::validate_name(&req.name)
            .map_err(|e| errors.push(field("name", e)))
            .ok();

        let slug = match req.slug.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => security::validate_slug(s).map(|()| s.to_owned()),
            None => security::slugify(&req.name),
        }
        .map_err(|e| errors.push(field("slug", e)))
        .ok();
        if let Some(slug) = &slug {
            match self.db.get_project_by_slug(slug) {
                Ok(None) => {}
                Ok(Some(_)) => errors.push(field(
                    "slug",
                    Invalid("a project with this slug already exists".into()),
                )),
                Err(e) => errors.push(field("slug", Invalid(e.to_string()))),
            }
        }

        let identifier = match (
            req.identifier
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            &slug,
        ) {
            (Some(id), _) => security::validate_identifier(id)
                .map(|()| id.to_owned())
                .map_err(|e| errors.push(field("identifier", e)))
                .ok(),
            (None, Some(slug)) => Some(security::default_identifier(slug)),
            (None, None) => None,
        };

        let home = std::env::var_os("HOME").map(PathBuf::from);
        let parent = security::expand_tilde(req.parent_dir.trim(), home.as_deref())
            .and_then(|p| security::validate_parent_dir(&p).map(|()| p))
            .map_err(|e| errors.push(field("parentDir", e)))
            .ok();

        let target = match (&parent, &slug) {
            (Some(parent), Some(slug)) => {
                let target = parent.join(slug);
                match self
                    .workspaces()
                    .map(|ws| security::validate_target(&target, &self.studio_data_dir, &ws))
                {
                    Ok(Ok(())) => Some(target),
                    Ok(Err(e)) => {
                        errors.push(field("slug", e));
                        None
                    }
                    Err(e) => {
                        errors.push(field("slug", Invalid(e.to_string())));
                        None
                    }
                }
            }
            _ => None,
        };

        match (errors.is_empty(), name, slug, identifier, parent, target) {
            (true, Some(name), Some(slug), Some(identifier), Some(parent), Some(target)) => {
                Ok(Plan {
                    name,
                    slug,
                    identifier,
                    parent,
                    target,
                })
            }
            _ => Err(errors),
        }
    }

    fn workspaces(&self) -> Result<Vec<PathBuf>, DbError> {
        Ok(self
            .db
            .list_projects()?
            .into_iter()
            .map(|r| PathBuf::from(r.workspace_path))
            .collect())
    }

    pub(crate) fn create(&self, req: &NewProjectRequest) -> Result<ProjectSummary, ProjectError> {
        let plan = self.plan(req).map_err(ProjectError::Validation)?;
        let meta = ProjectMeta::new(
            &plan.name,
            &plan.slug,
            TemplateId::TauriVue,
            &plan.identifier,
        );
        let temp = plan.parent.join(format!("{TEMP_PREFIX}{}", meta.id));

        self.db.insert_project(&NewProject {
            id: meta.id.clone(),
            name: meta.name.clone(),
            slug: meta.slug.clone(),
            template_id: meta.template_id,
            app_id: meta.app_id.clone(),
            workspace_path: plan.target.to_string_lossy().into_owned(),
            pending_op: Some(PendingOp::Create),
        })?;

        // Only the temp folder is ever removed on failure; the target is a user-picked
        // path and is only touched by the final rename, which replaces an empty dir.
        let built = self
            .build(&temp, &meta, req.init_git)
            .and_then(|()| fs::rename(&temp, &plan.target).map_err(Into::into));
        if let Err(e) = built {
            let _ = fs::remove_dir_all(&temp);
            let _ = self.db.delete_project(&meta.id);
            return Err(e);
        }
        self.db.set_pending(&meta.id, None)?;
        self.summary_of(&meta.id)
    }

    fn build(&self, dir: &Path, meta: &ProjectMeta, init_git: bool) -> Result<(), ProjectError> {
        template::copy_template(&self.template_dir, dir)?;
        template::substitute(
            dir,
            &template::Params {
                name: &meta.name,
                slug: &meta.slug,
                identifier: &meta.app_id,
                cargo_name: &security::cargo_name(&meta.slug),
            },
        )?;
        meta.write(dir)?;
        if init_git {
            self.git
                .run(dir, &["init", "-q", "--initial-branch=main"])?;
            self.git.run(dir, &["add", "-A"])?;
            self.git.run(
                dir,
                &[
                    "-c",
                    "user.name=AI App Studio",
                    "-c",
                    "user.email=studio@localhost",
                    "commit",
                    "-q",
                    "-m",
                    "Initial commit",
                ],
            )?;
        }
        Ok(())
    }

    pub(crate) fn import(&self, path: &Path) -> Result<ImportOutcome, ProjectError> {
        let dir = canonical_dir(path)?;
        let dir_str = dir.to_string_lossy().into_owned();
        if let Some(row) = self.db.get_project_by_path(&dir_str)? {
            return Ok(ImportOutcome::AlreadyInLibrary {
                project: summarize(row),
            });
        }
        let meta = match ProjectMeta::read(&dir)? {
            Some(meta) => {
                if let Some(existing) = self.db.get_project(&meta.id)? {
                    let existing = summarize(existing);
                    return Ok(if Path::new(&existing.workspace_path).exists() {
                        ImportOutcome::NeedsCopyConfirm {
                            existing,
                            new_path: dir_str,
                        }
                    } else {
                        ImportOutcome::NeedsRelink {
                            existing,
                            new_path: dir_str,
                        }
                    });
                }
                meta
            }
            None => {
                let meta = self.detect(&dir)?;
                meta.write(&dir)?;
                meta
            }
        };
        // The DB slug is only a handle and must be unique; project.json stays untouched.
        let slug = self.unique_slug(&meta.slug)?;
        self.db.insert_project(&NewProject {
            id: meta.id.clone(),
            name: meta.name,
            slug,
            template_id: meta.template_id,
            app_id: meta.app_id,
            workspace_path: dir_str,
            pending_op: None,
        })?;
        Ok(ImportOutcome::Imported {
            project: self.summary_of(&meta.id)?,
        })
    }

    /// Builds metadata for a folder that has no project.json.
    fn detect(&self, dir: &Path) -> Result<ProjectMeta, ProjectError> {
        let folder = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let conf = dir.join(TAURI_CONF);
        let (template, name, identifier) = if conf.is_file() {
            let (name, id) = read_conf(&conf)?;
            (TemplateId::TauriVue, name, id)
        } else {
            (TemplateId::Existing, None, None)
        };
        let name = display_name(name.as_deref().unwrap_or(&folder));
        let slug =
            self.unique_slug(&security::slugify(&name).unwrap_or_else(|_| "project".into()))?;
        let identifier = identifier.unwrap_or_else(|| security::default_identifier(&slug));
        Ok(ProjectMeta::new(&name, &slug, template, &identifier))
    }

    fn unique_slug(&self, base: &str) -> Result<String, DbError> {
        let mut slug = base.to_owned();
        let mut n = 2;
        while self.db.get_project_by_slug(&slug)?.is_some() {
            slug = format!("{base}-{n}");
            n += 1;
        }
        Ok(slug)
    }

    pub(crate) fn relink(&self, id: &str, new_path: &Path) -> Result<ProjectSummary, ProjectError> {
        let mut row = self.db.get_project(id)?.ok_or(ProjectError::NotFound)?;
        let dir = canonical_dir(new_path)?;
        let dir_str = dir.to_string_lossy().into_owned();
        if self
            .db
            .get_project_by_path(&dir_str)?
            .is_some_and(|r| r.id != id)
        {
            return Err(ProjectError::BadFolder(
                "that folder is already another project in your library".into(),
            ));
        }
        if ProjectMeta::read(&dir)?.is_some_and(|m| m.id != id) {
            return Err(ProjectError::BadFolder(
                "that folder belongs to a different project".into(),
            ));
        }
        row.workspace_path = dir_str;
        self.db.update_project(&row)?;
        self.summary_of(id)
    }

    /// Registers a folder that carries a known project id as a separate project.
    pub(crate) fn import_as_copy(&self, path: &Path) -> Result<ProjectSummary, ProjectError> {
        let dir = canonical_dir(path)?;
        let dir_str = dir.to_string_lossy().into_owned();
        if self.db.get_project_by_path(&dir_str)?.is_some() {
            return Err(ProjectError::BadFolder(
                "that folder is already in your library".into(),
            ));
        }
        let old = ProjectMeta::read(&dir)?.ok_or_else(|| {
            ProjectError::BadFolder("that folder has no project.json to copy".into())
        })?;
        let slug = self.unique_slug(&old.slug)?;
        let meta = ProjectMeta {
            id: uuid::Uuid::new_v4().to_string(),
            app_id: security::default_identifier(&slug),
            slug,
            ..old
        };

        self.db.insert_project(&NewProject {
            id: meta.id.clone(),
            name: meta.name.clone(),
            slug: meta.slug.clone(),
            template_id: meta.template_id,
            app_id: meta.app_id.clone(),
            workspace_path: dir_str,
            pending_op: Some(PendingOp::Copy),
        })?;
        // Until project.json is replaced the folder still says it is the old project, so
        // a failure here can safely drop the row; after that, reconcile finishes the job.
        if let Err(e) = meta.write(&dir) {
            let _ = self.db.delete_project(&meta.id);
            return Err(e.into());
        }
        let conf = dir.join(TAURI_CONF);
        if meta.template_id == TemplateId::TauriVue && conf.is_file() {
            template::edit_json(&conf, |v| {
                v["identifier"] = serde_json::json!(meta.app_id);
                Ok(())
            })?;
        }
        self.db.set_pending(&meta.id, None)?;
        self.summary_of(&meta.id)
    }

    /// Rows still being created or imported are not shown.
    pub(crate) fn list(&self) -> Result<Vec<ProjectSummary>, ProjectError> {
        Ok(self
            .db
            .list_projects()?
            .into_iter()
            .filter(|r| r.pending_op.is_none())
            .map(summarize)
            .collect())
    }

    /// Finishes or rolls back operations interrupted by a crash.
    pub(crate) fn reconcile_on_startup(&self) -> Result<(), ProjectError> {
        for row in self.db.list_projects()? {
            let Some(op) = row.pending_op else { continue };
            let workspace = Path::new(&row.workspace_path);
            if let (PendingOp::Create, Some(parent)) = (op, workspace.parent()) {
                let _ = fs::remove_dir_all(parent.join(format!("{TEMP_PREFIX}{}", row.id)));
            }
            let finished = matches!(ProjectMeta::read(workspace), Ok(Some(m)) if m.id == row.id);
            if finished {
                self.db.set_pending(&row.id, None)?;
            } else {
                self.db.delete_project(&row.id)?;
            }
        }
        Ok(())
    }

    fn summary_of(&self, id: &str) -> Result<ProjectSummary, ProjectError> {
        Ok(summarize(
            self.db.get_project(id)?.ok_or(ProjectError::NotFound)?,
        ))
    }
}

fn canonical_dir(path: &Path) -> Result<PathBuf, ProjectError> {
    let dir = fs::canonicalize(path)
        .map_err(|e| ProjectError::BadFolder(format!("cannot open {}: {e}", path.display())))?;
    if !dir.is_dir() {
        return Err(ProjectError::BadFolder("that path is not a folder".into()));
    }
    fs::read_dir(&dir)
        .map_err(|e| ProjectError::BadFolder(format!("folder is not readable: {e}")))?;
    Ok(dir)
}

/// `(productName, identifier)` from a tauri.conf.json.
fn read_conf(conf: &Path) -> Result<(Option<String>, Option<String>), ProjectError> {
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(conf)?)
        .map_err(|e| ProjectError::BadFolder(format!("tauri.conf.json is not valid JSON: {e}")))?;
    let get = |k: &str| v.get(k).and_then(|s| s.as_str()).map(str::to_owned);
    Ok((get("productName"), get("identifier")))
}

/// Trimmed, single-line and at most 80 chars, so a foreign name always passes validation.
fn display_name(raw: &str) -> String {
    let name: String = raw.chars().filter(|c| !c.is_control()).take(80).collect();
    let name = name.trim();
    if name.is_empty() {
        "Imported project".into()
    } else {
        name.into()
    }
}

fn summarize(row: ProjectRow) -> ProjectSummary {
    let state = state_of(&row);
    ProjectSummary {
        id: row.id,
        name: row.name,
        slug: row.slug,
        template_id: row.template_id,
        app_id: row.app_id,
        workspace_path: row.workspace_path,
        archived: row.archived,
        supports_operations: row.template_id.supports_operations(),
        state,
    }
}

/// project.json wins; this only reports where tauri.conf.json disagrees.
fn state_of(row: &ProjectRow) -> ProjectState {
    let dir = Path::new(&row.workspace_path);
    if !dir.is_dir() {
        return ProjectState::MissingWorkspace;
    }
    let mismatch = |detail: String| ProjectState::MetadataMismatch { detail };
    let meta = match ProjectMeta::read(dir) {
        Ok(Some(m)) => m,
        Ok(None) => return mismatch("project.json is missing".into()),
        Err(e) => return mismatch(e.to_string()),
    };
    if meta.template_id != TemplateId::TauriVue {
        return ProjectState::Ok;
    }
    let conf = dir.join(TAURI_CONF);
    if !conf.is_file() {
        return ProjectState::Ok;
    }
    let (name, id) = match read_conf(&conf) {
        Ok(v) => v,
        Err(e) => return mismatch(e.to_string()),
    };
    let mut diffs = Vec::new();
    if name.is_some_and(|n| n != meta.name) {
        diffs.push("name");
    }
    if id.is_some_and(|i| i != meta.app_id) {
        diffs.push("identifier");
    }
    if diffs.is_empty() {
        ProjectState::Ok
    } else {
        mismatch(format!(
            "tauri.conf.json differs from project.json: {}",
            diffs.join(", ")
        ))
    }
}
