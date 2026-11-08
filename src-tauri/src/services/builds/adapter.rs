//! The build adapter (PRD M4 step 4, M4-D2): how one kind of project becomes one artifact.
//! The Tauri AppImage adapter owns the commands, the environment and where the artifact lands;
//! `BuildService` owns the job, the source record, verification, storage and the smoke test.

use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::{Duration, SystemTime};

use tokio::sync::watch;

use super::BuildPhase;
use crate::services::jobs::proc::{LineSink, ProcEnd, ProcSpec, run_proc};
use crate::services::preview::Toolchain;
use crate::services::projects::tauri::{self as project, PackageManager};

/// How long `tauri build` gets to exit after Stop (SIGTERM, then SIGKILL).
const STOP_GRACE: Duration = Duration::from_secs(3);
/// File times come from the kernel's coarse clock, which can read a little behind
/// `SystemTime::now()`; an artifact written right after the build started must still count.
const MTIME_SLACK: Duration = Duration::from_secs(2);

/// Why a build did not produce a release; the strings are what `jobs.error_kind` stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FailureKind {
    Preflight,
    InstallFailed,
    BuildFailed,
    NoArtifact,
    VerifyFailed,
    StoreFailed,
    Cancelled,
}

impl FailureKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Preflight => "preflight",
            Self::InstallFailed => "installFailed",
            Self::BuildFailed => "buildFailed",
            Self::NoArtifact => "noArtifact",
            Self::VerifyFailed => "verifyFailed",
            Self::StoreFailed => "storeFailed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BuildFailure {
    pub kind: FailureKind,
    pub message: String,
    pub exit_code: Option<i32>,
}

impl BuildFailure {
    pub(crate) fn new(kind: FailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            exit_code: None,
        }
    }

    pub(crate) fn cancelled() -> Self {
        Self::new(FailureKind::Cancelled, "The build was stopped")
    }
}

/// The artifact a build produced, still in the project's build directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BuiltApp {
    pub artifact: PathBuf,
}

/// Where an adapter reports: output lines (a `LineSink`), its phase, and notes of its own.
pub(crate) trait BuildOutput: Send + Sync {
    fn line(&self, raw: &str);
    fn flush(&self);
    fn phase(&self, phase: BuildPhase);
    /// The first error line of the output so far.
    fn first_error(&self) -> Option<String>;
}

pub(crate) struct BuildCtx<'a> {
    pub project_dir: &'a Path,
    pub version: &'a str,
    pub toolchain: &'a dyn Toolchain,
    pub out: &'a dyn BuildOutput,
    pub cancel: &'a mut watch::Receiver<bool>,
}

pub(crate) type BuildFuture<'a> =
    Pin<Box<dyn Future<Output = Result<BuiltApp, BuildFailure>> + Send + 'a>>;

/// One async step: build `ctx.version` of the project in `ctx.project_dir`.
pub(crate) trait BuildAdapter: Send + Sync + 'static {
    fn build<'a>(&'a self, ctx: BuildCtx<'a>) -> BuildFuture<'a>;
}

struct Sink<'a>(&'a dyn BuildOutput);

impl LineSink for Sink<'_> {
    fn line(&self, raw: &str) {
        self.0.line(raw);
    }
    fn flush(&self) {
        self.0.flush();
    }
}

/// `tauri build --bundles appimage` for a Tauri + Vue project.
pub(crate) struct TauriAppImageAdapter;

impl TauriAppImageAdapter {
    /// The version override: `{"version":"X"}` (M4-D8; source files are never edited).
    pub(super) fn version_config(version: &str) -> String {
        serde_json::json!({ "version": version }).to_string()
    }

    fn build_args(manager: PackageManager, version: &str) -> Vec<String> {
        let mut args = manager.tauri_build_args("appimage");
        args.extend(["--config".to_owned(), Self::version_config(version)]);
        args
    }

    /// The build environment: the toolchain's `spawn_env` plus `NO_STRIP=true` (M4-D9), and the
    /// cached AppImage runtime when there is one (see [`runtime_cache`]).
    fn build_env(base: Vec<(String, String)>) -> Vec<(String, String)> {
        let runtime = runtime_cache(&base).filter(|p| p.metadata().is_ok_and(|m| m.len() > 0));
        let mut env: Vec<_> = base
            .into_iter()
            .filter(|(k, _)| k != "NO_STRIP" && k != "LDAI_RUNTIME_FILE")
            .collect();
        env.push(("NO_STRIP".into(), "true".into()));
        if let Some(rt) = runtime {
            env.push(("LDAI_RUNTIME_FILE".into(), rt.display().to_string()));
        }
        env
    }

    async fn run(
        ctx: &mut BuildCtx<'_>,
        program: &Path,
        args: &[String],
        env: &[(String, String)],
    ) -> ProcEnd {
        run_proc(
            ProcSpec {
                program,
                args,
                env,
                cwd: ctx.project_dir,
            },
            &Sink(ctx.out),
            ctx.cancel,
            STOP_GRACE,
        )
        .await
    }

    async fn build_inner(&self, mut ctx: BuildCtx<'_>) -> Result<BuiltApp, BuildFailure> {
        let started = SystemTime::now();
        let dir = ctx.project_dir;
        let cfg = project::read_tauri_config(dir).ok_or_else(|| {
            BuildFailure::new(
                FailureKind::BuildFailed,
                "Could not read src-tauri/tauri.conf.json",
            )
        })?;
        if !cfg.bundle_active {
            return Err(BuildFailure::new(
                FailureKind::NoArtifact,
                "No AppImage was produced: bundle.active is false in tauri.conf.json",
            ));
        }
        let manager = project::detect_package_manager(dir);
        let program = ctx
            .toolchain
            .resolve_binary(manager.name())
            .unwrap_or_else(|| manager.name().into());
        let base_env = ctx.toolchain.spawn_env();

        ctx.out.phase(BuildPhase::Installing);
        let install: Vec<String> = manager
            .install_args()
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        match Self::run(&mut ctx, &program, &install, &base_env).await {
            ProcEnd::Cancelled => return Err(BuildFailure::cancelled()),
            ProcEnd::Exited(st) if st.success() => {}
            ProcEnd::Exited(_) | ProcEnd::SpawnFailed(_) => {
                return Err(BuildFailure::new(
                    FailureKind::InstallFailed,
                    "Installing dependencies failed",
                ));
            }
        }

        ctx.out.phase(BuildPhase::Compiling);
        let env = Self::build_env(base_env);
        let args = Self::build_args(manager, ctx.version);
        match Self::run(&mut ctx, &program, &args, &env).await {
            ProcEnd::Cancelled => return Err(BuildFailure::cancelled()),
            ProcEnd::SpawnFailed(e) => {
                return Err(BuildFailure::new(
                    FailureKind::BuildFailed,
                    format!("Could not start the build: {e}"),
                ));
            }
            ProcEnd::Exited(st) if st.success() => {}
            ProcEnd::Exited(st) => {
                let message = ctx.out.first_error().unwrap_or_else(|| match st.code() {
                    Some(c) => format!("The build stopped with exit code {c}."),
                    None => "The build was stopped by a signal.".into(),
                });
                return Err(BuildFailure {
                    kind: FailureKind::BuildFailed,
                    message,
                    exit_code: st.code(),
                });
            }
        }
        let artifact = locate_artifact(dir, &env, started)?;
        save_runtime(&artifact, &env);
        Ok(BuiltApp { artifact })
    }
}

impl BuildAdapter for TauriAppImageAdapter {
    fn build<'a>(&'a self, ctx: BuildCtx<'a>) -> BuildFuture<'a> {
        Box::pin(self.build_inner(ctx))
    }
}

/// Where Tauri writes one kind of bundle below the cargo target directory, and what it is
/// called in messages. The one table behind release builds and exports (F22 T1).
pub(crate) struct BundleSpec {
    /// Relative to the cargo target directory.
    pub dir: &'static str,
    /// The file name ends with this.
    pub suffix: &'static str,
    pub label: &'static str,
}

pub(crate) const APPIMAGE: BundleSpec = BundleSpec {
    dir: "release/bundle/appimage",
    suffix: ".AppImage",
    label: "AppImage",
};
/// What the Linux export makes (`--bundles appimage,deb,rpm`).
pub(crate) const LINUX_BUNDLES: [BundleSpec; 3] = [
    APPIMAGE,
    BundleSpec {
        dir: "release/bundle/deb",
        suffix: ".deb",
        label: "deb package",
    },
    BundleSpec {
        dir: "release/bundle/rpm",
        suffix: ".rpm",
        label: "rpm package",
    },
];
/// What the Windows export makes (`--target x86_64-pc-windows-msvc --bundles nsis`).
pub(crate) const WINDOWS_BUNDLES: [BundleSpec; 1] = [BundleSpec {
    dir: "x86_64-pc-windows-msvc/release/bundle/nsis",
    suffix: "-setup.exe",
    label: "Windows installer",
}];

/// `$CARGO_TARGET_DIR` from the build environment (a relative value is taken from the project
/// folder), else `<project>/src-tauri/target`.
fn cargo_target_dir(project_dir: &Path, env: &[(String, String)]) -> PathBuf {
    match env.iter().find(|(k, _)| k == "CARGO_TARGET_DIR") {
        Some((_, v)) if !v.is_empty() => project_dir.join(v), // absolute values replace the base
        _ => project_dir.join("src-tauri/target"),
    }
}

/// A11: exactly one file of this bundle kind, modified since the build started. A stale file
/// never counts.
pub(crate) fn locate_bundle(
    project_dir: &Path,
    env: &[(String, String)],
    started: SystemTime,
    spec: &BundleSpec,
) -> Result<PathBuf, BuildFailure> {
    let bundle_dir = cargo_target_dir(project_dir, env).join(spec.dir);
    let cutoff = started.checked_sub(MTIME_SLACK).unwrap_or(started);
    let mut fresh: Vec<PathBuf> = fs::read_dir(&bundle_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(spec.suffix))
        .filter(|e| {
            e.metadata()
                .is_ok_and(|m| m.is_file() && m.modified().is_ok_and(|t| t >= cutoff))
        })
        .map(|e| e.path())
        .collect();
    fresh.sort();
    let label = spec.label;
    match fresh.len() {
        1 => Ok(fresh.remove(0)),
        0 => Err(BuildFailure::new(
            FailureKind::NoArtifact,
            format!("No {label} was produced"),
        )),
        n => Err(BuildFailure::new(
            FailureKind::NoArtifact,
            format!("No {label} was produced: found {n} new {label} files, expected exactly one"),
        )),
    }
}

/// The release build's AppImage: `locate_bundle` with the AppImage row.
/// appimagetool downloads the AppImage runtime from GitHub on every build and fails while GitHub
/// does (504s on 2026-11-08). One copy lives here, shared with `make appimage`: it is handed over
/// as `LDAI_RUNTIME_FILE` and saved from the first AppImage that builds without one.
fn runtime_cache(env: &[(String, String)]) -> Option<PathBuf> {
    let get = |k: &str| {
        env.iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| PathBuf::from(v))
            .filter(|p| p.is_absolute())
    };
    let cache = get("XDG_CACHE_HOME").or_else(|| get("HOME").map(|h| h.join(".cache")))?;
    Some(
        cache
            .join("tauri")
            .join(format!("runtime-{}", std::env::consts::ARCH)),
    )
}

/// The runtime at the front of a type-2 AppImage: everything before the squashfs, which starts
/// where the ELF section headers end. None for anything else, and for a runtime carrying update
/// information (it would point every later app at this one's updates).
fn runtime_of(appimage: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut f = fs::File::open(appimage).ok()?;
    let mut h = [0u8; 64];
    f.read_exact(&mut h).ok()?;
    // ELF, 64-bit, little-endian, AppImage type 2.
    if &h[..4] != b"\x7fELF" || h[4] != 2 || h[5] != 1 || &h[8..11] != b"AI\x02" {
        return None;
    }
    let shoff = u64::from_le_bytes(h[0x28..0x30].try_into().ok()?);
    let size = shoff
        + u64::from(u16::from_le_bytes([h[0x3A], h[0x3B]]))
            * u64::from(u16::from_le_bytes([h[0x3C], h[0x3D]]));
    // The runtime is about 1 MB; anything far larger is not one.
    if !(64..=16 << 20).contains(&size) || size >= f.metadata().ok()?.len() {
        return None;
    }
    let mut runtime = h.to_vec();
    f.take(size - 64).read_to_end(&mut runtime).ok()?;
    (runtime.len() as u64 == size && !runtime.windows(6).any(|w| w == b"zsync|")).then_some(runtime)
}

/// Keep the runtime of a finished build when none is cached. Best effort: a failure only means
/// the next build downloads it again. Skipped for signed builds, whose runtime holds a signature.
fn save_runtime(artifact: &Path, env: &[(String, String)]) {
    let signed = env
        .iter()
        .any(|(k, v)| (k == "SIGN" || k == "LDAI_SIGN") && !v.is_empty());
    let Some(dest) = runtime_cache(env).filter(|p| !p.exists() && !signed) else {
        return;
    };
    let Some(runtime) = runtime_of(artifact) else {
        return;
    };
    let part = dest.with_extension("part");
    let saved = dest.parent().is_some_and(|d| fs::create_dir_all(d).is_ok())
        && fs::write(&part, runtime).is_ok()
        && fs::rename(&part, &dest).is_ok();
    if !saved {
        let _ = fs::remove_file(&part);
    }
}

pub(crate) fn locate_artifact(
    project_dir: &Path,
    env: &[(String, String)],
    started: SystemTime,
) -> Result<PathBuf, BuildFailure> {
    locate_bundle(project_dir, env, started, &APPIMAGE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn bundle(root: &Path) -> PathBuf {
        let d = root.join("src-tauri/target/release/bundle/appimage");
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn touch(path: &Path, age: Duration) {
        let f = File::create(path).unwrap();
        f.set_modified(SystemTime::now() - age).unwrap();
    }

    /// A 300-byte type-2 AppImage whose section headers end at 228: the runtime is bytes 0..228.
    fn fake_appimage(path: &Path, payload: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; 300];
        b[..4].copy_from_slice(b"\x7fELF");
        b[4] = 2;
        b[5] = 1;
        b[8..11].copy_from_slice(b"AI\x02");
        b[0x28..0x30].copy_from_slice(&100u64.to_le_bytes());
        b[0x3A..0x3C].copy_from_slice(&64u16.to_le_bytes());
        b[0x3C..0x3E].copy_from_slice(&2u16.to_le_bytes());
        b[64..64 + payload.len()].copy_from_slice(payload);
        fs::write(path, &b).unwrap();
        b[..228].to_vec()
    }

    fn xdg_env(cache: &Path) -> Vec<(String, String)> {
        vec![("XDG_CACHE_HOME".into(), cache.display().to_string())]
    }

    fn cached(cache: &Path) -> PathBuf {
        cache
            .join("tauri")
            .join(format!("runtime-{}", std::env::consts::ARCH))
    }

    #[test]
    fn a_cached_runtime_is_handed_to_the_build() {
        let d = tempfile::tempdir().unwrap();
        let ldai = |env: Vec<(String, String)>| {
            TauriAppImageAdapter::build_env(env)
                .into_iter()
                .find(|(k, _)| k == "LDAI_RUNTIME_FILE")
                .map(|(_, v)| v)
        };
        assert_eq!(
            ldai(xdg_env(d.path())),
            None,
            "no cached runtime: appimagetool downloads it"
        );
        fs::create_dir_all(cached(d.path()).parent().unwrap()).unwrap();
        fs::write(cached(d.path()), b"").unwrap();
        assert_eq!(
            ldai(xdg_env(d.path())),
            None,
            "an empty file is not a runtime"
        );
        fs::write(cached(d.path()), b"runtime").unwrap();
        let want = cached(d.path()).display().to_string();
        assert_eq!(ldai(xdg_env(d.path())).as_deref(), Some(want.as_str()));
        // Without XDG_CACHE_HOME the cache is ~/.cache.
        let home = vec![("HOME".into(), "/home/u".into())];
        assert_eq!(
            runtime_cache(&home),
            Some(cached(Path::new("/home/u/.cache")))
        );
    }

    #[test]
    fn the_runtime_of_the_first_build_is_kept_and_never_replaced() {
        let d = tempfile::tempdir().unwrap();
        let app = d.path().join("app.AppImage");
        let runtime = fake_appimage(&app, b"first");
        save_runtime(&app, &xdg_env(d.path()));
        assert_eq!(fs::read(cached(d.path())).unwrap(), runtime);
        fake_appimage(&app, b"second");
        save_runtime(&app, &xdg_env(d.path()));
        assert_eq!(fs::read(cached(d.path())).unwrap(), runtime);
    }

    #[test]
    fn no_runtime_is_kept_from_a_non_appimage_an_updatable_one_or_a_signed_build() {
        let d = tempfile::tempdir().unwrap();
        let app = d.path().join("app.AppImage");
        fake_appimage(&app, b"gh-releases-zsync|me|app|latest|app.zsync");
        save_runtime(&app, &xdg_env(d.path()));
        assert!(
            !cached(d.path()).exists(),
            "update information would leak into other apps"
        );

        fake_appimage(&app, b"");
        let mut signed = xdg_env(d.path());
        signed.push(("SIGN".into(), "1".into()));
        save_runtime(&app, &signed);
        assert!(
            !cached(d.path()).exists(),
            "a signed build's runtime holds its signature"
        );

        let mut not_appimage = fs::read(&app).unwrap();
        not_appimage[8..11].fill(0);
        fs::write(&app, not_appimage).unwrap();
        save_runtime(&app, &xdg_env(d.path()));
        assert!(!cached(d.path()).exists());
    }

    #[test]
    fn args_and_env() {
        assert_eq!(
            TauriAppImageAdapter::build_args(PackageManager::Pnpm, "1.2.3"),
            [
                "exec",
                "tauri",
                "build",
                "--bundles",
                "appimage",
                "--config",
                r#"{"version":"1.2.3"}"#
            ]
        );
        let env = TauriAppImageAdapter::build_env(vec![
            ("PATH".into(), "/bin".into()),
            ("NO_STRIP".into(), "false".into()),
        ]);
        assert_eq!(
            env,
            [
                ("PATH".to_owned(), "/bin".to_owned()),
                ("NO_STRIP".to_owned(), "true".to_owned())
            ]
        );
    }

    #[test]
    fn no_bundle_dir_or_no_file_is_no_artifact() {
        let d = tempfile::tempdir().unwrap();
        let e = locate_artifact(d.path(), &[], SystemTime::now()).unwrap_err();
        assert_eq!(
            (e.kind, e.message.as_str()),
            (FailureKind::NoArtifact, "No AppImage was produced")
        );
        bundle(d.path());
        let e = locate_artifact(d.path(), &[], SystemTime::now()).unwrap_err();
        assert_eq!(e.kind, FailureKind::NoArtifact);
    }

    #[test]
    fn a_stale_file_never_counts() {
        let d = tempfile::tempdir().unwrap();
        touch(
            &bundle(d.path()).join("old.AppImage"),
            Duration::from_secs(3600),
        );
        let e = locate_artifact(d.path(), &[], SystemTime::now()).unwrap_err();
        assert_eq!(e.kind, FailureKind::NoArtifact);
    }

    #[test]
    fn two_new_files_are_refused_and_a_stale_one_beside_a_new_one_is_ignored() {
        let d = tempfile::tempdir().unwrap();
        let b = bundle(d.path());
        touch(&b.join("a.AppImage"), Duration::ZERO);
        touch(&b.join("b.AppImage"), Duration::ZERO);
        let e = locate_artifact(d.path(), &[], SystemTime::now()).unwrap_err();
        assert_eq!(e.kind, FailureKind::NoArtifact);
        assert!(e.message.contains("found 2"));

        fs::remove_file(b.join("b.AppImage")).unwrap();
        touch(&b.join("stale.AppImage"), Duration::from_secs(3600));
        touch(&b.join("notes.txt"), Duration::ZERO);
        let got = locate_artifact(d.path(), &[], SystemTime::now()).unwrap();
        assert_eq!(got, b.join("a.AppImage"));
    }

    #[test]
    fn export_bundles_are_located_per_target_and_stale_files_are_ignored() {
        let d = tempfile::tempdir().unwrap();
        let t = d.path().join("src-tauri/target");
        for (spec, name) in [
            (&LINUX_BUNDLES[0], "app_1.0.0_amd64.AppImage"),
            (&LINUX_BUNDLES[1], "app_1.0.0_amd64.deb"),
            (&LINUX_BUNDLES[2], "app-1.0.0-1.x86_64.rpm"),
            (&WINDOWS_BUNDLES[0], "app_1.0.0_x64-setup.exe"),
        ] {
            let dir = t.join(spec.dir);
            fs::create_dir_all(&dir).unwrap();
            touch(&dir.join(name), Duration::ZERO);
            touch(
                &dir.join(format!("old{}", spec.suffix)),
                Duration::from_secs(3600),
            );
            // Not this bundle's kind.
            touch(&dir.join("notes.txt"), Duration::ZERO);
            let got = locate_bundle(d.path(), &[], SystemTime::now(), spec).unwrap();
            assert_eq!(got, dir.join(name));
        }
        // A target with only a stale file is a NoArtifact naming its kind.
        let dir = t.join(WINDOWS_BUNDLES[0].dir);
        fs::remove_file(dir.join("app_1.0.0_x64-setup.exe")).unwrap();
        let e = locate_bundle(d.path(), &[], SystemTime::now(), &WINDOWS_BUNDLES[0]).unwrap_err();
        assert_eq!(
            (e.kind, e.message.as_str()),
            (FailureKind::NoArtifact, "No Windows installer was produced")
        );
    }

    #[test]
    fn cargo_target_dir_from_the_build_env_is_honoured() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("elsewhere");
        let b = target.join("release/bundle/appimage");
        fs::create_dir_all(&b).unwrap();
        touch(&b.join("x.AppImage"), Duration::ZERO);
        // The default location is empty, so only the override can find it.
        let env = vec![("CARGO_TARGET_DIR".to_owned(), target.display().to_string())];
        assert_eq!(
            locate_artifact(d.path(), &env, SystemTime::now()).unwrap(),
            b.join("x.AppImage")
        );
        assert!(locate_artifact(d.path(), &[], SystemTime::now()).is_err());
        // Relative values resolve against the project folder.
        let env = vec![("CARGO_TARGET_DIR".to_owned(), "elsewhere".to_owned())];
        assert!(locate_artifact(d.path(), &env, SystemTime::now()).is_ok());
    }
}
