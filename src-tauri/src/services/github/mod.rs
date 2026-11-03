//! GhService: everything Studio asks of the `gh` CLI for github.com (PRD M5 steps 4, 5, 8):
//! auth state, in-app sign-in, repository list, create-and-push, clone.
//!
//! `gh` is the only credential holder. Network git commands that `gh` runs get the same
//! non-interactive settings as RepoService (`services/git/net.rs`), through the environment:
//! `network_env` plus the `-c` pairs as `GIT_CONFIG_COUNT`/`KEY_n`/`VALUE_n`, so nothing is
//! written to the user's git config. Every message that leaves this module passes `redact`.
// The commands lane wires this up (M5 lane C); remove when it does.
#![allow(dead_code)]

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, watch};

use crate::persistence::{Db, SyncOp, SyncRecord};
use crate::platform::process::spawn_group;
use crate::services::agents::secrets::redact;
use crate::services::diagnostics::spawn::{Output, RunError, read_capped, run_capped};
use crate::services::git::net::{network_config_args, network_env};
use crate::services::git::repo::{HeadState, RepoService, SourceError};

/// Temp folder prefix of an unfinished clone (M5-D10).
pub(crate) const CLONE_TEMP_PREFIX: &str = ".ai-app-studio-clone-";
const HOST: &str = "github.com";
const AUTH_TIMEOUT: Duration = Duration::from_secs(20);
const LIST_TIMEOUT: Duration = Duration::from_secs(30);
// ponytail: create+push has a fixed 5 min cap instead of Stop; add cancel when a UI needs it.
const CREATE_TIMEOUT: Duration = Duration::from_secs(300);
/// `gh auth login` must print its code within this long or the output is "unparseable".
const CODE_TIMEOUT: Duration = Duration::from_secs(20);
const TRANSCRIPT_CAP: usize = 4096;

// ------------------------------------------------------------------ types

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum AuthState {
    SignedIn {
        login: String,
        git_protocol: String,
    },
    SignedOut,
    /// Offline, DNS or timeout: not a reason to ask for a sign-in.
    Unreachable {
        message: String,
    },
    /// The stored token was rejected (HTTP 401).
    TokenInvalid {
        message: String,
    },
    GhMissing,
}

/// What the sign-in screen shows while the browser step is pending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginCode {
    /// `XXXX-XXXX`
    pub code: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepoSummary {
    pub name_with_owner: String,
    pub private: bool,
    pub description: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Visibility {
    Private,
    Public,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteCreated {
    pub name_with_owner: String,
    pub url: String,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, thiserror::Error)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum GhError {
    #[error("The GitHub command line tool (gh) was not found.")]
    GhMissing,
    #[error("GitHub needs you to sign in again.")]
    SignedOut,
    #[error("Studio could not reach GitHub: {message}")]
    Unreachable { message: String },
    #[error("GitHub rejected the saved sign-in: {message}")]
    TokenInvalid { message: String },
    #[error("You already have a repository with this name on GitHub.")]
    NameTaken,
    #[error("{message}")]
    InvalidName { message: String },
    #[error("This project is a folder inside the larger Git repository at {top}.")]
    InsideLargerRepo { top: String },
    #[error("This project already has a remote named origin ({url}).")]
    OriginExists { url: String },
    #[error("Make a first commit before creating the GitHub repository.")]
    NoCommits,
    #[error("This folder is not a Git repository yet.")]
    NotARepo,
    #[error("{path} already exists.")]
    TargetExists { path: String },
    #[error("Enter a repository as owner/name or a GitHub address.")]
    InvalidRepoSpec,
    #[error("Stopped.")]
    Stopped,
    #[error("{message}")]
    Gh { message: String },
}

fn gh_err(m: impl AsRef<str>) -> GhError {
    GhError::Gh {
        message: redact(m.as_ref().trim()).into_owned(),
    }
}

// ------------------------------------------------------------------ validation

/// GitHub's rules: letters, digits, `-` `_` `.`; at most 100; not `.`/`..`; not `*.git`.
pub(crate) fn validate_repo_name(name: &str) -> Result<(), GhError> {
    let bad = |m: &str| Err(GhError::InvalidName { message: m.into() });
    if name.is_empty() {
        return bad("Enter a name.");
    }
    if name.len() > 100 {
        return bad("Use at most 100 characters.");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    {
        return bad("Use letters, digits, hyphens, underscores and dots only.");
    }
    if name == "." || name == ".." || name.ends_with(".git") {
        return bad("GitHub does not allow this name.");
    }
    Ok(())
}

/// `owner/name`, or an https/ssh GitHub address, as `(owner, name)`.
pub(crate) fn parse_repo_spec(spec: &str) -> Result<(String, String), GhError> {
    const PREFIXES: [&str; 5] = [
        "https://github.com/",
        "http://github.com/",
        "git@github.com:",
        "ssh://git@github.com/",
        "github.com/",
    ];
    let s = spec.trim();
    let s = PREFIXES.iter().find_map(|p| s.strip_prefix(p)).unwrap_or(s);
    let s = s.trim_end_matches('/');
    let s = s.strip_suffix(".git").unwrap_or(s);
    let (owner, name) = s.split_once('/').ok_or(GhError::InvalidRepoSpec)?;
    let owner_ok = (1..=39).contains(&owner.len())
        && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !owner_ok || validate_repo_name(name).is_err() {
        return Err(GhError::InvalidRepoSpec);
    }
    Ok((owner.into(), name.into()))
}

// ------------------------------------------------------------------ service

pub(crate) struct GhService {
    /// `None` when the doctor did not find gh.
    gh: Option<PathBuf>,
    git: PathBuf,
    /// The doctor's host env (`DiagnosticsService::spawn_env`).
    env: Vec<(String, String)>,
    /// The user has their own `core.sshCommand`: do not override ssh (see `network_env`).
    user_ssh_config: bool,
    db: Arc<Db>,
}

impl GhService {
    pub(crate) fn new(
        gh: Option<PathBuf>,
        git: PathBuf,
        env: Vec<(String, String)>,
        db: Arc<Db>,
    ) -> Self {
        Self {
            gh,
            git,
            env,
            user_ssh_config: false,
            db,
        }
    }

    pub(crate) fn with_user_ssh_config(mut self, yes: bool) -> Self {
        self.user_ssh_config = yes;
        self
    }

    /// The environment for a gh command that runs network git: batch ssh, no prompts, the gh
    /// credential helper and stall limits as `GIT_CONFIG_*` (the `-c` pairs of `net.rs`).
    fn network_cmd_env(&self) -> Vec<(String, String)> {
        let base: Vec<_> = self
            .env
            .iter()
            .filter(|(k, _)| {
                !k.starts_with("GIT_CONFIG_COUNT")
                    && !k.starts_with("GIT_CONFIG_KEY_")
                    && !k.starts_with("GIT_CONFIG_VALUE_")
            })
            .cloned()
            .collect();
        let mut env = network_env(&base, self.user_ssh_config);
        let pairs: Vec<(String, String)> = network_config_args(self.gh.as_deref())
            .chunks(2)
            .filter_map(|c| {
                let (k, v) = c.get(1)?.split_once('=')?;
                Some((k.to_owned(), v.to_owned()))
            })
            .collect();
        env.push(("GIT_CONFIG_COUNT".into(), pairs.len().to_string()));
        for (i, (k, v)) in pairs.into_iter().enumerate() {
            env.push((format!("GIT_CONFIG_KEY_{i}"), k));
            env.push((format!("GIT_CONFIG_VALUE_{i}"), v));
        }
        env
    }

    fn gh_path(&self) -> Result<&Path, GhError> {
        self.gh.as_deref().ok_or(GhError::GhMissing)
    }

    async fn run(
        &self,
        args: &[&str],
        env: &[(String, String)],
        timeout: Duration,
    ) -> Result<Output, GhError> {
        match run_capped(self.gh_path()?, args, env, timeout).await {
            Ok(o) => Ok(o),
            Err(RunError::Spawn(e)) => Err(spawn_err(&e)),
            Err(RunError::TimedOut(_)) => Err(GhError::Unreachable {
                message: "GitHub did not answer in time.".into(),
            }),
        }
    }

    /// Sign-in state for github.com (M5-D15: the caller decides when to ask).
    pub(crate) async fn auth_status(&self) -> AuthState {
        let args = ["auth", "status", "--hostname", HOST, "--json", "hosts"];
        match self.run(&args, &self.env, AUTH_TIMEOUT).await {
            Ok(o) => parse_auth(&o),
            Err(GhError::GhMissing) => AuthState::GhMissing,
            Err(e) => AuthState::Unreachable {
                message: redact(&e.to_string()).into_owned(),
            },
        }
    }

    /// `gh auth login --web`: `on_code` gets the one-time code and URL once, then this waits for
    /// the browser step. Stop (`cancel` true) kills the process group.
    pub(crate) async fn login(
        &self,
        on_code: impl FnOnce(LoginCode),
        mut cancel: watch::Receiver<bool>,
    ) -> Result<AuthState, GhError> {
        let gh = self.gh_path()?;
        let args = [
            "auth",
            "login",
            "--web",
            "--hostname",
            HOST,
            "--git-protocol",
            "https",
            "--skip-ssh-key",
        ];
        let mut g = spawn_group(gh, &args, &self.env, None, Stdio::null())
            .await
            .map_err(|e| spawn_err(&e))?;
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        if let Some(p) = g.child.stdout.take() {
            tokio::spawn(forward_lines(p, tx.clone()));
        }
        if let Some(p) = g.child.stderr.take() {
            tokio::spawn(forward_lines(p, tx.clone()));
        }
        drop(tx);

        let code_re = Regex::new(r"\b([A-Z0-9]{4}-[A-Z0-9]{4})\b").expect("static regex");
        let url_re = Regex::new(r"https://github\.com\S*").expect("static regex");
        let (mut code, mut url) = (None::<String>, None::<String>);
        let mut on_code = Some(on_code);
        let mut transcript = String::new();
        let deadline = tokio::time::Instant::now() + CODE_TIMEOUT;
        loop {
            tokio::select! {
                line = rx.recv() => {
                    let Some(line) = line else { break };
                    if transcript.len() < TRANSCRIPT_CAP {
                        transcript.push_str(&line);
                        transcript.push('\n');
                    }
                    if code.is_none() && line.contains("One-time code") {
                        code = code_re.captures(&line).map(|c| c[1].to_owned());
                    }
                    if url.is_none() {
                        url = url_re.find(&line).map(|m| m.as_str().to_owned());
                    }
                    if let (Some(code), Some(url)) = (&code, &url)
                        && let Some(f) = on_code.take()
                    {
                        f(LoginCode { code: code.clone(), url: url.clone() });
                    }
                }
                () = stopped(&mut cancel) => {
                    let _ = g.kill_group().await;
                    return Err(GhError::Stopped);
                }
                () = tokio::time::sleep_until(deadline), if on_code.is_some() => {
                    let _ = g.kill_group().await;
                    return Err(gh_err(format!("Could not read the sign-in code from gh. {transcript}")));
                }
            }
        }
        let status = g.child.wait().await.map_err(|e| gh_err(e.to_string()))?;
        if on_code.is_some() {
            return Err(gh_err(format!(
                "Could not read the sign-in code from gh. {transcript}"
            )));
        }
        if !status.success() {
            return Err(gh_err(format!(
                "GitHub sign-in did not finish. {transcript}"
            )));
        }
        Ok(self.auth_status().await)
    }

    /// Your repositories, newest first (DR12).
    pub(crate) async fn list_repos(&self, limit: u32) -> Result<Vec<RepoSummary>, GhError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            name_with_owner: String,
            is_private: bool,
            description: Option<String>,
            updated_at: String,
        }
        let limit = limit.clamp(1, 100).to_string();
        let args = [
            "repo",
            "list",
            "--json",
            "nameWithOwner,isPrivate,description,updatedAt",
            "--limit",
            &limit,
        ];
        let o = self.run(&args, &self.env, LIST_TIMEOUT).await?;
        if !o.status.success() {
            return Err(classify(&o.stderr));
        }
        let mut repos: Vec<RepoSummary> = serde_json::from_str::<Vec<Raw>>(&o.stdout)
            .map_err(|e| gh_err(format!("unexpected answer from gh: {e}")))?
            .into_iter()
            .map(|r| RepoSummary {
                name_with_owner: r.name_with_owner,
                private: r.is_private,
                description: r.description.filter(|d| !d.is_empty()),
                updated_at: r.updated_at,
            })
            .collect();
        // RFC 3339 UTC timestamps sort as text.
        repos.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(repos)
    }

    /// Creates the GitHub repository for `project` and pushes it (M5-D9 guards first).
    /// Once the guards pass, the outcome (ok or not) is recorded in `git_sync`.
    pub(crate) async fn create_repo(
        &self,
        repo: &RepoService,
        project_id: &str,
        project: &Path,
        name: &str,
        visibility: Visibility,
        description: Option<&str>,
    ) -> Result<RemoteCreated, GhError> {
        validate_repo_name(name)?;
        let status = repo
            .status(project_id, project, &[])
            .await
            .map_err(|e| match e {
                SourceError::NotARepo => GhError::NotARepo,
                e => gh_err(e.to_string()),
            })?;
        let top = status.location.top.clone();
        if status.location.is_subfolder {
            return Err(GhError::InsideLargerRepo { top });
        }
        if let Some(url) = self.origin_url(Path::new(&top)).await {
            return Err(GhError::OriginExists { url });
        }
        if matches!(status.head, HeadState::Unborn { .. }) {
            return Err(GhError::NoCommits);
        }

        let result = self
            .create_and_push(&top, name, visibility, description)
            .await;
        let (ok, message) = match &result {
            Ok(c) => (true, format!("Created {}", c.url)),
            Err(e) => (false, e.to_string()),
        };
        let _ = self.db.upsert_sync(&SyncRecord {
            project_id: project_id.to_owned(),
            operation: SyncOp::Create,
            ok,
            message: Some(redact(&message).into_owned()),
            finished_at: now_rfc3339(),
        });
        result
    }

    async fn create_and_push(
        &self,
        top: &str,
        name: &str,
        visibility: Visibility,
        description: Option<&str>,
    ) -> Result<RemoteCreated, GhError> {
        let vis = match visibility {
            Visibility::Private => "--private",
            Visibility::Public => "--public",
        };
        let mut args = vec!["repo", "create", name, vis];
        if let Some(d) = description.map(str::trim).filter(|d| !d.is_empty()) {
            args.extend(["--description", d]);
        }
        args.extend(["--source", top, "--remote", "origin", "--push"]);
        let o = self
            .run(&args, &self.network_cmd_env(), CREATE_TIMEOUT)
            .await?;
        let both = format!("{}\n{}", o.stderr, o.stdout);
        if !o.status.success() {
            return Err(if both.to_lowercase().contains("already exists") {
                GhError::NameTaken
            } else {
                classify(&both)
            });
        }
        let url = both
            .lines()
            .map(str::trim)
            .rev()
            .find(|l| l.starts_with("https://github.com/"))
            .ok_or_else(|| gh_err("gh did not print the new repository's address"))?
            .to_owned();
        Ok(RemoteCreated {
            name_with_owner: url["https://github.com/".len()..]
                .trim_end_matches('/')
                .to_owned(),
            url,
            visibility,
        })
    }

    async fn origin_url(&self, top: &Path) -> Option<String> {
        let top = top.display().to_string();
        let args = ["-C", &top, "remote", "get-url", "origin"];
        let o = run_capped(&self.git, &args, &self.env, AUTH_TIMEOUT)
            .await
            .ok()?;
        o.status
            .success()
            .then(|| redact(o.stdout.trim()).into_owned())
    }

    /// Clones into `<parent>/<repo name>` through a temp sibling (M5-D10). No time limit; Stop
    /// (`cancel` true) kills the process group and removes only the temp folder.
    pub(crate) async fn clone(
        &self,
        spec: &str,
        parent: &Path,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<PathBuf, GhError> {
        let (owner, name) = parse_repo_spec(spec)?;
        let target = parent.join(&name);
        let exists = |p: &Path| p.symlink_metadata().is_ok();
        let taken = || GhError::TargetExists {
            path: target.display().to_string(),
        };
        if exists(&target) {
            return Err(taken());
        }
        let temp = parent.join(format!("{CLONE_TEMP_PREFIX}{}", uuid::Uuid::new_v4()));
        let args = [
            "repo".to_owned(),
            "clone".into(),
            format!("{owner}/{name}"),
            temp.display().to_string(),
        ];

        let mut g = spawn_group(
            self.gh_path()?,
            &args,
            &self.network_cmd_env(),
            Some(parent),
            Stdio::null(),
        )
        .await
        .map_err(|e| spawn_err(&e))?;
        let (out, err) = (g.child.stdout.take(), g.child.stderr.take());
        let done = {
            let work = async { tokio::join!(g.child.wait(), read_capped(out), read_capped(err)) };
            tokio::select! {
                r = work => Some(r),
                () = stopped(&mut cancel) => None,
            }
        };
        let finished = match done {
            None => {
                let _ = g.kill_group().await;
                Err(GhError::Stopped)
            }
            Some((Ok(s), ..)) if s.success() => Ok(()),
            Some((Ok(_), stdout, stderr)) => Err(classify(&format!("{stderr}\n{stdout}"))),
            Some((Err(e), ..)) => Err(gh_err(e.to_string())),
        }
        .and_then(|()| {
            // ponytail: check-then-rename race with another writer in `parent`; rename onto a
            // non-empty folder fails anyway, so the worst case is an error.
            if exists(&target) {
                return Err(taken());
            }
            std::fs::rename(&temp, &target)
                .map_err(|e| gh_err(format!("could not finish the clone: {e}")))
        });
        if finished.is_err() {
            let _ = std::fs::remove_dir_all(&temp);
        }
        finished.map(|()| target)
    }
}

/// Removes leftover `.ai-app-studio-clone-*` folders directly inside `dir` (M5-D10 startup).
pub(crate) fn remove_stale_clone_temps(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let is_temp = e
            .file_name()
            .to_string_lossy()
            .starts_with(CLONE_TEMP_PREFIX);
        if is_temp && e.file_type().is_ok_and(|t| t.is_dir()) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

// ------------------------------------------------------------------ helpers

fn spawn_err(e: &std::io::Error) -> GhError {
    if e.kind() == std::io::ErrorKind::NotFound {
        GhError::GhMissing
    } else {
        gh_err(format!("could not start gh: {e}"))
    }
}

/// Resolves when Stop is requested; never when the sender is gone.
async fn stopped(rx: &mut watch::Receiver<bool>) {
    loop {
        if *rx.borrow() {
            return;
        }
        if rx.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

async fn forward_lines(pipe: impl tokio::io::AsyncRead + Unpin, tx: mpsc::UnboundedSender<String>) {
    let mut lines = BufReader::new(pipe).lines();
    while let Ok(Some(l)) = lines.next_line().await {
        if tx.send(l).is_err() {
            break;
        }
    }
}

fn parse_auth(o: &Output) -> AuthState {
    #[derive(Deserialize)]
    struct Resp {
        hosts: std::collections::HashMap<String, Vec<Host>>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Host {
        state: String,
        #[serde(default)]
        active: bool,
        login: Option<String>,
        git_protocol: Option<String>,
        error: Option<String>,
    }
    let Ok(resp) = serde_json::from_str::<Resp>(&o.stdout) else {
        let text = format!("{}{}", o.stderr, o.stdout);
        return match classify(&text) {
            GhError::SignedOut => AuthState::SignedOut,
            _ => AuthState::Unreachable {
                message: redact(text.trim()).into_owned(),
            },
        };
    };
    let Some(hosts) = resp.hosts.get(HOST) else {
        return AuthState::SignedOut;
    };
    let Some(h) = hosts.iter().find(|h| h.active).or(hosts.first()) else {
        return AuthState::SignedOut;
    };
    if h.state == "success" {
        return AuthState::SignedIn {
            login: h.login.clone().unwrap_or_default(),
            git_protocol: h.git_protocol.clone().unwrap_or_else(|| "https".into()),
        };
    }
    let message = redact(h.error.as_deref().unwrap_or("unknown error")).into_owned();
    if message.contains("401") || message.to_lowercase().contains("bad credentials") {
        AuthState::TokenInvalid { message }
    } else {
        AuthState::Unreachable { message }
    }
}

/// Maps gh/git stderr to a typed error; anything unknown is a redacted `Gh`.
fn classify(text: &str) -> GhError {
    let l = text.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| l.contains(n));
    let message = || redact(text.trim()).into_owned();
    if has(&[
        "not logged in",
        "gh auth login",
        "authentication required",
        "could not read username",
    ]) {
        GhError::SignedOut
    } else if has(&["http 401", "bad credentials", "authentication failed"]) {
        GhError::TokenInvalid { message: message() }
    } else if has(&[
        "connection refused",
        "dial tcp",
        "no such host",
        "could not resolve host",
        "i/o timeout",
        "network is unreachable",
        "timed out",
    ]) {
        GhError::Unreachable { message: message() }
    } else {
        gh_err(text)
    }
}

/// UTC `YYYY-MM-DDTHH:MM:SSZ` (same arithmetic as `install::now_rfc3339`, which is private to
/// that module; merge the two when the lanes meet).
fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rem) = (i64::try_from(secs / 86400).unwrap_or(0), secs % 86400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}
