//! Domain types shared across services (project metadata, statuses).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

pub(crate) const PROJECT_SCHEMA: u32 = 1;
const META_DIR: &str = ".ai-app-studio";
const META_FILE: &str = "project.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub(crate) enum TemplateId {
    #[serde(rename = "tauri-vue")]
    TauriVue,
    #[serde(rename = "existing")]
    Existing,
}

impl TemplateId {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::TauriVue => "tauri-vue",
            Self::Existing => "existing",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "tauri-vue" => Some(Self::TauriVue),
            "existing" => Some(Self::Existing),
            _ => None,
        }
    }

    /// Preview, build, package and install; `existing` projects get none of them.
    pub(crate) fn supports_operations(self) -> bool {
        self == Self::TauriVue
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum MetaError {
    #[error("cannot access project.json: {0}")]
    Io(#[from] std::io::Error),
    #[error("project.json is malformed: {0}")]
    Malformed(String),
    #[error("this project was made by a newer Studio (project.json schema {0})")]
    NewerSchema(u32),
}

/// The `database` entry of project.json (app databases, D5): the app keeps its data in the convention's
/// SQLite files. Older Studios ignore it, so adding it needs no schema bump.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub(crate) struct DatabaseEntry {
    pub engine: String,
}

/// The `crafts` entry of project.json (ArtCraft, eng D3/D9): the ArtCraft apps Claude may use in
/// this project's runs. Absent means off. Older Studios ignore it, so it needs no schema bump.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub(crate) struct CraftsEntry {
    pub apps: Vec<String>,
}

/// Contents of `<workspace>/.ai-app-studio/project.json`. Snake_case on disk (PRD Section 11 step 5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub(crate) struct ProjectMeta {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub slug: String,
    pub template_id: TemplateId,
    pub app_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<DatabaseEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crafts: Option<CraftsEntry>,
}

impl ProjectMeta {
    pub(crate) fn new(name: &str, slug: &str, template_id: TemplateId, app_id: &str) -> Self {
        Self {
            schema: PROJECT_SCHEMA,
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            slug: slug.into(),
            template_id,
            app_id: app_id.into(),
            database: None,
            crafts: None,
        }
    }

    fn path(workspace: &Path) -> PathBuf {
        workspace.join(META_DIR).join(META_FILE)
    }

    /// `Ok(None)` when the workspace has no project.json.
    pub(crate) fn read(workspace: &Path) -> Result<Option<Self>, MetaError> {
        let text = match std::fs::read_to_string(Self::path(workspace)) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let value: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| MetaError::Malformed(e.to_string()))?;
        // Check the version first: a newer schema may not fit this struct.
        let schema = value.get("schema").and_then(|s| s.as_u64());
        match schema {
            Some(s) if s > u64::from(PROJECT_SCHEMA) => {
                return Err(MetaError::NewerSchema(u32::try_from(s).unwrap_or(u32::MAX)));
            }
            Some(_) => {}
            None => return Err(MetaError::Malformed("missing numeric `schema`".into())),
        }
        serde_json::from_value(value)
            .map(Some)
            .map_err(|e| MetaError::Malformed(e.to_string()))
    }

    /// Atomic: write a temp file beside the target, then rename over it.
    pub(crate) fn write(&self, workspace: &Path) -> Result<(), MetaError> {
        let dir = workspace.join(META_DIR);
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join(format!("{META_FILE}.tmp"));
        let mut text =
            serde_json::to_string_pretty(self).map_err(|e| MetaError::Malformed(e.to_string()))?;
        text.push('\n');
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, Self::path(workspace))?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum ProjectState {
    Ok,
    MissingWorkspace,
    MetadataMismatch { detail: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub template_id: TemplateId,
    pub app_id: String,
    pub workspace_path: String,
    pub archived: bool,
    /// Preview/build/package/install available (false for `existing` imports).
    pub supports_operations: bool,
    pub state: ProjectState,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NewProjectRequest {
    pub name: String,
    pub slug: Option<String>,
    pub parent_dir: String,
    pub identifier: Option<String>,
    pub init_git: bool,
    /// Store data in a SQLite database: the template keeps its database code (D9).
    #[serde(default)]
    pub database: bool,
    /// A picture (PNG, JPEG, WebP or SVG) to make the app icon from; None keeps the template's.
    #[serde(default)]
    pub icon: Option<String>,
    /// Also create a GitHub repository after the local project exists (M5-D3). Only with
    /// `init_git`. The caller runs `GhService::create_repo`; this create does not.
    pub create_github: Option<GithubCreate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GithubCreate {
    pub name: String,
    pub private: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FieldError {
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum ImportOutcome {
    Imported {
        project: ProjectSummary,
    },
    AlreadyInLibrary {
        project: ProjectSummary,
    },
    #[serde(rename_all = "camelCase")]
    NeedsRelink {
        existing: ProjectSummary,
        new_path: String,
    },
    #[serde(rename_all = "camelCase")]
    NeedsCopyConfirm {
        existing: ProjectSummary,
        new_path: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> ProjectMeta {
        ProjectMeta::new("My \"App\"", "my-app", TemplateId::TauriVue, "com.x.myapp")
    }

    #[test]
    fn crafts_entry_roundtrips_and_is_left_out_when_off() {
        let dir = tempfile::tempdir().unwrap();
        let raw = |dir: &Path| -> serde_json::Value {
            serde_json::from_str(&std::fs::read_to_string(ProjectMeta::path(dir)).unwrap()).unwrap()
        };
        let mut m = meta();
        m.crafts = Some(CraftsEntry {
            apps: vec!["photocraft".into(), "soundcraft".into()],
        });
        m.write(dir.path()).unwrap();
        assert_eq!(ProjectMeta::read(dir.path()).unwrap(), Some(m.clone()));
        assert_eq!(
            raw(dir.path())["crafts"]["apps"],
            serde_json::json!(["photocraft", "soundcraft"])
        );

        m.crafts = None;
        m.write(dir.path()).unwrap();
        assert!(
            raw(dir.path()).get("crafts").is_none(),
            "off leaves no key behind"
        );
    }

    #[test]
    fn write_read_roundtrip_uses_snake_case_keys() {
        let dir = tempfile::tempdir().unwrap();
        let m = meta();
        m.write(dir.path()).unwrap();
        assert_eq!(ProjectMeta::read(dir.path()).unwrap(), Some(m));
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(ProjectMeta::path(dir.path())).unwrap())
                .unwrap();
        assert_eq!(raw["template_id"], "tauri-vue");
        assert_eq!(raw["schema"], 1);
        assert!(!dir.path().join(".ai-app-studio/project.json.tmp").exists());
    }

    #[test]
    fn absent_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(ProjectMeta::read(dir.path()).unwrap(), None);
    }

    #[test]
    fn newer_schema_is_refused_even_with_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".ai-app-studio")).unwrap();
        std::fs::write(
            ProjectMeta::path(dir.path()),
            r#"{"schema":2,"future":true}"#,
        )
        .unwrap();
        let err = ProjectMeta::read(dir.path()).unwrap_err();
        assert!(matches!(err, MetaError::NewerSchema(2)));
        assert!(err.to_string().contains("newer Studio"));
    }

    #[test]
    fn malformed_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".ai-app-studio")).unwrap();
        std::fs::write(ProjectMeta::path(dir.path()), "{ nope").unwrap();
        assert!(matches!(
            ProjectMeta::read(dir.path()),
            Err(MetaError::Malformed(_))
        ));
    }

    #[test]
    fn ui_types_serialize_camel_case() {
        let s = serde_json::to_value(ImportOutcome::NeedsRelink {
            existing: ProjectSummary {
                id: "i".into(),
                name: "n".into(),
                slug: "s".into(),
                template_id: TemplateId::Existing,
                app_id: "a".into(),
                workspace_path: "/p".into(),
                archived: false,
                supports_operations: false,
                state: ProjectState::MetadataMismatch { detail: "d".into() },
            },
            new_path: "/q".into(),
        })
        .unwrap();
        assert_eq!(s["kind"], "needsRelink");
        assert_eq!(s["newPath"], "/q");
        assert_eq!(s["existing"]["workspacePath"], "/p");
        assert_eq!(s["existing"]["templateId"], "existing");
        assert_eq!(s["existing"]["state"]["kind"], "metadataMismatch");
    }
}
