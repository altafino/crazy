//! Docker as a CLI (like git and gh): the readiness probe, the `docker run` arguments, and the
//! cache size and removal. Nothing here touches the docker socket directly, and the only host
//! folder a container ever sees is the temporary source folder of one export (D7).

use std::path::Path;
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::process::Command;

use super::DockerState;
use crate::services::preview::Toolchain;

/// The build environment, embedded so the app needs no resource lookup.
pub(super) const DOCKERFILE: &str = include_str!("../../../../resources/export/Dockerfile");
pub(super) const IMAGE_REPO: &str = "ai-app-studio-export";
/// Download caches shared by every project: never the cargo target dir or node_modules (D12).
pub(super) const VOLUMES: [&str; 4] = [
    "ai-app-studio-export-cargo",
    "ai-app-studio-export-pnpm",
    "ai-app-studio-export-npm",
    "ai-app-studio-export-xwin",
];
/// Where each cache volume is mounted in the container.
const VOLUME_MOUNTS: [&str; 4] = ["/cache/cargo", "/cache/pnpm", "/cache/npm", "/cache/xwin"];

pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// `docker system df -v` sizes every image and volume: 5-7 s on a host with ~700 images.
pub(crate) const DF_TIMEOUT: Duration = Duration::from_secs(30);
const RM_TIMEOUT: Duration = Duration::from_secs(60);
/// `docker stop -t 3`: the container gets three seconds like a build gets after Stop.
const STOP_TIMEOUT: Duration = Duration::from_secs(15);

/// `ai-app-studio-export:<first 12 hex digits of the Dockerfile's SHA-256>`: a changed
/// Dockerfile is a new image, built on first use.
pub(super) fn image_tag() -> String {
    let digest = Sha256::digest(DOCKERFILE.as_bytes());
    let short: String = digest[..6].iter().map(|b| format!("{b:02x}")).collect();
    format!("{IMAGE_REPO}:{short}")
}

pub(super) fn container_name(job_id: &str) -> String {
    format!("studio-export-{job_id}")
}

// ------------------------------------------------------------------ capture

/// The result of one short docker command.
#[derive(Debug)]
pub(super) enum Captured {
    Done {
        ok: bool,
        stdout: String,
        stderr: String,
    },
    TimedOut,
    /// The binary could not be started.
    Failed(String),
}

/// Runs `docker args` to the end or until `timeout`; the child is killed on timeout.
pub(super) async fn capture(
    docker: &Path,
    args: &[&str],
    env: &[(String, String)],
    timeout: Duration,
) -> Captured {
    let mut cmd = Command::new(docker);
    cmd.args(args)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    match tokio::time::timeout(timeout, cmd.output()).await {
        Err(_) => Captured::TimedOut,
        Ok(Err(e)) => Captured::Failed(format!("{}: {e}", docker.display())),
        Ok(Ok(out)) => Captured::Done {
            ok: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        },
    }
}

/// The first lines of an error text, short enough for a message.
fn brief(text: &str) -> String {
    let t: String = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    t.chars().take(300).collect()
}

// -------------------------------------------------------------------- probe

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Probe {
    pub state: DockerState,
    pub version: Option<String>,
    pub detail: Option<String>,
    pub image_present: bool,
}

/// `docker version --format '{{.Server.Version}}'` classified (re-review RS2): permission
/// denied on the socket is `NoPermission`; any other failure to reach the daemon, or no answer
/// in time, is `NotRunning`; an unstartable binary is `Missing`.
pub(super) fn classify_version(c: &Captured) -> (DockerState, Option<String>, Option<String>) {
    match c {
        Captured::Done {
            ok: true, stdout, ..
        } => (
            DockerState::Ready,
            Some(stdout.trim().to_owned()).filter(|v| !v.is_empty()),
            None,
        ),
        Captured::Done { stderr, .. } => {
            let state = if stderr.to_lowercase().contains("permission denied") {
                DockerState::NoPermission
            } else {
                DockerState::NotRunning
            };
            (state, None, Some(brief(stderr)).filter(|d| !d.is_empty()))
        }
        Captured::TimedOut => (
            DockerState::NotRunning,
            None,
            Some("Docker did not answer within 5 seconds".into()),
        ),
        Captured::Failed(e) => (DockerState::Missing, None, Some(e.clone())),
    }
}

/// The one Docker readiness check: binary, daemon, socket permission, image present.
pub(super) async fn probe(tc: &dyn Toolchain, timeout: Duration) -> Probe {
    let Some(docker) = tc.resolve_binary("docker") else {
        return Probe {
            state: DockerState::Missing,
            version: None,
            detail: None,
            image_present: false,
        };
    };
    let env = tc.spawn_env();
    let version = capture(
        &docker,
        &["version", "--format", "{{.Server.Version}}"],
        &env,
        timeout,
    )
    .await;
    let (state, version, detail) = classify_version(&version);
    let image_present = state == DockerState::Ready
        && matches!(
            capture(&docker, &["image", "inspect", &image_tag()], &env, timeout).await,
            Captured::Done { ok: true, .. }
        );
    Probe {
        state,
        version,
        detail,
        image_present,
    }
}

/// What the user is told when an export cannot start, with the fix.
pub(super) fn not_ready_message(state: DockerState) -> &'static str {
    match state {
        DockerState::Ready => "Docker is ready.",
        DockerState::Missing => {
            "Docker is not installed. Install Docker to export apps for other systems."
        }
        DockerState::NotRunning => {
            "Docker is not running. Start it with: sudo systemctl start docker"
        }
        DockerState::NoPermission => {
            "Your user may not use Docker. Add it to the docker group with: sudo usermod -aG docker $USER, then sign out and in again."
        }
    }
}

// ----------------------------------------------------------------- docker run

pub(super) struct RunSpec<'a> {
    pub job_id: &'a str,
    /// The temporary source folder: the only host folder mounted (D7).
    pub src: &'a Path,
    pub uid: u32,
    pub gid: u32,
    pub image: &'a str,
    /// The command inside the container (`pnpm install`, `pnpm exec tauri build ...`).
    pub cmd: &'a [String],
}

/// Arguments of `docker run`: removed on exit, named for `docker stop`, the host's uid:gid,
/// one source mount, the cache volumes, never the docker socket and never `--privileged`.
pub(super) fn run_args(s: &RunSpec<'_>) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "run".into(),
        "--rm".into(),
        "--name".into(),
        container_name(s.job_id),
        "--user".into(),
        format!("{}:{}", s.uid, s.gid),
        "-v".into(),
        // `:z` relabels the Studio-owned source folder on SELinux hosts; ignored elsewhere.
        format!("{}:/src:z", s.src.display()),
        "-w".into(),
        "/src".into(),
    ];
    for (vol, mount) in VOLUMES.iter().zip(VOLUME_MOUNTS) {
        a.extend(["-v".into(), format!("{vol}:{mount}")]);
    }
    for env in [
        "HOME=/tmp/home",
        "CARGO_HOME=/cache/cargo",
        "npm_config_cache=/cache/npm",
        "npm_config_store_dir=/cache/pnpm",
        "XWIN_CACHE_DIR=/cache/xwin",
        "BUN_INSTALL_CACHE_DIR=/cache/npm/bun",
        "APPIMAGE_EXTRACT_AND_RUN=1",
        "NO_STRIP=true",
    ] {
        a.extend(["-e".into(), env.into()]);
    }
    a.push(s.image.into());
    a.extend(s.cmd.iter().cloned());
    a
}

/// The host user, so files the container writes into the source folder belong to the user.
pub(super) fn host_ids() -> (u32, u32) {
    // SAFETY: getuid and getgid take no arguments, cannot fail and touch no memory.
    unsafe { (libc::getuid(), libc::getgid()) }
}

/// `docker stop -t 3 studio-export-<job>`: killing the client does not stop the container.
pub(super) async fn stop_container(docker: &Path, env: &[(String, String)], job_id: &str) {
    let name = container_name(job_id);
    let _ = capture(docker, &["stop", "-t", "3", &name], env, STOP_TIMEOUT).await;
}

/// App startup: `docker rm -f` the containers of exports a killed Studio left (`--rm` alone
/// keeps them building until they end). In the background, as Docker may be slow or down.
pub(crate) fn remove_orphan_containers(
    docker: std::path::PathBuf,
    env: Vec<(String, String)>,
    job_ids: Vec<String>,
) {
    std::thread::spawn(move || {
        let _ = std::process::Command::new(&docker)
            .args(["rm", "-f"])
            .args(job_ids.iter().map(|j| container_name(j)))
            .env_clear()
            .envs(env)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    });
}

// ------------------------------------------------------------------- caches

/// "3.2GB", "512kB", "0B" as bytes. Docker prints decimal units; unknown text is 0.
fn parse_size(text: &str) -> f64 {
    let t = text.trim();
    let split = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(t.len());
    let n: f64 = t[..split].parse().unwrap_or(0.0);
    let mult = match t[split..].trim().to_lowercase().as_str() {
        "b" | "" => 1.0,
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "tb" => 1e12,
        "kib" => 1024.0,
        "mib" => 1024.0 * 1024.0,
        "gib" => 1024.0 * 1024.0 * 1024.0,
        _ => 0.0,
    };
    n * mult
}

/// Bytes of the export volumes and images in `docker system df -v --format '{{json .}}'`.
fn cache_bytes(stdout: &str) -> Option<f64> {
    let doc: Value = stdout
        .lines()
        .find_map(|l| serde_json::from_str(l).ok())
        .or_else(|| serde_json::from_str(stdout).ok())?;
    let size = |v: &Value| parse_size(v.get("Size").and_then(Value::as_str).unwrap_or(""));
    let list = |key: &str| {
        doc.get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let volumes: f64 = list("Volumes")
        .iter()
        .filter(|v| {
            v.get("Name")
                .and_then(Value::as_str)
                .is_some_and(|n| VOLUMES.contains(&n))
        })
        .map(size)
        .sum();
    let images: f64 = list("Images")
        .iter()
        .filter(|v| v.get("Repository").and_then(Value::as_str) == Some(IMAGE_REPO))
        .map(size)
        .sum();
    Some(volumes + images)
}

pub(super) const CACHE_READ_ERROR: &str = "Couldn't read Docker volumes";

/// Space used by the export caches and images; `Err` is the text to show.
pub(super) async fn cache_size(
    docker: &Path,
    env: &[(String, String)],
    timeout: Duration,
) -> Result<f64, &'static str> {
    match capture(
        docker,
        &["system", "df", "-v", "--format", "{{json .}}"],
        env,
        timeout,
    )
    .await
    {
        Captured::Done {
            ok: true, stdout, ..
        } => cache_bytes(&stdout).ok_or(CACHE_READ_ERROR),
        _ => Err(CACHE_READ_ERROR),
    }
}

/// Removes the cache volumes and every export image.
pub(super) async fn clear_caches(docker: &Path, env: &[(String, String)]) -> Result<(), String> {
    let mut args = vec!["volume", "rm", "-f"];
    args.extend(VOLUMES);
    rm_result(
        "export caches",
        &capture(docker, &args, env, RM_TIMEOUT).await,
    )?;
    remove_images(docker, env, None).await
}

fn rm_result(what: &str, c: &Captured) -> Result<(), String> {
    match c {
        Captured::Done { ok: true, .. } => Ok(()),
        Captured::Done { stderr, .. } => {
            Err(format!("Couldn't remove the {what}: {}", brief(stderr)))
        }
        Captured::TimedOut => Err(format!("Couldn't remove the {what}: Docker did not answer")),
        Captured::Failed(e) => Err(format!("Couldn't remove the {what}: {e}")),
    }
}

/// Removes the export images except `keep`: after a Studio update builds a new image, the old
/// one is 2-3 GB nothing uses.
pub(super) async fn remove_images(
    docker: &Path,
    env: &[(String, String)],
    keep: Option<&str>,
) -> Result<(), String> {
    let ls = capture(
        docker,
        &[
            "image",
            "ls",
            "--format",
            "{{.Repository}}:{{.Tag}}",
            "--filter",
            &format!("reference={IMAGE_REPO}"),
        ],
        env,
        RM_TIMEOUT,
    )
    .await;
    let Captured::Done {
        ok: true, stdout, ..
    } = ls
    else {
        return Err("Couldn't list the export images".into());
    };
    let tags: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|t| t.starts_with(IMAGE_REPO) && Some(*t) != keep)
        .collect();
    if tags.is_empty() {
        return Ok(());
    }
    let mut args = vec!["image", "rm", "-f"];
    args.extend(tags);
    rm_result(
        "export image",
        &capture(docker, &args, env, RM_TIMEOUT).await,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done(ok: bool, stdout: &str, stderr: &str) -> Captured {
        Captured::Done {
            ok,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn version_output_is_classified() {
        let (s, v, _) = classify_version(&done(true, "27.1.1\n", ""));
        assert_eq!((s, v.as_deref()), (DockerState::Ready, Some("27.1.1")));
        let (s, _, d) = classify_version(&done(
            false,
            "",
            "permission denied while trying to connect to the Docker daemon socket at unix:///var/run/docker.sock",
        ));
        assert_eq!(s, DockerState::NoPermission);
        assert!(d.unwrap().contains("permission denied"));
        let (s, _, _) = classify_version(&done(
            false,
            "",
            "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?",
        ));
        assert_eq!(s, DockerState::NotRunning);
        assert_eq!(
            classify_version(&Captured::TimedOut).0,
            DockerState::NotRunning
        );
        assert_eq!(
            classify_version(&Captured::Failed("x".into())).0,
            DockerState::Missing
        );
    }

    #[test]
    fn image_tag_follows_the_dockerfile() {
        let t = image_tag();
        let (repo, hash) = t.split_once(':').unwrap();
        assert_eq!(repo, "ai-app-studio-export");
        assert_eq!(hash.len(), 12);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn run_args_golden() {
        let cmd = vec!["pnpm".to_owned(), "install".to_owned()];
        let args = run_args(&RunSpec {
            job_id: "j1",
            src: Path::new("/data/tmp/export-j1"),
            uid: 1000,
            gid: 1001,
            image: "ai-app-studio-export:abc",
            cmd: &cmd,
        });
        let want = [
            "run",
            "--rm",
            "--name",
            "studio-export-j1",
            "--user",
            "1000:1001",
            "-v",
            "/data/tmp/export-j1:/src:z",
            "-w",
            "/src",
            "-v",
            "ai-app-studio-export-cargo:/cache/cargo",
            "-v",
            "ai-app-studio-export-pnpm:/cache/pnpm",
            "-v",
            "ai-app-studio-export-npm:/cache/npm",
            "-v",
            "ai-app-studio-export-xwin:/cache/xwin",
            "-e",
            "HOME=/tmp/home",
            "-e",
            "CARGO_HOME=/cache/cargo",
            "-e",
            "npm_config_cache=/cache/npm",
            "-e",
            "npm_config_store_dir=/cache/pnpm",
            "-e",
            "XWIN_CACHE_DIR=/cache/xwin",
            "-e",
            "BUN_INSTALL_CACHE_DIR=/cache/npm/bun",
            "-e",
            "APPIMAGE_EXTRACT_AND_RUN=1",
            "-e",
            "NO_STRIP=true",
            "ai-app-studio-export:abc",
            "pnpm",
            "install",
        ];
        assert_eq!(args, want);
        // The safety properties, stated on their own.
        let joined = args.join(" ");
        assert!(!joined.contains("docker.sock"));
        assert!(!joined.contains("--privileged"));
        assert_eq!(args.iter().filter(|a| a.ends_with(":/src:z")).count(), 1);
    }

    #[test]
    fn sizes_and_cache_totals_are_parsed() {
        assert_eq!(parse_size("3.2GB"), 3.2e9);
        assert_eq!(parse_size("512kB"), 512e3);
        assert_eq!(parse_size("0B"), 0.0);
        assert_eq!(parse_size("N/A"), 0.0);
        let doc = r#"{"Images":[{"Repository":"ai-app-studio-export","Tag":"abc","Size":"2GB"},{"Repository":"other","Size":"9GB"}],"Volumes":[{"Name":"ai-app-studio-export-cargo","Size":"500MB"},{"Name":"ai-app-studio-export-xwin","Size":"1GB"},{"Name":"mine","Size":"7GB"}]}"#;
        assert_eq!(cache_bytes(doc), Some(3.5e9));
        assert_eq!(cache_bytes("not json"), None);
        assert_eq!(cache_bytes("{}"), Some(0.0));
    }
}
