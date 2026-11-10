//! Studio's own updates: asks GitHub which release is the latest, and replaces the running
//! AppImage with it. Uses the system `curl`, like the other services use `git` and `gh`.
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use specta::Type;
use tokio::process::Command;

use crate::platform::{appimage::host_env, process_env};

const REPO: &str = "https://github.com/altafino/crazy";

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("Couldn't reach GitHub. {0}")]
    Network(String),
    #[error("GitHub didn't name a latest release.")]
    NoRelease,
    #[error(
        "This copy of Studio can't update itself. Download the new version from the release page."
    )]
    NotInstallable,
    #[error("The download doesn't match its checksum. Nothing was changed.")]
    Checksum,
    #[error("Couldn't write {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// What a check found.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current: String,
    pub latest: String,
    pub newer: bool,
    /// Studio runs from an AppImage it can replace. False for dev builds: they only get the notice.
    pub can_install: bool,
    pub release_url: String,
}

/// `x.y.z`, digits only: the tag also goes into a URL and a file name, so nothing else passes.
fn parse(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.split('.').map(|p| {
        p.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| p.parse().ok())
            .flatten()
    });
    let v = (it.next()??, it.next()??, it.next()??);
    it.next().is_none().then_some(v)
}

/// The version in the URL that `/releases/latest` redirects to (`…/releases/tag/v0.4.1`).
fn version_in(url: &str) -> Option<&str> {
    let v = url.trim().rsplit_once("/releases/tag/v")?.1;
    parse(v).map(|_| v)
}

/// The AppImage Studio runs from, when it can be replaced.
fn appimage() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("APPIMAGE")?);
    (cfg!(target_arch = "x86_64") && p.is_file()).then_some(p)
}

async fn curl(args: &[&str]) -> Result<String, UpdateError> {
    let out = Command::new("curl")
        .args(["--fail", "--silent", "--show-error", "--location"])
        .args(args)
        .env_clear()
        .envs(host_env(&process_env()))
        .output()
        .await
        .map_err(|e| UpdateError::Network(format!("curl didn't start: {e}")))?;
    if !out.status.success() {
        return Err(UpdateError::Network(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Asks GitHub for the latest release. One request to the releases page, no API and no token.
pub async fn check() -> Result<UpdateStatus, UpdateError> {
    let current = env!("CARGO_PKG_VERSION");
    let latest_url = format!("{REPO}/releases/latest");
    let url = curl(&[
        "--max-time",
        "20",
        "--output",
        "/dev/null",
        "--write-out",
        "%{url_effective}",
        &latest_url,
    ])
    .await?;
    let latest = version_in(&url).ok_or(UpdateError::NoRelease)?;
    Ok(UpdateStatus {
        current: current.into(),
        latest: latest.into(),
        newer: parse(latest) > parse(current),
        can_install: appimage().is_some(),
        release_url: format!("{REPO}/releases/tag/v{latest}"),
    })
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> UpdateError + '_ {
    move |source| UpdateError::Io {
        path: path.display().to_string(),
        source,
    }
}

/// Downloads the release's AppImage next to the running one, checks it against the published
/// SHA-256 and moves it over the running file. The running process keeps the old file open; the
/// next start uses the new one.
// ponytail: the file keeps its name, so a name with the old version in it stays (AppImageLauncher
// adds the version and a hash). Rename and re-integrate if that ever confuses someone.
pub async fn install(version: &str) -> Result<(), UpdateError> {
    parse(version).ok_or(UpdateError::NoRelease)?;
    let target = appimage().ok_or(UpdateError::NotInstallable)?;
    let asset =
        format!("{REPO}/releases/download/v{version}/AI-App-Studio_{version}_x86_64.AppImage");
    let sums = curl(&["--max-time", "30", &format!("{asset}.sha256")]).await?;
    let want = sums
        .split_whitespace()
        .next()
        .filter(|h| h.len() == 64)
        .ok_or(UpdateError::Checksum)?
        .to_lowercase();

    // Same folder as the target, so the final rename is atomic.
    let part = target.with_extension("update-part");
    let done = async {
        curl(&[
            "--max-time",
            "900",
            "--output",
            &part.to_string_lossy(),
            &asset,
        ])
        .await?;
        let file = part.clone();
        let got = tokio::task::spawn_blocking(move || -> std::io::Result<String> {
            let mut hasher = Sha256::new();
            std::io::copy(&mut std::fs::File::open(&file)?, &mut hasher)?;
            Ok(format!("{:x}", hasher.finalize()))
        })
        .await
        .map_err(|e| UpdateError::Network(e.to_string()))?
        .map_err(io(&part))?;
        if got != want {
            return Err(UpdateError::Checksum);
        }
        std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755))
            .map_err(io(&part))?;
        std::fs::rename(&part, &target).map_err(io(&target))
    }
    .await;
    if done.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_version_from_the_redirect_and_refuses_anything_else() {
        assert_eq!(
            version_in("https://github.com/altafino/crazy/releases/tag/v0.4.1\n"),
            Some("0.4.1")
        );
        assert_eq!(
            version_in("https://github.com/altafino/crazy/releases"),
            None
        ); // no release yet
        assert_eq!(version_in("https://x/releases/tag/v1.2"), None);
        assert_eq!(version_in("https://x/releases/tag/v1.2.3/../../evil"), None);
        assert_eq!(version_in("https://x/releases/tag/v1.2.3-rc1"), None);
    }

    #[test]
    fn compares_versions_by_number_not_by_text() {
        assert!(parse("0.10.0") > parse("0.9.9"));
        assert!(parse("0.4.1") > parse("0.4.0"));
        assert!(!(parse("0.4.1") > parse("0.4.1")));
        assert_eq!(parse("+1.2.3"), None);
    }

    #[tokio::test]
    async fn install_refuses_a_version_that_is_not_plain_numbers() {
        assert!(matches!(
            install("1.2.3/../x").await,
            Err(UpdateError::NoRelease)
        ));
    }
}
