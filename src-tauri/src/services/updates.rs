//! Studio's own updates: asks GitHub which release is the latest, and replaces the running
//! AppImage with it. Uses the system `curl`, like the other services use `git` and `gh`.
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
    #[error("The new version was downloaded, but AppImageLauncher couldn't install it. {0}")]
    Integrate(String),
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

/// Where the updated Studio is, for Restart now. Set by a finished install.
static RELAUNCH: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn relaunch_path() -> Option<PathBuf> {
    RELAUNCH.lock().ok()?.clone()
}

/// AppImageLauncher's CLI, when installed: it owns the menu entries of the AppImages it manages.
fn ail_cli() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("ail-cli"))
        .find(|p| p.is_file())
}

/// `<product>_<current>_<rest>`: the running file's name around its version. None when the name
/// doesn't carry the version; such a file is replaced in place under its own name.
fn name_parts<'a>(name: &'a str, current: &str) -> Option<(&'a str, &'a str)> {
    name.split_once(&format!("_{current}_"))
}

/// Without AppImageLauncher: the verified download takes a name with the new version, launcher
/// entries that start the old file are pointed at the new one, and the old file goes.
fn finish_plain(
    stage: &Path,
    target: &Path,
    current: &str,
    version: &str,
    applications: &Path,
) -> Result<PathBuf, UpdateError> {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let Some((product, rest)) = name_parts(&name, current).filter(|_| current != version) else {
        std::fs::rename(stage, target).map_err(io(target))?;
        return Ok(target.to_path_buf());
    };
    let new = target.with_file_name(format!("{product}_{version}_{rest}"));
    std::fs::rename(stage, &new).map_err(io(&new))?;
    let (old_text, new_text) = (target.to_string_lossy(), new.to_string_lossy());
    for entry in std::fs::read_dir(applications)
        .into_iter()
        .flatten()
        .flatten()
    {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "desktop") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if text.contains(old_text.as_ref()) {
            std::fs::write(&path, text.replace(old_text.as_ref(), &new_text)).map_err(io(&path))?;
        }
    }
    let _ = std::fs::remove_file(target);
    Ok(new)
}

/// With AppImageLauncher, like `make install`: it moves the download into its folder under a
/// name with the new version and makes the menu entry; then the old file is taken out.
async fn finish_ail(
    ail: &Path,
    stage: &Path,
    target: &Path,
    prefix: &str,
) -> Result<PathBuf, UpdateError> {
    let env = host_env(&process_env());
    let run = |verb: &'static str, file: PathBuf| {
        Command::new(ail)
            .arg(verb)
            .arg(file)
            .env_clear()
            .envs(env.clone())
            .output()
    };
    let out = run("integrate", stage.to_path_buf())
        .await
        .map_err(io(ail))?;
    let dir = target.parent().unwrap_or(Path::new("."));
    // AppImageLauncher adds a hash to the name: take the newest file of the new version.
    let new = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p != target
                && p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with(prefix))
        })
        .max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok());
    let Some(new) = new.filter(|_| out.status.success()) else {
        return Err(UpdateError::Integrate(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    };
    let _ = run("unintegrate", target.to_path_buf()).await;
    let _ = std::fs::remove_file(target);
    Ok(new)
}

/// Downloads the release's AppImage, checks it against the published SHA-256 and installs it in
/// place of the running one, under a name with the new version. The running process keeps the
/// old file open; Restart now (or the next start) uses the new one.
pub async fn install(version: &str) -> Result<(), UpdateError> {
    parse(version).ok_or(UpdateError::NoRelease)?;
    let target = appimage().ok_or(UpdateError::NotInstallable)?;
    let current = env!("CARGO_PKG_VERSION");
    let asset =
        format!("{REPO}/releases/download/v{version}/AI-App-Studio_{version}_x86_64.AppImage");
    let sums = curl(&["--max-time", "30", &format!("{asset}.sha256")]).await?;
    let want = sums
        .split_whitespace()
        .next()
        .filter(|h| h.len() == 64)
        .ok_or(UpdateError::Checksum)?
        .to_lowercase();

    let name = target
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    // AppImageLauncher takes a file from outside its folder; otherwise the download lands next
    // to the target, so the final rename is atomic.
    let ail = ail_cli()
        .zip(name_parts(&name, current))
        .map(|(cli, (product, _))| (cli, format!("{product}_{version}_")));
    let stage = match &ail {
        Some((_, prefix)) => {
            let dir =
                std::env::temp_dir().join(format!("ai-app-studio-update-{}", std::process::id()));
            std::fs::create_dir_all(&dir).map_err(io(&dir))?;
            dir.join(format!("{prefix}amd64.AppImage"))
        }
        None => target.with_extension("update-part"),
    };
    let done = async {
        curl(&[
            "--max-time",
            "900",
            "--output",
            &stage.to_string_lossy(),
            &asset,
        ])
        .await?;
        let file = stage.clone();
        let got = tokio::task::spawn_blocking(move || -> std::io::Result<String> {
            let mut hasher = Sha256::new();
            std::io::copy(&mut std::fs::File::open(&file)?, &mut hasher)?;
            Ok(format!("{:x}", hasher.finalize()))
        })
        .await
        .map_err(|e| UpdateError::Network(e.to_string()))?
        .map_err(io(&stage))?;
        if got != want {
            return Err(UpdateError::Checksum);
        }
        std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o755))
            .map_err(io(&stage))?;
        match &ail {
            Some((cli, prefix)) => finish_ail(cli, &stage, &target, prefix).await,
            None => {
                let apps = crate::platform::paths::StudioPaths::from_process_env()
                    .map(|p| p.applications_dir)
                    .unwrap_or_default();
                finish_plain(&stage, &target, current, version, &apps)
            }
        }
    }
    .await;
    let _ = std::fs::remove_file(&stage);
    if ail.is_some() {
        let _ = stage.parent().map(std::fs::remove_dir); // the folder made for AppImageLauncher
    }
    if let Ok(mut slot) = RELAUNCH.lock() {
        *slot = done.as_ref().ok().cloned();
    }
    done.map(|_| ())
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

    #[test]
    fn a_plain_install_takes_the_new_version_in_its_name_and_fixes_the_menu_entry() {
        let dir = tempfile::tempdir().unwrap();
        let apps = dir.path().join("applications");
        std::fs::create_dir(&apps).unwrap();
        let old = dir.path().join("AI App Studio_0.5.0_amd64.AppImage");
        let stage = old.with_extension("update-part");
        std::fs::write(&old, "old").unwrap();
        std::fs::write(&stage, "new").unwrap();
        let entry = apps.join("ai-app-studio-appimage.desktop");
        std::fs::write(
            &entry,
            format!("[Desktop Entry]\nExec=\"{}\"\n", old.display()),
        )
        .unwrap();
        std::fs::write(apps.join("other.desktop"), "Exec=/usr/bin/other\n").unwrap();

        let new = finish_plain(&stage, &old, "0.5.0", "0.6.0", &apps).unwrap();

        assert_eq!(new, dir.path().join("AI App Studio_0.6.0_amd64.AppImage"));
        assert_eq!(std::fs::read_to_string(&new).unwrap(), "new");
        assert!(!old.exists() && !stage.exists());
        assert!(
            std::fs::read_to_string(&entry)
                .unwrap()
                .contains("AI App Studio_0.6.0_amd64.AppImage")
        );
        assert_eq!(
            std::fs::read_to_string(apps.join("other.desktop")).unwrap(),
            "Exec=/usr/bin/other\n"
        );
    }

    #[test]
    fn a_file_without_the_version_in_its_name_is_replaced_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("studio.AppImage");
        let stage = old.with_extension("update-part");
        std::fs::write(&old, "old").unwrap();
        std::fs::write(&stage, "new").unwrap();
        let new = finish_plain(&stage, &old, "0.5.0", "0.6.0", dir.path()).unwrap();
        assert_eq!(new, old);
        assert_eq!(std::fs::read_to_string(&old).unwrap(), "new");
    }

    #[tokio::test]
    async fn install_refuses_a_version_that_is_not_plain_numbers() {
        assert!(matches!(
            install("1.2.3/../x").await,
            Err(UpdateError::NoRelease)
        ));
    }
}
