//! SettingsService: app_settings (default projects folder, per-tool path overrides).
//! Stored as one JSON document under the `settings` key.

use crate::domain::FieldError;
use crate::persistence::{Db, DbError};
use crate::security::expand_tilde;
use crate::services::builds::export::ExportTarget;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;
use std::path::Path;

const KEY: &str = "settings";
const PREVIEW_KEY: &str = "preview_limits";
const ATTACHMENT_KEY: &str = "attachment_limits";
const APPROVAL_KEY: &str = "approval_settings";
const DATABASE_KEY: &str = "database_limits";

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

const EXPORT_LICENSE_KEY: &str = "export_license_accepted_at";
const EXPORT_TARGETS_KEY: &str = "export_last_targets";

/// When the user accepted Microsoft's license for the Windows build tools (F22, D10).
pub(crate) fn export_license_accepted_at(db: &Db) -> Option<String> {
    db.get_setting(EXPORT_LICENSE_KEY)
        .ok()
        .flatten()
        .and_then(|v| v.as_str().map(str::to_owned))
        .filter(|s| !s.is_empty())
}

/// Stores the acceptance date, or forgets it with `None` (Withdraw).
pub(crate) fn set_export_license_accepted_at(
    db: &Db,
    at: Option<&str>,
) -> Result<(), SettingsError> {
    db.set_setting(EXPORT_LICENSE_KEY, &serde_json::json!(at.unwrap_or("")))?;
    Ok(())
}

/// The export targets ticked last time; both on first use (design 3).
pub(crate) fn export_last_targets(db: &Db) -> Vec<ExportTarget> {
    db.get_setting(EXPORT_TARGETS_KEY)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value::<Vec<ExportTarget>>(v).ok())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| vec![ExportTarget::Linux, ExportTarget::Windows])
}

pub(crate) fn set_export_last_targets(
    db: &Db,
    targets: &[ExportTarget],
) -> Result<(), SettingsError> {
    let value = serde_json::to_value(targets).expect("targets always serialize");
    db.set_setting(EXPORT_TARGETS_KEY, &value)?;
    Ok(())
}

const NOTIFY_FINISH_KEY: &str = "notify_finish";
const LAST_SEEN_VERSION_KEY: &str = "last_seen_version";

/// Start-up state the UI needs before any settings form opens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UiState {
    pub notify_finish: bool,
    pub last_seen_version: Option<String>,
}

pub(crate) fn ui_state(db: &Db) -> UiState {
    UiState {
        notify_finish: db
            .get_setting(NOTIFY_FINISH_KEY)
            .ok()
            .flatten()
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        last_seen_version: db
            .get_setting(LAST_SEEN_VERSION_KEY)
            .ok()
            .flatten()
            .and_then(|v| v.as_str().map(str::to_owned))
            .filter(|s| !s.is_empty()),
    }
}

pub(crate) fn set_notify_finish(db: &Db, on: bool) -> Result<(), SettingsError> {
    db.set_setting(NOTIFY_FINISH_KEY, &serde_json::json!(on))?;
    Ok(())
}

pub(crate) fn set_last_seen_version(db: &Db, version: &str) -> Result<(), SettingsError> {
    db.set_setting(LAST_SEEN_VERSION_KEY, &serde_json::json!(version))?;
    Ok(())
}

/// What a chat prompt may carry (D12). Read at attach time, so a change applies at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct AttachmentLimits {
    /// Files per prompt (1-50).
    pub max_files: u32,
    /// Image size cap in MB (1-256), pasted images included.
    pub max_image_mb: u32,
    /// PDF size cap in MB (1-256).
    pub max_pdf_mb: u32,
    /// Text/code size cap in MB (1-256).
    pub max_text_mb: u32,
    /// Word file size cap in MB (1-256).
    pub max_docx_mb: u32,
    /// Converted Markdown size cap in MB (1-256).
    pub max_docx_converted_mb: u32,
    /// All files of one prompt together, in MB (1-1024).
    pub max_total_mb: u32,
    /// How long pandoc may take per file, in seconds (10-600).
    pub pandoc_timeout_secs: u32,
}

impl Default for AttachmentLimits {
    fn default() -> Self {
        Self {
            max_files: 10,
            max_image_mb: 20,
            max_pdf_mb: 32,
            max_text_mb: 2,
            max_docx_mb: 20,
            max_docx_converted_mb: 2,
            max_total_mb: 64,
            pandoc_timeout_secs: 60,
        }
    }
}

impl AttachmentLimits {
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
        check("Files per prompt", self.max_files, 1, 50)?;
        check("Image size (MB)", self.max_image_mb, 1, 256)?;
        check("PDF size (MB)", self.max_pdf_mb, 1, 256)?;
        check("Text file size (MB)", self.max_text_mb, 1, 256)?;
        check("Word file size (MB)", self.max_docx_mb, 1, 256)?;
        check(
            "Converted Word text size (MB)",
            self.max_docx_converted_mb,
            1,
            256,
        )?;
        check("Total per prompt (MB)", self.max_total_mb, 1, 1024)?;
        check(
            "Word conversion timeout (s)",
            self.pandoc_timeout_secs,
            10,
            600,
        )
    }
}

/// Stored limits, or the defaults when nothing valid is stored.
pub(crate) fn load_attachment_limits(db: &Db) -> AttachmentLimits {
    db.get_setting(ATTACHMENT_KEY)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value::<AttachmentLimits>(v).ok())
        .filter(|l| l.validate().is_ok())
        .unwrap_or_default()
}

pub(crate) fn save_attachment_limits(
    db: &Db,
    limits: AttachmentLimits,
) -> Result<AttachmentLimits, SettingsError> {
    limits.validate()?;
    let value = serde_json::to_value(limits).expect("AttachmentLimits always serializes");
    db.set_setting(ATTACHMENT_KEY, &value)?;
    Ok(limits)
}

/// Settings > General "Database tab" card (R9/D13). Read per query, so a change applies at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct DatabaseLimits {
    /// How long one Database tab read query may run, in seconds (0-60; 0 = no limit).
    pub query_time_limit_secs: u32,
}

impl Default for DatabaseLimits {
    fn default() -> Self {
        Self {
            query_time_limit_secs: 2,
        }
    }
}

impl DatabaseLimits {
    fn validate(&self) -> Result<(), SettingsError> {
        if self.query_time_limit_secs <= 60 {
            Ok(())
        } else {
            Err(SettingsError::Invalid(
                "Query time limit (s) must be between 0 and 60".into(),
            ))
        }
    }
}

/// Stored limits, or the defaults when nothing valid is stored.
pub(crate) fn load_database_limits(db: &Db) -> DatabaseLimits {
    db.get_setting(DATABASE_KEY)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value::<DatabaseLimits>(v).ok())
        .filter(|l| l.validate().is_ok())
        .unwrap_or_default()
}

pub(crate) fn save_database_limits(
    db: &Db,
    limits: DatabaseLimits,
) -> Result<DatabaseLimits, SettingsError> {
    limits.validate()?;
    let value = serde_json::to_value(limits).expect("DatabaseLimits always serializes");
    db.set_setting(DATABASE_KEY, &value)?;
    Ok(limits)
}

/// Settings > Approvals. The composer's Auto-approve checkbox starts from `auto_approve`;
/// `ask_for_risky` is read when a run starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ApprovalSettings {
    /// Make changes runs allow commands, file edits and reads without asking.
    pub auto_approve: bool,
    /// Even then, ask for risky commands, edits outside the project and reads of secrets.
    pub ask_for_risky: bool,
}

impl Default for ApprovalSettings {
    fn default() -> Self {
        Self {
            auto_approve: false,
            ask_for_risky: true,
        }
    }
}

pub(crate) fn load_approval_settings(db: &Db) -> ApprovalSettings {
    db.get_setting(APPROVAL_KEY)
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

pub(crate) fn save_approval_settings(
    db: &Db,
    s: ApprovalSettings,
) -> Result<ApprovalSettings, SettingsError> {
    let value = serde_json::to_value(s).expect("ApprovalSettings always serializes");
    db.set_setting(APPROVAL_KEY, &value)?;
    Ok(s)
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SettingsError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("{0}")]
    Invalid(String),
    /// Per-field problems; `save` reports every one at once (`defaultProjectsDir`,
    /// `toolOverrides.<toolId>`).
    #[error("Please fix the highlighted fields.")]
    Fields(Vec<FieldError>),
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
    let mut errors = Vec::new();
    let fe = |field: String, e: String| FieldError {
        field,
        message: capitalize(&e),
    };
    match absolute(s.default_projects_dir.trim(), home) {
        Ok(dir) => s.default_projects_dir = dir.display().to_string(),
        Err(e) => errors.push(fe("defaultProjectsDir".into(), e)),
    }

    let mut overrides = BTreeMap::new();
    for (tool, raw) in s.tool_overrides {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        match absolute(raw, home) {
            Ok(path) => {
                overrides.insert(tool, path.display().to_string());
            }
            Err(e) => errors.push(fe(format!("toolOverrides.{tool}"), e)),
        }
    }
    if !errors.is_empty() {
        return Err(SettingsError::Fields(errors));
    }
    s.tool_overrides = overrides;

    let value = serde_json::to_value(&s).expect("Settings always serializes");
    db.set_setting(KEY, &value)?;
    Ok(s)
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
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
    fn ui_state_defaults_and_round_trips() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(
            ui_state(&db),
            UiState {
                notify_finish: true,
                last_seen_version: None
            }
        );
        set_notify_finish(&db, false).unwrap();
        set_last_seen_version(&db, "0.3.0").unwrap();
        assert_eq!(
            ui_state(&db),
            UiState {
                notify_finish: false,
                last_seen_version: Some("0.3.0".into())
            }
        );
    }

    #[test]
    fn relative_paths_come_back_as_field_errors() {
        let db = Db::open_in_memory().unwrap();
        let mut overrides = BTreeMap::new();
        overrides.insert("git".into(), "bin/git".into());
        let err = save(
            &db,
            Settings {
                default_projects_dir: "Code".into(),
                tool_overrides: overrides,
            },
            Some(Path::new("/home/u")),
        )
        .unwrap_err();
        let SettingsError::Fields(f) = err else {
            panic!("expected field errors, got {err:?}")
        };
        let fields: Vec<_> = f.iter().map(|e| e.field.as_str()).collect();
        assert_eq!(fields, ["defaultProjectsDir", "toolOverrides.git"]);
        assert_eq!(f[0].message, "Use an absolute path");
        assert!(db.get_setting(KEY).unwrap().is_none(), "nothing stored");
    }

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
    fn database_limits_default_validate_and_round_trip() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(load_database_limits(&db).query_time_limit_secs, 2);
        for ok in [0, 60] {
            let l = DatabaseLimits {
                query_time_limit_secs: ok,
            };
            assert_eq!(save_database_limits(&db, l).unwrap(), l);
            assert_eq!(load_database_limits(&db), l);
        }
        let bad = DatabaseLimits {
            query_time_limit_secs: 61,
        };
        assert!(save_database_limits(&db, bad).is_err());
        assert_eq!(load_database_limits(&db).query_time_limit_secs, 60);
        db.set_setting(DATABASE_KEY, &serde_json::json!({"queryTimeLimitSecs": 99}))
            .unwrap();
        assert_eq!(load_database_limits(&db), DatabaseLimits::default());
    }

    #[test]
    fn attachment_limits_default_validate_and_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let d = AttachmentLimits::default();
        assert_eq!(load_attachment_limits(&db), d);
        let lo = AttachmentLimits {
            max_files: 1,
            max_image_mb: 1,
            max_pdf_mb: 1,
            max_text_mb: 1,
            max_docx_mb: 1,
            max_docx_converted_mb: 1,
            max_total_mb: 1,
            pandoc_timeout_secs: 10,
        };
        let hi = AttachmentLimits {
            max_files: 50,
            max_image_mb: 256,
            max_pdf_mb: 256,
            max_text_mb: 256,
            max_docx_mb: 256,
            max_docx_converted_mb: 256,
            max_total_mb: 1024,
            pandoc_timeout_secs: 600,
        };
        for ok in [lo, hi] {
            assert_eq!(save_attachment_limits(&db, ok).unwrap(), ok);
            assert_eq!(load_attachment_limits(&db), ok);
        }
        let bad = [
            AttachmentLimits { max_files: 0, ..d },
            AttachmentLimits { max_files: 51, ..d },
            AttachmentLimits {
                max_image_mb: 0,
                ..d
            },
            AttachmentLimits {
                max_pdf_mb: 257,
                ..d
            },
            AttachmentLimits {
                max_text_mb: 257,
                ..d
            },
            AttachmentLimits {
                max_docx_mb: 0,
                ..d
            },
            AttachmentLimits {
                max_docx_converted_mb: 257,
                ..d
            },
            AttachmentLimits {
                max_total_mb: 1025,
                ..d
            },
            AttachmentLimits {
                pandoc_timeout_secs: 9,
                ..d
            },
            AttachmentLimits {
                pandoc_timeout_secs: 601,
                ..d
            },
        ];
        for b in bad {
            assert!(save_attachment_limits(&db, b).is_err(), "{b:?}");
        }
        assert_eq!(
            load_attachment_limits(&db),
            hi,
            "a rejected save changes nothing"
        );
        // Out-of-range stored value falls back to the defaults; a missing field takes its default.
        db.set_setting(ATTACHMENT_KEY, &serde_json::json!({"maxFiles": 0}))
            .unwrap();
        assert_eq!(load_attachment_limits(&db), d);
        db.set_setting(ATTACHMENT_KEY, &serde_json::json!({"maxFiles": 3}))
            .unwrap();
        assert_eq!(
            load_attachment_limits(&db),
            AttachmentLimits { max_files: 3, ..d }
        );
    }

    #[test]
    fn approval_settings_default_off_and_cautious_then_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let d = load_approval_settings(&db);
        assert!(!d.auto_approve && d.ask_for_risky);
        let on = ApprovalSettings {
            auto_approve: true,
            ask_for_risky: false,
        };
        save_approval_settings(&db, on).unwrap();
        assert_eq!(load_approval_settings(&db), on);
    }

    #[test]
    fn export_license_and_targets_are_stored_and_withdrawn() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(export_license_accepted_at(&db), None);
        assert_eq!(
            export_last_targets(&db),
            [ExportTarget::Linux, ExportTarget::Windows]
        );
        set_export_license_accepted_at(&db, Some("2026-11-06T10:00:00.000Z")).unwrap();
        assert_eq!(
            export_license_accepted_at(&db).as_deref(),
            Some("2026-11-06T10:00:00.000Z")
        );
        set_export_license_accepted_at(&db, None).unwrap();
        assert_eq!(export_license_accepted_at(&db), None);
        set_export_last_targets(&db, &[ExportTarget::Windows]).unwrap();
        assert_eq!(export_last_targets(&db), [ExportTarget::Windows]);
        // An empty stored list falls back to both.
        set_export_last_targets(&db, &[]).unwrap();
        assert_eq!(export_last_targets(&db).len(), 2);
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
        assert!(
            matches!(&err, SettingsError::Fields(f) if f[0].field == "toolOverrides.node"),
            "{err}"
        );
        assert_eq!(db.get_setting(KEY).unwrap(), None);
    }
}
