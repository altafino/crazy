//! SettingsService: app_settings (default projects folder, per-tool path overrides).
//! Stored as one JSON document under the `settings` key.

use crate::persistence::{Db, DbError};
use crate::security::expand_tilde;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;
use std::path::Path;

const KEY: &str = "settings";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Settings {
    /// Folder the New Project wizard suggests as parent.
    pub default_projects_dir: String,
    /// Tool id (as reported by the doctor) -> absolute path to the binary.
    pub tool_overrides: BTreeMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SettingsError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("{0}")]
    Invalid(String),
}

pub(crate) fn load(db: &Db, default_projects_dir: &Path) -> Result<Settings, SettingsError> {
    let stored = db
        .get_setting(KEY)?
        .and_then(|v| serde_json::from_value::<Settings>(v).ok());
    Ok(stored.unwrap_or_else(|| Settings {
        default_projects_dir: default_projects_dir.display().to_string(),
        tool_overrides: BTreeMap::new(),
    }))
}

/// Validates and stores; returns what was stored (tilde expanded, blank overrides dropped).
pub(crate) fn save(
    db: &Db,
    mut s: Settings,
    home: Option<&Path>,
) -> Result<Settings, SettingsError> {
    let dir = absolute(s.default_projects_dir.trim(), home)
        .map_err(|e| SettingsError::Invalid(format!("Default projects folder: {e}")))?;
    s.default_projects_dir = dir.display().to_string();

    let mut overrides = BTreeMap::new();
    for (tool, raw) in s.tool_overrides {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let path = absolute(raw, home)
            .map_err(|e| SettingsError::Invalid(format!("Path for {tool}: {e}")))?;
        overrides.insert(tool, path.display().to_string());
    }
    s.tool_overrides = overrides;

    let value = serde_json::to_value(&s).expect("Settings always serializes");
    db.set_setting(KEY, &value)?;
    Ok(s)
}

fn absolute(raw: &str, home: Option<&Path>) -> Result<std::path::PathBuf, String> {
    let p = expand_tilde(raw, home).map_err(|e| e.0)?;
    if p.is_absolute() {
        Ok(p)
    } else {
        Err("use an absolute path".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_until_saved_then_round_trips() {
        let db = Db::open_in_memory().unwrap();
        let home = Path::new("/home/u");
        let first = load(&db, Path::new("/home/u/Projects/AIAppStudio")).unwrap();
        assert_eq!(first.default_projects_dir, "/home/u/Projects/AIAppStudio");
        assert!(first.tool_overrides.is_empty());

        let mut overrides = BTreeMap::new();
        overrides.insert("pnpm".into(), "~/.local/share/pnpm/pnpm".into());
        overrides.insert("git".into(), "   ".into());
        let saved = save(
            &db,
            Settings {
                default_projects_dir: "~/Code".into(),
                tool_overrides: overrides,
            },
            Some(home),
        )
        .unwrap();
        assert_eq!(saved.default_projects_dir, "/home/u/Code");
        assert_eq!(saved.tool_overrides.len(), 1, "blank override is dropped");
        assert_eq!(
            saved.tool_overrides["pnpm"],
            "/home/u/.local/share/pnpm/pnpm"
        );
        assert_eq!(load(&db, Path::new("/ignored")).unwrap(), saved);
    }

    #[test]
    fn relative_override_is_rejected_and_nothing_is_stored() {
        let db = Db::open_in_memory().unwrap();
        let mut overrides = BTreeMap::new();
        overrides.insert("node".into(), "bin/node".into());
        let err = save(
            &db,
            Settings {
                default_projects_dir: "/p".into(),
                tool_overrides: overrides,
            },
            Some(Path::new("/home/u")),
        )
        .unwrap_err();
        assert!(err.to_string().contains("node"), "{err}");
        assert_eq!(db.get_setting(KEY).unwrap(), None);
    }
}
