//! XDG-based locations for Studio's own files.
use std::path::{Path, PathBuf};

const APP_DIR: &str = "ai-app-studio";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PathsError {
    #[error("HOME is not set, cannot locate the user directories")]
    HomeMissing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StudioPaths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub default_projects_dir: PathBuf,
    /// `$XDG_DATA_HOME/applications`, where GNOME looks for launcher entries.
    pub applications_dir: PathBuf,
}

impl StudioPaths {
    pub fn from_process_env() -> Result<Self, PathsError> {
        Self::from_env(|k| std::env::var(k).ok())
    }

    pub fn from_env(get: impl Fn(&str) -> Option<String>) -> Result<Self, PathsError> {
        let home = get("HOME")
            .filter(|h| !h.is_empty())
            .map(PathBuf::from)
            .ok_or(PathsError::HomeMissing)?;
        // The XDG spec says relative values must be ignored.
        let xdg = |var: &str, fallback: &str| {
            get(var)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(fallback))
        };
        let data_home = xdg("XDG_DATA_HOME", ".local/share");
        Ok(Self {
            applications_dir: data_home.join("applications"),
            data_dir: data_home.join(APP_DIR),
            config_dir: xdg("XDG_CONFIG_HOME", ".config"),
            cache_dir: xdg("XDG_CACHE_HOME", ".cache"),
            default_projects_dir: home.join("Projects/AIAppStudio"),
        })
    }
}

/// Where one project's built and installed releases live (PRD M4 step 4/5, M4-D6, M4-D12):
///
/// ```text
/// <data_dir>/installed/<project-id>/
///   current.AppImage -> releases/<version>/app.AppImage   (the active release, M4-D5)
///   icon.png                                              (the launcher's Icon=)
///   releases/<version>/app.AppImage, releases/<version>/icon.png
/// <applications_dir>/ai-app-studio-generated-<project-id>.desktop
/// ```
///
/// Callers pass a version that already passed strict semver validation and a project id
/// from the database; neither ever comes from free user text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AppLayout {
    root: PathBuf,
    desktop_file: PathBuf,
}

impl AppLayout {
    pub(crate) fn new(paths: &StudioPaths, project_id: &str) -> Self {
        Self {
            root: paths.data_dir.join("installed").join(project_id),
            desktop_file: paths
                .applications_dir
                .join(format!("ai-app-studio-generated-{project_id}.desktop")),
        }
    }
    /// `installed/<project-id>`: everything Studio stores for this app.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
    pub(crate) fn release_dir(&self, version: &str) -> PathBuf {
        self.root.join("releases").join(version)
    }
    pub(crate) fn release_appimage(&self, version: &str) -> PathBuf {
        self.release_dir(version).join("app.AppImage")
    }
    pub(crate) fn release_icon(&self, version: &str) -> PathBuf {
        self.release_dir(version).join("icon.png")
    }
    /// The symlink GNOME starts; its target is relative (`releases/<v>/app.AppImage`).
    pub(crate) fn current_link(&self) -> PathBuf {
        self.root.join("current.AppImage")
    }
    /// The relative symlink target for a version.
    pub(crate) fn current_target(version: &str) -> PathBuf {
        Path::new("releases").join(version).join("app.AppImage")
    }
    /// The version `current.AppImage` points at (M4-D5: the symlink is the only record of the
    /// active release). `None` when there is no link or its target isn't
    /// `releases/<version>/app.AppImage`.
    pub(crate) fn active_version(&self) -> Option<String> {
        let target = std::fs::read_link(self.current_link()).ok()?;
        let mut parts = target.components().map(|c| c.as_os_str().to_str());
        match (parts.next()?, parts.next()?, parts.next()?, parts.next()) {
            (Some("releases"), Some(v), Some("app.AppImage"), None) if !v.is_empty() => {
                Some(v.to_owned())
            }
            _ => None,
        }
    }
    pub(crate) fn icon(&self) -> PathBuf {
        self.root.join("icon.png")
    }
    pub(crate) fn desktop_file(&self) -> &Path {
        &self.desktop_file
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn defaults_from_home() {
        let p = StudioPaths::from_env(env(&[("HOME", "/home/u")])).unwrap();
        assert_eq!(
            p.data_dir,
            PathBuf::from("/home/u/.local/share/ai-app-studio")
        );
        assert_eq!(p.config_dir, PathBuf::from("/home/u/.config"));
        assert_eq!(p.cache_dir, PathBuf::from("/home/u/.cache"));
        assert_eq!(
            p.default_projects_dir,
            PathBuf::from("/home/u/Projects/AIAppStudio")
        );
    }

    #[test]
    fn absolute_xdg_wins() {
        let p = StudioPaths::from_env(env(&[
            ("HOME", "/home/u"),
            ("XDG_DATA_HOME", "/d"),
            ("XDG_CONFIG_HOME", "/c"),
            ("XDG_CACHE_HOME", "/k"),
        ]))
        .unwrap();
        assert_eq!(p.data_dir, PathBuf::from("/d/ai-app-studio"));
        assert_eq!(p.applications_dir, PathBuf::from("/d/applications"));
        assert_eq!(p.config_dir, PathBuf::from("/c"));
        assert_eq!(p.cache_dir, PathBuf::from("/k"));
    }

    #[test]
    fn relative_xdg_is_ignored() {
        let p = StudioPaths::from_env(env(&[("HOME", "/home/u"), ("XDG_DATA_HOME", "rel/dir")]))
            .unwrap();
        assert_eq!(
            p.data_dir,
            PathBuf::from("/home/u/.local/share/ai-app-studio")
        );
    }

    #[test]
    fn missing_home_is_an_error() {
        assert_eq!(
            StudioPaths::from_env(env(&[("XDG_DATA_HOME", "/d")])),
            Err(PathsError::HomeMissing)
        );
    }

    #[test]
    fn app_layout_paths() {
        let p = StudioPaths::from_env(env(&[("HOME", "/home/u")])).unwrap();
        assert_eq!(
            p.applications_dir,
            PathBuf::from("/home/u/.local/share/applications")
        );
        let l = AppLayout::new(&p, "abc");
        let root = PathBuf::from("/home/u/.local/share/ai-app-studio/installed/abc");
        assert_eq!(l.root(), root);
        assert_eq!(
            l.release_appimage("1.2.0"),
            root.join("releases/1.2.0/app.AppImage")
        );
        assert_eq!(
            l.release_icon("1.2.0"),
            root.join("releases/1.2.0/icon.png")
        );
        assert_eq!(l.current_link(), root.join("current.AppImage"));
        assert_eq!(
            AppLayout::current_target("1.2.0"),
            PathBuf::from("releases/1.2.0/app.AppImage")
        );
        assert_eq!(l.icon(), root.join("icon.png"));
        assert_eq!(
            l.desktop_file(),
            Path::new("/home/u/.local/share/applications/ai-app-studio-generated-abc.desktop")
        );
    }

    #[test]
    fn active_version_reads_the_symlink() {
        let d = tempfile::tempdir().unwrap();
        let p = StudioPaths::from_env(env(&[
            ("HOME", "/home/u"),
            ("XDG_DATA_HOME", d.path().to_str().unwrap()),
        ]))
        .unwrap();
        let l = AppLayout::new(&p, "abc");
        assert_eq!(l.active_version(), None);
        std::fs::create_dir_all(l.root()).unwrap();
        std::os::unix::fs::symlink(AppLayout::current_target("1.0.0"), l.current_link()).unwrap();
        assert_eq!(l.active_version().as_deref(), Some("1.0.0"));
        std::fs::remove_file(l.current_link()).unwrap();
        std::os::unix::fs::symlink("elsewhere/x.AppImage", l.current_link()).unwrap();
        assert_eq!(l.active_version(), None);
    }

    /// The asset protocol may only ever serve the attachments folder (data dir = `$DATA/ai-app-studio`).
    #[test]
    fn asset_protocol_scope_is_only_the_attachments_folder() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let ap = &conf["app"]["security"]["assetProtocol"];
        if ap["enable"] == true {
            assert_eq!(
                ap["scope"],
                serde_json::json!(["$DATA/ai-app-studio/attachments/**"])
            );
        }
        assert_eq!(ap["enable"], true, "attachments previews need it");
    }
}
