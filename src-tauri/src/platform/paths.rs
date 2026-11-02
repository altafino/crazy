//! XDG-based locations for Studio's own files.
use std::path::PathBuf;

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
        Ok(Self {
            data_dir: xdg("XDG_DATA_HOME", ".local/share").join(APP_DIR),
            config_dir: xdg("XDG_CONFIG_HOME", ".config"),
            cache_dir: xdg("XDG_CACHE_HOME", ".cache"),
            default_projects_dir: home.join("Projects/AIAppStudio"),
        })
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
}
