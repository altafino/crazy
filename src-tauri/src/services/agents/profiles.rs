//! Agent profiles: validation, the proposed first profile (D33), CRUD and probe (D32, D40).

use std::{path::Path, sync::LazyLock, time::Duration};

use regex::Regex;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::domain::FieldError;
use crate::persistence::{Db, DbError, ProfileRow};
use crate::services::agents::secrets::looks_secret;
use crate::services::diagnostics::spawn::run_capped;

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const SECRET_MSG: &str = "Secrets can't be stored here; use the CLI's own login (claude /login)";

/// Studio's house rules, appended to Claude's system prompt (D32).
pub const DEFAULT_INSTRUCTIONS: &str = "\
You are working inside AI App Studio on one project.
- Work only inside the current project folder.
- Do not edit files outside the project.
- Use pnpm for JavaScript packages (never npm or yarn).
- Do not run git commit, git push or any publish command, and never change git remotes.
- Keep changes focused on the request.
- At the end, explain what you changed.";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub executable: String,
    pub env: Vec<EnvVar>,
    pub model: Option<String>,
    pub inactivity_secs: u32,
    pub instructions: String,
    pub is_default: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProposedProfile {
    pub profile: AgentProfile,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub ok: bool,
    pub version: Option<String>,
    /// The version is inside the range the live-approval protocol was verified on.
    pub verified: bool,
    pub message: String,
}

// ------------------------------------------------------------- validation

static ENV_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap());

fn err(field: &str, message: &str) -> FieldError {
    FieldError {
        field: field.into(),
        message: message.into(),
    }
}

pub fn validate(p: &AgentProfile) -> Vec<FieldError> {
    let mut e = Vec::new();
    if p.name.trim().is_empty() {
        e.push(err("name", "Name is required"));
    }
    if p.provider != "claude" {
        e.push(err("provider", "Only the claude provider is supported"));
    }
    if !Path::new(&p.executable).is_absolute() {
        e.push(err("executable", "Executable must be an absolute path"));
    }
    let mut seen = std::collections::HashSet::new();
    for v in &p.env {
        let field = format!("env.{}", v.name);
        if !ENV_NAME.is_match(&v.name) {
            e.push(err(&field, "Not a valid variable name"));
        } else if !seen.insert(v.name.as_str()) {
            e.push(err(&field, "Duplicate variable"));
        }
        if looks_secret(&v.name, &v.value) {
            e.push(err(&field, SECRET_MSG));
        }
    }
    if !(60..=7200).contains(&p.inactivity_secs) {
        e.push(err("inactivitySecs", "Must be between 60 and 7200 seconds"));
    }
    e
}

// --------------------------------------------------------------- proposal

/// Builds the first profile from the doctor's findings. Not saved; the caller shows it
/// for confirmation.
pub fn propose_default(
    doctor_claude_path: Option<&str>,
    alias_env_assignments: &[String],
    home: Option<&Path>,
) -> ProposedProfile {
    let mut notes = Vec::new();
    let mut env = Vec::new();
    for a in alias_env_assignments {
        let Some((name, raw)) = a.split_once('=') else {
            notes.push(format!("not copied: {a} is not NAME=value"));
            continue;
        };
        if !ENV_NAME.is_match(name) {
            notes.push(format!("not copied: {name} is not a valid variable name"));
            continue;
        }
        match expand_value(raw, home) {
            Some(value) if looks_secret(name, &value) => {
                notes.push(format!("not copied: {name} looks like a secret"));
            }
            Some(value) => env.push(EnvVar {
                name: name.into(),
                value,
            }),
            None => notes.push(format!("not copied: {name} uses shell syntax")),
        }
    }
    if doctor_claude_path.is_none() {
        notes.push("claude was not found; enter its full path".into());
    }
    ProposedProfile {
        profile: AgentProfile {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Claude".into(),
            provider: "claude".into(),
            executable: doctor_claude_path.unwrap_or_default().into(),
            env,
            model: None,
            inactivity_secs: 600,
            instructions: DEFAULT_INSTRUCTIONS.into(),
            is_default: true,
            enabled: true,
        },
        notes,
    }
}

/// Strips one pair of surrounding quotes, expands a leading `~`/`$HOME`/`${HOME}`.
/// `None` means the value needs a shell and must not be copied.
fn expand_value(raw: &str, home: Option<&Path>) -> Option<String> {
    let s = raw.trim();
    let s = ["'", "\""]
        .iter()
        .find_map(|q| s.strip_prefix(q)?.strip_suffix(q))
        .unwrap_or(s);
    let (rest, uses_home) = ["${HOME}", "$HOME", "~"]
        .iter()
        .find_map(|p| {
            let r = s.strip_prefix(p)?;
            (r.is_empty() || r.starts_with('/')).then_some(r)
        })
        .map_or((s, false), |r| (r, true));
    if (!uses_home && rest.starts_with('~'))
        || rest.contains(['$', '`', '\'', '"', '*', '?', '[', '{', '\\', ';', '|', '&'])
    {
        return None;
    }
    if uses_home {
        let home = home?.to_str()?;
        return Some(format!("{}{rest}", home.trim_end_matches('/')));
    }
    Some(rest.to_string())
}

// ------------------------------------------------------------------- CRUD

#[derive(Debug, thiserror::Error)]
pub(crate) enum SaveError {
    #[error("profile is invalid")]
    Invalid(Vec<FieldError>),
    #[error(transparent)]
    Db(#[from] DbError),
}

fn to_row(p: &AgentProfile) -> ProfileRow {
    ProfileRow {
        id: p.id.clone(),
        name: p.name.clone(),
        provider: p.provider.clone(),
        executable: p.executable.clone(),
        env: p
            .env
            .iter()
            .map(|v| (v.name.clone(), v.value.clone()))
            .collect(),
        model: p.model.clone(),
        inactivity_secs: p.inactivity_secs,
        instructions: p.instructions.clone(),
        is_default: p.is_default,
        enabled: p.enabled,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn from_row(r: ProfileRow) -> AgentProfile {
    AgentProfile {
        id: r.id,
        name: r.name,
        provider: r.provider,
        executable: r.executable,
        env: r
            .env
            .into_iter()
            .map(|(name, value)| EnvVar { name, value })
            .collect(),
        model: r.model,
        inactivity_secs: r.inactivity_secs,
        instructions: r.instructions,
        is_default: r.is_default,
        enabled: r.enabled,
    }
}

/// Validates, then inserts or updates. An empty id gets a fresh one. Setting
/// `is_default` unsets every other profile.
pub(crate) fn save(db: &Db, p: &AgentProfile) -> Result<AgentProfile, SaveError> {
    let errors = validate(p);
    if !errors.is_empty() {
        return Err(SaveError::Invalid(errors));
    }
    let mut p = p.clone();
    if p.id.is_empty() {
        p.id = uuid::Uuid::new_v4().to_string();
    }
    db.save_profile(&to_row(&p))?;
    Ok(p)
}

pub(crate) fn list(db: &Db) -> Result<Vec<AgentProfile>, DbError> {
    Ok(db.list_profiles()?.into_iter().map(from_row).collect())
}

pub(crate) fn get_default(db: &Db) -> Result<Option<AgentProfile>, DbError> {
    Ok(db.get_default_profile()?.map(from_row))
}

/// Also clears `projects.default_agent_id` where it pointed at this profile.
pub(crate) fn delete(db: &Db, id: &str) -> Result<(), DbError> {
    db.delete_profile(id)
}

// ------------------------------------------------------------------ probe

/// `spawn_env` with the profile's env on top (profile wins): exactly what a run sees.
pub(crate) fn merged_env(
    profile: &AgentProfile,
    spawn_env: &[(String, String)],
) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = spawn_env
        .iter()
        .filter(|(k, _)| !profile.env.iter().any(|v| &v.name == k))
        .cloned()
        .collect();
    env.extend(
        profile
            .env
            .iter()
            .map(|v| (v.name.clone(), v.value.clone())),
    );
    env
}

/// Runs `<executable> --version` with the merged env, so the check sees exactly what a
/// real run would.
pub async fn probe(profile: &AgentProfile, spawn_env: &[(String, String)]) -> ProbeResult {
    let env = merged_env(profile, spawn_env);
    match run_capped(&profile.executable, &["--version"], &env, PROBE_TIMEOUT).await {
        Ok(out) if out.status.success() => {
            let version = out.stdout.lines().next().map(|l| l.trim().to_string());
            ProbeResult {
                ok: true,
                message: version.clone().unwrap_or_default(),
                verified: version.as_deref().is_some_and(super::claude::version_verified),
                version,
            }
        }
        Ok(out) => ProbeResult {
            ok: false,
            version: None,
            verified: false,
            message: format!("exited with {}: {}", out.status, out.stderr.trim()),
        },
        Err(e) => ProbeResult {
            ok: false,
            version: None,
            verified: false,
            message: e.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::TemplateId;
    use crate::persistence::NewProject;
    use std::path::PathBuf;

    fn profile() -> AgentProfile {
        propose_default(Some("/usr/bin/claude"), &[], None).profile
    }

    fn with_env(name: &str, value: &str) -> AgentProfile {
        let mut p = profile();
        p.env = vec![EnvVar {
            name: name.into(),
            value: value.into(),
        }];
        p
    }

    fn fields(p: &AgentProfile) -> Vec<String> {
        validate(p).into_iter().map(|e| e.field).collect()
    }

    #[test]
    fn alias_parse_table() {
        let home = PathBuf::from("/home/u");
        let cases: &[(&str, Option<&str>)] = &[
            ("CLAUDE_CONFIG_DIR=~/.claude-a", Some("/home/u/.claude-a")),
            ("CLAUDE_CONFIG_DIR=~", Some("/home/u")),
            ("CLAUDE_CONFIG_DIR='~/.claude-a'", Some("/home/u/.claude-a")),
            ("X=\"/abs/path\"", Some("/abs/path")),
            ("X=$HOME/x", Some("/home/u/x")),
            ("X=${HOME}/x", Some("/home/u/x")),
            ("X=plain", Some("plain")),
            ("X=$OTHER/x", None),
            ("X=$(whoami)", None),
            ("X=`id`", None),
            ("X=/tmp/*", None),
            ("X=~other/x", None),
            ("X=$HOMEX/x", None),
            ("ANTHROPIC_API_KEY=abc", None),
            ("X=sk-ant-abc123", None),
        ];
        for (input, want) in cases {
            let p = propose_default(Some("/c"), &[(*input).into()], Some(&home));
            let got = p.profile.env.first().map(|v| v.value.as_str());
            assert_eq!(got, *want, "{input}");
            if want.is_none() {
                assert!(
                    p.notes.iter().any(|n| n.starts_with("not copied: ")),
                    "{input}"
                );
            }
        }
        let p = propose_default(Some("/c"), &["X=$OTHER/x".into()], Some(&home));
        assert_eq!(p.notes, ["not copied: X uses shell syntax"]);
        // No home known: a leading ~ cannot be expanded, so it is not copied.
        let p = propose_default(Some("/c"), &["X=~/a".into()], None);
        assert!(p.profile.env.is_empty());
    }

    #[test]
    fn proposal_defaults() {
        let p = propose_default(None, &[], None);
        assert!(p.notes.iter().any(|n| n.contains("claude was not found")));
        let p = propose_default(Some("/usr/bin/claude"), &[], None).profile;
        assert_eq!(p.executable, "/usr/bin/claude");
        assert!(p.is_default && p.instructions == DEFAULT_INSTRUCTIONS);
        assert!(validate(&p).is_empty());
    }

    #[test]
    fn validation_table() {
        assert!(validate(&profile()).is_empty());
        assert!(validate(&with_env("CLAUDE_CONFIG_DIR", "/h/.claude-a")).is_empty());

        let mut p = profile();
        p.name = "  ".into();
        p.executable = "claude".into();
        p.inactivity_secs = 59;
        assert_eq!(fields(&p), ["name", "executable", "inactivitySecs"]);
        p.inactivity_secs = 7201;
        assert!(fields(&p).contains(&"inactivitySecs".to_string()));
        p.inactivity_secs = 60;
        p.name = "n".into();
        p.executable = "/c".into();
        assert!(validate(&p).is_empty());
        p.inactivity_secs = 7200;
        assert!(validate(&p).is_empty());

        assert_eq!(fields(&with_env("1BAD", "x")), ["env.1BAD"]);
        assert_eq!(fields(&with_env("A-B", "x")), ["env.A-B"]);
        let mut p = with_env("A", "1");
        p.env.push(p.env[0].clone());
        assert_eq!(validate(&p)[0].message, "Duplicate variable");
    }

    #[test]
    fn secrets_are_refused() {
        for name in [
            "FOO_KEY",
            "foo_token",
            "X_SECRET",
            "MY_PASSWORD_HINT",
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY_2",
        ] {
            let e = validate(&with_env(name, "v"));
            assert_eq!(e.len(), 1, "{name}");
            assert!(e[0].message.contains("claude /login"));
        }
        for value in [
            "sk-ant-api03-abc",
            "sk-abc",
            "ghp_abcdef",
            "gho_abcdef",
            "github_pat_11AAA",
            "AKIAABCDEFGHIJKLMNOP",
            "xoxb-1-2",
            "xoxa-1",
            "xoxp-1",
        ] {
            assert_eq!(fields(&with_env("HARMLESS", value)).len(), 1, "{value}");
        }
        for ok in ["/home/u/task-force", "AKIA123", "main"] {
            assert!(validate(&with_env("HARMLESS", ok)).is_empty(), "{ok}");
        }
    }

    fn db_with_project() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.insert_project(&NewProject {
            id: "p1".into(),
            name: "a".into(),
            slug: "a".into(),
            template_id: TemplateId::TauriVue,
            app_id: "com.x.a".into(),
            workspace_path: "/p/a".into(),
            pending_op: None,
        })
        .unwrap();
        db
    }

    #[test]
    fn save_validates_and_default_is_exclusive() {
        let db = db_with_project();
        let mut bad = profile();
        bad.name.clear();
        assert!(matches!(save(&db, &bad), Err(SaveError::Invalid(_))));
        assert!(list(&db).unwrap().is_empty());

        let a = save(&db, &profile()).unwrap();
        assert!(!a.id.is_empty());
        let mut b = profile();
        b.id = "b".into();
        save(&db, &b).unwrap();
        let all = list(&db).unwrap();
        assert_eq!(all.iter().filter(|p| p.is_default).count(), 1);
        assert_eq!(get_default(&db).unwrap().unwrap().id, "b");
    }

    #[test]
    fn delete_clears_project_default() {
        let db = db_with_project();
        let a = save(&db, &profile()).unwrap();
        let mut row = db.get_project("p1").unwrap().unwrap();
        row.default_agent_id = Some(a.id.clone());
        db.update_project(&row).unwrap();
        delete(&db, &a.id).unwrap();
        assert!(
            db.get_project("p1")
                .unwrap()
                .unwrap()
                .default_agent_id
                .is_none()
        );
        assert!(list(&db).unwrap().is_empty());
    }

    fn script(dir: &Path, body: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("fake-claude");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_str().unwrap().into()
    }

    #[tokio::test]
    async fn probe_applies_profile_env_over_spawn_env() {
        let dir = tempfile::tempdir().unwrap();
        let exe = script(
            dir.path(),
            "[ \"$CLAUDE_CONFIG_DIR\" = /h/.claude-a ] || { echo \"got $CLAUDE_CONFIG_DIR\" >&2; exit 3; }\n\
             echo '2.0.1 (Claude Code)'",
        );
        let mut p = with_env("CLAUDE_CONFIG_DIR", "/h/.claude-a");
        p.executable = exe;
        let spawn_env = [("CLAUDE_CONFIG_DIR".to_string(), "/other".to_string())];
        let r = probe(&p, &spawn_env).await;
        assert!(r.ok, "{}", r.message);
        assert_eq!(r.version.as_deref(), Some("2.0.1 (Claude Code)"));

        // Without the profile env the script rejects, proving the env was really applied.
        p.env.clear();
        let r = probe(&p, &spawn_env).await;
        assert!(!r.ok && r.message.contains("got /other"), "{}", r.message);
    }

    #[tokio::test]
    async fn probe_reports_missing_executable() {
        let mut p = profile();
        p.executable = "/nonexistent/claude".into();
        let r = probe(&p, &[]).await;
        assert!(!r.ok && r.version.is_none());
    }
}
