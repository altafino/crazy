//! SettingsService: app_settings (default projects folder, per-tool path overrides).
//! Stored as one JSON document under the `settings` key.

use crate::persistence::{Db, DbError};
use crate::security::expand_tilde;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;
use std::path::Path;

const KEY: &str = "settings";
const PREVIEW_KEY: &str = "preview_limits";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Settings {
    /// Folder the New Project wizard suggests as parent.
    pub default_projects_dir: String,
    /// Tool id (as reported by the doctor) -> absolute path to the binary.
    pub tool_overrides: BTreeMap<String, String>,
}

/// How much of a preview's output Studio keeps (M3-D20, PRD M3 step 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewLimits {
    /// Lines kept in memory per running preview (1,000-50,000).
    pub max_lines: u32,
    /// Size cap of one preview's log file in MB (1-100).
    pub max_log_mb: u32,
    /// Log files kept per project (1-50).
    pub keep_logs: u32,
}

impl Default for PreviewLimits {
    fn default() -> Self {
        Self {
            max_lines: 5_000,
            max_log_mb: 10,
            keep_logs: 5,
        }
    }
}

impl PreviewLimits {
    fn validate(&self) -> Result<(), SettingsError> {
        let check = |name: &str, v: u32, lo: u32, hi: u32| {
            if (lo..=hi).contains(&v) {
                Ok(())
            } else {
                Err(SettingsError::Invalid(format!(
                    "{name} must be between {lo} and {hi}"
                )))
            }
        };
        check("Lines kept in memory", self.max_lines, 1_000, 50_000)?;
        check("Log file size (MB)", self.max_log_mb, 1, 100)?;
        check("Log files kept per project", self.keep_logs, 1, 50)
    }
}

/// Stored limits, or the defaults when nothing valid is stored.
pub(crate) fn load_preview_limits(db: &Db) -> PreviewLimits {
    db.get_setting(PREVIEW_KEY)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value::<PreviewLimits>(v).ok())
        .filter(|l| l.validate().is_ok())
        .unwrap_or_default()
}

pub(crate) fn save_preview_limits(
    db: &Db,
    limits: PreviewLimits,
) -> Result<PreviewLimits, SettingsError> {
    limits.validate()?;
    let value = serde_json::to_value(limits).expect("PreviewLimits always serializes");
    db.set_setting(PREVIEW_KEY, &value)?;
    Ok(limits)
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
    fn preview_limits_default_validate_and_round_trip() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(load_preview_limits(&db), PreviewLimits::default());
        let ok = PreviewLimits {
            max_lines: 1_000,
            max_log_mb: 100,
            keep_logs: 50,
        };
        assert_eq!(save_preview_limits(&db, ok).unwrap(), ok);
        assert_eq!(load_preview_limits(&db), ok);
        let d = PreviewLimits::default();
        for bad in [
            PreviewLimits {
                max_lines: 999,
                ..d
            },
            PreviewLimits {
                max_lines: 50_001,
                ..d
            },
            PreviewLimits { max_log_mb: 0, ..d },
            PreviewLimits {
                max_log_mb: 101,
                ..d
            },
            PreviewLimits { keep_logs: 0, ..d },
            PreviewLimits { keep_logs: 51, ..d },
        ] {
            assert!(save_preview_limits(&db, bad).is_err(), "{bad:?}");
        }
        assert_eq!(
            load_preview_limits(&db),
            ok,
            "a rejected save changes nothing"
        );
        // A hand-edited out-of-range row falls back to the defaults.
        db.set_setting(
            PREVIEW_KEY,
            &serde_json::json!({"maxLines": 5, "maxLogMb": 10, "keepLogs": 5}),
        )
        .unwrap();
        assert_eq!(load_preview_limits(&db), d);
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
