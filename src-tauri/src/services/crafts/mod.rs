//! ArtCraft Crafting Apps: which ones are installed and how to start them (eng review D5, D7).
//! One catalog drives detection, Doctor and the agent; an app is "wired" when its headless
//! `<id>-cli mcp` launch was verified, and confined when it accepts `--automation-write-root`.
use crate::domain::{CraftsEntry, ProjectMeta};
use crate::platform::appimage::host_env;
use crate::services::agents::{McpServerSpec, RunRequest};
use crate::services::diagnostics::{
    spawn::{run_capped, run_capped_in},
    tools::find_in_path,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex, PoisonError},
    time::{Duration, UNIX_EPOCH},
};

/// How a verified app's MCP server starts: arguments after `<id>-cli`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct McpLaunch {
    pub args: &'static [&'static str],
    /// Accepts `--automation-write-root`, so its writes can be confined (D13).
    pub write_root: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CraftSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub purpose: &'static str,
    /// `None`: in the lineup but not verified, shown as "not wired".
    pub mcp: Option<McpLaunch>,
}

const fn wired(args: &'static [&'static str], write_root: bool) -> Option<McpLaunch> {
    Some(McpLaunch { args, write_root })
}

/// The getartcraft.com/apps lineup. Launch arguments were checked against each `--help`
/// on 2026-11-07; vectorcraft connects to a running GUI unless told `--headless`.
pub(crate) const CATALOG: &[CraftSpec] = &[
    CraftSpec {
        id: "photocraft",
        name: "PhotoCraft",
        purpose: "Images and sprites",
        mcp: wired(&["mcp"], true),
    },
    CraftSpec {
        id: "vectorcraft",
        name: "VectorCraft",
        purpose: "Icons and UI art",
        mcp: wired(&["mcp", "--headless"], true),
    },
    CraftSpec {
        id: "soundcraft",
        name: "SoundCraft",
        purpose: "Sound effects and music",
        mcp: wired(&["mcp"], true),
    },
    CraftSpec {
        id: "deckcraft",
        name: "DeckCraft",
        purpose: "Presentations",
        mcp: wired(&["mcp"], true),
    },
    CraftSpec {
        id: "cadcraft",
        name: "CADCraft",
        purpose: "Technical drawings",
        mcp: wired(&["mcp"], false),
    },
    CraftSpec {
        id: "gridcraft",
        name: "GridCraft",
        purpose: "Spreadsheets",
        mcp: wired(&["mcp"], false),
    },
    CraftSpec {
        id: "texturecraft",
        name: "TextureCraft",
        purpose: "3D texture painting",
        mcp: None,
    },
    CraftSpec {
        id: "materialcraft",
        name: "MaterialCraft",
        purpose: "Procedural materials",
        mcp: None,
    },
    CraftSpec {
        id: "filmcraft",
        name: "FilmCraft",
        purpose: "Video editing",
        mcp: None,
    },
    CraftSpec {
        id: "effectcraft",
        name: "EffectCraft",
        purpose: "Motion graphics",
        mcp: None,
    },
    CraftSpec {
        id: "figcraft",
        name: "FigCraft",
        purpose: "Interface design",
        mcp: None,
    },
    CraftSpec {
        id: "designcraft",
        name: "DesignCraft",
        purpose: "Page layout",
        mcp: None,
    },
    CraftSpec {
        id: "lightcraft",
        name: "LightCraft",
        purpose: "Photo library and RAW",
        mcp: None,
    },
    CraftSpec {
        id: "pdfcraft",
        name: "PdfCraft",
        purpose: "PDF editing",
        mcp: None,
    },
    CraftSpec {
        id: "wordcraft",
        name: "WordCraft",
        purpose: "Word processing",
        mcp: None,
    },
    // ponytail: id guessed from the app name; correct it once a Craft Anything install is seen.
    CraftSpec {
        id: "craftanything",
        name: "Craft Anything",
        purpose: "Custom software variants",
        mcp: None,
    },
];

/// Declaration order is the launch precedence (D7): package, then Flatpak, then AppImage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum InstallKind {
    Package,
    Flatpak,
    AppImage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Install {
    pub kind: InstallKind,
    pub version: String,
    /// `/usr/bin/<id>-cli`, the Flatpak app id, or the AppImage file.
    pub location: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DetectedApp {
    pub spec: CraftSpec,
    /// Every install found; the first is the one Studio launches.
    pub installs: Vec<Install>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Detection {
    /// Catalog order.
    pub apps: Vec<DetectedApp>,
    /// `ai.storyteller.*` Flatpaks that are not in the catalog (app id, version).
    pub unknown: Vec<(String, String)>,
    /// Problems worth one Doctor line (flatpak timed out, ...).
    pub warnings: Vec<String>,
}

const FLATPAK_PREFIX: &str = "ai.storyteller.";
const LAUNCHER_ID: &str = "ai.storyteller.artcraft-launcher";
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// `flatpak list --app --columns=application,version` rows for ArtCraft apps (id, version).
fn parse_flatpak_list(stdout: &str) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = Vec::new();
    for line in stdout.lines() {
        let (app, version) = line.split_once('\t').unwrap_or((line, ""));
        let app = app.trim();
        if app == LAUNCHER_ID {
            continue;
        }
        let Some(id) = app.strip_prefix(FLATPAK_PREFIX) else {
            continue;
        };
        // The same app can be installed for the user and system-wide.
        let row = (id.to_owned(), version.trim().to_owned());
        if !rows.contains(&row) {
            rows.push(row);
        }
    }
    rows
}

/// `<id>-<ver>-linux-<arch>[(N)][_<32 hex>].AppImage` for `arch` (the host's); the `(N)` comes
/// from browsers, the hash from AppImageLauncher.
fn parse_appimage_name(file: &str, arch: &str) -> Option<(&'static CraftSpec, String)> {
    let mut stem = file.strip_suffix(".AppImage")?;
    if let Some((s, hash)) = stem.rsplit_once('_')
        && hash.len() == 32
        && hash.bytes().all(|b| b.is_ascii_hexdigit())
    {
        stem = s;
    }
    if let Some((s, n)) = stem.strip_suffix(')').and_then(|s| s.rsplit_once('('))
        && !n.is_empty()
        && n.bytes().all(|b| b.is_ascii_digit())
    {
        stem = s;
    }
    let rest = stem.strip_suffix(arch)?.strip_suffix("-linux-")?;
    let (spec, version) = CATALOG
        .iter()
        .find_map(|s| Some((s, rest.strip_prefix(s.id)?.strip_prefix('-')?)))?;
    (!version.is_empty()).then(|| (spec, version.to_owned()))
}

/// Numeric version order; a `-dev...` suffix sorts as its base version.
fn version_key(v: &str) -> Vec<u64> {
    let base = v.split('-').next().unwrap_or_default();
    base.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// Groups installs per catalog app, sorted by precedence, then highest version first.
fn assemble(installs: Vec<(&'static CraftSpec, Install)>) -> Vec<DetectedApp> {
    let mut apps: Vec<DetectedApp> = Vec::new();
    for (spec, install) in installs {
        match apps.iter_mut().find(|a| a.spec.id == spec.id) {
            Some(app) => app.installs.push(install),
            None => apps.push(DetectedApp {
                spec: *spec,
                installs: vec![install],
            }),
        }
    }
    for app in &mut apps {
        app.installs.sort_by(|a, b| {
            a.kind
                .cmp(&b.kind)
                .then_with(|| version_key(&b.version).cmp(&version_key(&a.version)))
        });
    }
    apps.sort_by_key(|a| CATALOG.iter().position(|s| s.id == a.spec.id));
    apps
}

/// Finds every ArtCraft install. `env` and `path` are Studio's spawn environment and PATH;
/// a missing `flatpak` binary is not an error.
pub(crate) async fn detect(env: &[(String, String)], path: &str, home: Option<&Path>) -> Detection {
    detect_in(
        env,
        path,
        home,
        Path::new("/usr/bin"),
        std::env::consts::ARCH,
    )
    .await
}

async fn detect_in(
    env: &[(String, String)],
    path: &str,
    home: Option<&Path>,
    usr_bin: &Path,
    arch: &str,
) -> Detection {
    let mut found = Vec::new();
    let mut d = Detection::default();

    let usr_bin = usr_bin.to_string_lossy();
    for spec in CATALOG {
        let Some(cli) = find_in_path(&format!("{}-cli", spec.id), &usr_bin) else {
            continue;
        };
        let version = match run_capped(&cli, &["--version"], env, PROBE_TIMEOUT).await {
            Ok(out) if out.status.success() => out
                .stdout
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().last())
                .unwrap_or_default()
                .to_owned(),
            _ => String::new(),
        };
        let location = cli.display().to_string();
        found.push((
            spec,
            Install {
                kind: InstallKind::Package,
                version,
                location,
            },
        ));
    }

    if let Some(flatpak) = find_in_path("flatpak", path) {
        let args = ["list", "--app", "--columns=application,version"];
        match run_capped(&flatpak, &args, env, PROBE_TIMEOUT).await {
            Ok(out) if out.status.success() => {
                for (id, version) in parse_flatpak_list(&out.stdout) {
                    let location = format!("{FLATPAK_PREFIX}{id}");
                    match CATALOG.iter().find(|s| s.id == id) {
                        Some(spec) => found.push((
                            spec,
                            Install {
                                kind: InstallKind::Flatpak,
                                version,
                                location,
                            },
                        )),
                        None => d.unknown.push((location, version)),
                    }
                }
            }
            Ok(out) => d.warnings.push(format!(
                "flatpak list failed: {}",
                out.stderr.lines().next().unwrap_or_default().trim()
            )),
            Err(e) => d.warnings.push(format!("flatpak list: {e}")),
        }
    }

    // No home folder: nothing to scan, not an error.
    if let Some(Ok(entries)) = home.map(|h| std::fs::read_dir(h.join("Applications"))) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some((spec, version)) = name.to_str().and_then(|n| parse_appimage_name(n, arch))
            else {
                continue;
            };
            let location = entry.path().display().to_string();
            found.push((
                spec,
                Install {
                    kind: InstallKind::AppImage,
                    version,
                    location,
                },
            ));
        }
    }

    d.apps = assemble(found);
    d
}

/// Pre-ticked when a project turns ArtCraft on (eng D9): the game-asset apps.
pub(crate) const DEFAULT_APPS: &[&str] = &["photocraft", "vectorcraft", "soundcraft"];

/// One checkbox row of the ArtCraft popover (design D21, D22).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CraftAppRow {
    pub id: String,
    pub name: String,
    pub purpose: String,
    /// The install Studio launches; `None` for a ticked app that is no longer installed.
    pub kind: Option<InstallKind>,
    pub version: String,
    /// No write root, so every action shows an approval card (eng D13).
    pub asks_each_action: bool,
    pub ticked: bool,
    /// Ticked in project.json but not installed anymore ("Not found").
    pub missing: bool,
}

/// A catalog app that is not installed ("Get it" row).
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CatalogApp {
    pub id: String,
    pub name: String,
    pub purpose: String,
}

/// Everything the ArtCraft popover shows for one project.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CraftAppsView {
    /// The project has a `crafts` entry.
    pub enabled: bool,
    /// Usable apps plus ticked-but-missing ones, catalog order.
    pub apps: Vec<CraftAppRow>,
    pub not_installed: Vec<CatalogApp>,
    /// Installed but not usable yet: catalog names, then unknown Flatpak ids.
    pub not_wired: Vec<String>,
    /// What switching on ticks.
    pub defaults: Vec<String>,
}

/// The popover's view of `detection` and the project's `crafts` entry.
pub(crate) fn apps_view(detection: &Detection, chosen: Option<&CraftsEntry>) -> CraftAppsView {
    let ticked = |id: &str| chosen.is_some_and(|c| c.apps.iter().any(|a| a == id));
    let mut view = CraftAppsView {
        enabled: chosen.is_some(),
        apps: Vec::new(),
        not_installed: Vec::new(),
        not_wired: Vec::new(),
        defaults: Vec::new(),
    };
    for spec in CATALOG {
        let used = detection
            .apps
            .iter()
            .find(|a| a.spec.id == spec.id)
            .and_then(|a| a.installs.first());
        let row = |kind, version, mcp: McpLaunch, missing| CraftAppRow {
            id: spec.id.into(),
            name: spec.name.into(),
            purpose: spec.purpose.into(),
            kind,
            version,
            asks_each_action: !mcp.write_root,
            ticked: ticked(spec.id),
            missing,
        };
        match (used, spec.mcp) {
            (Some(install), Some(mcp)) => {
                view.apps
                    .push(row(Some(install.kind), install.version.clone(), mcp, false));
                if DEFAULT_APPS.contains(&spec.id) {
                    view.defaults.push(spec.id.into());
                }
            }
            (None, Some(mcp)) if ticked(spec.id) => {
                view.apps.push(row(None, String::new(), mcp, true))
            }
            (Some(_), None) => view.not_wired.push(spec.name.into()),
            (None, _) => view.not_installed.push(CatalogApp {
                id: spec.id.into(),
                name: spec.name.into(),
                purpose: spec.purpose.into(),
            }),
        }
    }
    view.not_wired
        .extend(detection.unknown.iter().map(|(id, _)| id.clone()));
    view
}

/// The entry to store for `apps`: catalog order, no duplicates, `None` when empty (switch off).
/// Only wired apps can be chosen; a ticked app may be missing right now.
pub(crate) fn validate_choice(apps: &[String]) -> Result<Option<CraftsEntry>, String> {
    for id in apps {
        match CATALOG.iter().find(|s| s.id == id) {
            None => return Err(format!("\"{id}\" isn't an ArtCraft app")),
            Some(spec) if spec.mcp.is_none() => {
                return Err(format!("{} can't be used by Claude yet", spec.name));
            }
            Some(_) => {}
        }
    }
    let chosen: Vec<String> = CATALOG
        .iter()
        .filter(|s| apps.iter().any(|a| a == s.id))
        .map(|s| s.id.to_owned())
        .collect();
    Ok((!chosen.is_empty()).then_some(CraftsEntry { apps: chosen }))
}

/// Stores the choice in the project's project.json and returns what was stored.
pub(crate) fn save_choice(
    project_dir: &Path,
    apps: &[String],
) -> Result<Option<CraftsEntry>, String> {
    let entry = validate_choice(apps)?;
    let mut meta = ProjectMeta::read(project_dir)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "This project has no .ai-app-studio/project.json".to_owned())?;
    meta.crafts = entry.clone();
    meta.write(project_dir).map_err(|e| e.to_string())?;
    Ok(entry)
}

/// Folder under Studio's cache dir that holds unpacked AppImage CLIs.
pub(crate) const EXTRACT_DIR: &str = "artcraft-extract";
const TEMP_PREFIX: &str = ".tmp-";
/// Measured: 152 MB for CADCraft 0.18.0 in a few seconds; slow disks get room.
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(120);

/// Unpacked AppImage CLIs (eng D5 B, D12 A): `<root>/<id>-<version>-<size>-<mtime>/usr/bin/<id>-cli`.
/// One unpack per key, even when two projects start runs at once; nothing is deleted while
/// Studio runs ([`prune_extracts`] runs at start-up).
pub(crate) struct ExtractCache {
    root: PathBuf,
    locks: StdMutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl ExtractCache {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
            locks: StdMutex::new(HashMap::new()),
        }
    }

    /// The `<id>-cli` inside the AppImage `install.location`, unpacking it on first use.
    /// The error is one sentence for the run's warnings ("Couldn't unpack CADCraft: ...").
    pub(crate) async fn ensure_extracted(
        &self,
        spec: &CraftSpec,
        install: &Install,
        env: &[(String, String)],
    ) -> Result<PathBuf, String> {
        let fail = |why: String| format!("Couldn't unpack {}: {why}", spec.name);
        let image = Path::new(&install.location);
        let meta =
            std::fs::metadata(image).map_err(|e| fail(format!("{}: {e}", image.display())))?;
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        let key = format!("{}-{}-{}-{mtime}", spec.id, install.version, meta.len());
        let cli = format!("usr/bin/{}-cli", spec.id);
        let done = self.root.join(&key);

        let lock = self
            .locks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(key.clone())
            .or_default()
            .clone();
        let _held = lock.lock().await;
        if done.join(&cli).is_file() {
            return Ok(done.join(cli));
        }

        let tmp = self.root.join(format!("{TEMP_PREFIX}{key}"));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| fail(e.to_string()))?;
        // The glob is an argument for the AppImage runtime, not a shell pattern.
        let mut env = host_env(env);
        env.push(("APPIMAGELAUNCHER_DISABLE".into(), "1".into()));
        let args = ["--appimage-extract", "usr/bin/*"];
        let outcome = match run_capped_in(image, &args, &env, Some(&tmp), EXTRACT_TIMEOUT).await {
            Ok(out) if out.status.success() => {
                let unpacked = tmp.join("squashfs-root");
                if unpacked.join(&cli).is_file() {
                    let _ = std::fs::remove_dir_all(&done);
                    std::fs::rename(&unpacked, &done).map_err(|e| e.to_string())
                } else {
                    Err(format!("{}-cli is not in this AppImage", spec.id))
                }
            }
            Ok(out) => Err(format!(
                "{}: {}",
                out.status,
                out.stderr.lines().next().unwrap_or_default().trim()
            )),
            Err(e) => Err(e.to_string()),
        };
        let _ = std::fs::remove_dir_all(&tmp);
        outcome.map(|()| done.join(cli)).map_err(fail)
    }
}

/// Start-up cleanup: keeps the newest unpacked version of each app and removes the rest,
/// plus temp folders an interrupted unpack left behind.
pub(crate) fn prune_extracts(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut newest: HashMap<&'static str, ((Vec<u64>, u64), PathBuf)> = HashMap::new();
    let mut doomed = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(TEMP_PREFIX) {
            doomed.push(entry.path());
            continue;
        }
        let Some((spec, version, mtime)) = parse_extract_name(&name) else {
            continue;
        };
        let rank = (version_key(&version), mtime);
        match newest.get(spec.id) {
            Some((best, _)) if *best >= rank => doomed.push(entry.path()),
            _ => {
                if let Some((_, older)) = newest.insert(spec.id, (rank, entry.path())) {
                    doomed.push(older);
                }
            }
        }
    }
    for dir in doomed {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// `<id>-<version>-<size>-<mtime>` back into (app, version, mtime).
fn parse_extract_name(name: &str) -> Option<(&'static CraftSpec, String, u64)> {
    let mut parts = name.rsplitn(3, '-');
    let mtime = parts.next()?.parse().ok()?;
    parts.next()?.parse::<u64>().ok()?;
    let rest = parts.next()?;
    CATALOG.iter().find_map(|s| {
        Some((
            s,
            rest.strip_prefix(s.id)?.strip_prefix('-')?.to_owned(),
            mtime,
        ))
    })
}

/// Folder of the project the Craft engines may write into (eng D14); created on demand.
pub(crate) const ASSETS_DIR: &str = "assets";
/// Claude kills nothing itself, but Studio fails a run without init after 30 s and Claude
/// waits for its MCP servers before init: a hung engine must give up first (eng D10).
const MCP_TIMEOUT_MS: &str = "10000";

/// The launch of one ticked app for a run. `program` is the CLI to start: the package path,
/// the unpacked AppImage CLI, or `flatpak`.
fn server_spec(
    spec: &CraftSpec,
    mcp: McpLaunch,
    install: &Install,
    program: &str,
    project: &Path,
    attachments: Option<&Path>,
) -> McpServerSpec {
    let show = |p: &Path| p.display().to_string();
    let assets = project.join(ASSETS_DIR);
    let mut args = Vec::new();
    if install.kind == InstallKind::Flatpak {
        // The sandbox sees only what is shared: the project read-only, assets/ writable.
        args.push("run".to_owned());
        args.push(format!("--command={}-cli", spec.id));
        args.push(format!("--filesystem={}:ro", show(project)));
        args.push(format!("--filesystem={}", show(&assets)));
        if let Some(a) = attachments {
            args.push(format!("--filesystem={}:ro", show(a)));
        }
        args.push(install.location.clone());
    }
    args.extend(mcp.args.iter().map(|a| (*a).to_owned()));
    if mcp.write_root {
        for root in std::iter::once(project).chain(attachments) {
            args.push("--automation-read-root".to_owned());
            args.push(show(root));
        }
        args.push("--automation-write-root".to_owned());
        args.push(show(&assets));
    }
    McpServerSpec {
        name: format!("craft_{}", spec.id),
        command: program.to_owned(),
        args,
    }
}

/// What a run gets from the project's ArtCraft choice: servers for the ticked apps that can
/// start, and one warning per ticked app that can't (eng D6).
#[derive(Debug, Default, PartialEq)]
pub(crate) struct ForRun {
    pub servers: Vec<McpServerSpec>,
    pub warnings: Vec<(String, Option<String>)>,
}

impl ForRun {
    /// Adds the servers and warnings to `req`, plus `MCP_TIMEOUT` when there is a server.
    pub(crate) fn apply(self, req: &mut RunRequest) {
        if !self.servers.is_empty() {
            req.spawn_env
                .push(("MCP_TIMEOUT".to_owned(), MCP_TIMEOUT_MS.to_owned()));
        }
        req.mcp_servers.extend(self.servers);
        req.pre_warnings.extend(self.warnings);
    }
}

async fn plan_run(
    chosen: &CraftsEntry,
    detection: &Detection,
    cache: &ExtractCache,
    project: &Path,
    attachments: Option<&Path>,
    env: &[(String, String)],
) -> ForRun {
    let mut run = ForRun::default();
    for id in &chosen.apps {
        // Hand-edited ids that aren't wired are skipped; the popover never stores them.
        let Some((spec, mcp)) = CATALOG
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| Some((s, s.mcp?)))
        else {
            continue;
        };
        let Some(install) = detection
            .apps
            .iter()
            .find(|a| a.spec.id == spec.id)
            .and_then(|a| a.installs.first())
        else {
            run.warnings.push((
                format!(
                    "{} isn't installed anymore. Reinstall it or untick it in ArtCraft.",
                    spec.name
                ),
                None,
            ));
            continue;
        };
        let program = match install.kind {
            InstallKind::Package => install.location.clone(),
            InstallKind::Flatpak => "flatpak".to_owned(),
            InstallKind::AppImage => match cache.ensure_extracted(spec, install, env).await {
                Ok(cli) => cli.display().to_string(),
                Err(e) => {
                    run.warnings.push((
                        format!(
                            "{} didn't start, so Claude can't use it in this run.",
                            spec.name
                        ),
                        Some(e),
                    ));
                    continue;
                }
            },
        };
        run.servers.push(server_spec(
            spec,
            mcp,
            install,
            &program,
            project,
            attachments,
        ));
    }
    if !run.servers.is_empty()
        && let Err(e) = std::fs::create_dir_all(project.join(ASSETS_DIR))
    {
        run.warnings.push((
            "Couldn't create the assets folder".to_owned(),
            Some(e.to_string()),
        ));
    }
    run
}

/// The catalog app behind a `craft_<id>` server name.
pub(crate) fn server_app(name: &str) -> Option<&'static CraftSpec> {
    let id = name.strip_prefix("craft_")?;
    CATALOG.iter().find(|c| c.id == id)
}

/// Rules for the agent when the run has ArtCraft servers (eng D14): which apps, and that
/// what they make goes in `assets/`. `None` without Craft servers.
pub(crate) fn agent_rules(servers: &[McpServerSpec]) -> Option<String> {
    let apps: Vec<String> = servers
        .iter()
        .filter_map(|s| server_app(&s.name))
        .map(|c| {
            let mut purpose = c.purpose.chars();
            let first = purpose.next().map(|f| f.to_lowercase().collect::<String>());
            format!(
                "{} ({}{})",
                c.name,
                first.unwrap_or_default(),
                purpose.as_str()
            )
        })
        .collect();
    if apps.is_empty() {
        return None;
    }
    Some(format!(
        "This project can use ArtCraft apps through their MCP tools: {}.\n\
         - Use them to make the assets the app needs (images, icons, sounds, drawings) instead of placeholder files.\n\
         - They can read the project and the attached files, and may save only into the project's `{ASSETS_DIR}/` folder. Save what they make there and refer to it from the code by that path.\n\
         - If an ArtCraft tool fails or is missing, tell the user instead of faking the asset.",
        apps.join(", ")
    ))
}

/// Studio-wide ArtCraft state for runs: the AppImage extract cache and the home folder.
pub(crate) struct Crafts {
    cache: ExtractCache,
    home: Option<PathBuf>,
}

impl Crafts {
    pub(crate) fn new(cache_root: PathBuf, home: Option<PathBuf>) -> Self {
        Self {
            cache: ExtractCache::new(cache_root),
            home,
        }
    }

    /// Nothing for a project without a `crafts` entry, so its runs stay as before (eng D3).
    pub(crate) async fn for_run(
        &self,
        project: &Path,
        attachments: Option<&Path>,
        env: &[(String, String)],
    ) -> ForRun {
        let Some(chosen) = ProjectMeta::read(project)
            .ok()
            .flatten()
            .and_then(|m| m.crafts)
        else {
            return ForRun::default();
        };
        let path = env
            .iter()
            .find(|(k, _)| k == "PATH")
            .map_or("", |(_, v)| v.as_str());
        let detection = detect(env, path, self.home.as_deref()).await;
        plan_run(&chosen, &detection, &self.cache, project, attachments, env).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake AppImage: logs its arguments to `log` on every unpack, then builds
    /// `squashfs-root/usr/bin/<cli>` in its working directory (unless `cli` is empty)
    /// and exits with `code`.
    fn fake_appimage(path: &Path, log: &Path, cli: &str, code: i32) {
        use std::os::unix::fs::PermissionsExt;
        let make_cli = if cli.is_empty() {
            "mkdir -p squashfs-root/usr/bin".to_owned()
        } else {
            format!(
                "mkdir -p squashfs-root/usr/bin && printf '#!/bin/sh\\n' > squashfs-root/usr/bin/{cli} \
                 && chmod +x squashfs-root/usr/bin/{cli}"
            )
        };
        let body = format!(
            "#!/bin/sh\necho \"$@ $APPIMAGELAUNCHER_DISABLE\" >> '{}'\nsleep 0.2\n{make_cli}\nexit {code}\n",
            log.display()
        );
        std::fs::write(path, body).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn appimage_install(path: &Path) -> Install {
        Install {
            kind: InstallKind::AppImage,
            version: "0.18.0".into(),
            location: path.display().to_string(),
        }
    }

    fn unpack_env() -> Vec<(String, String)> {
        vec![("PATH".to_owned(), "/usr/bin:/bin".to_owned())]
    }

    fn lines(log: &Path) -> Vec<String> {
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|r| {
                r.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    fn detected(id: &str, kind: InstallKind, version: &str) -> DetectedApp {
        DetectedApp {
            spec: *spec(id),
            installs: vec![install(kind, version)],
        }
    }

    fn entry(apps: &[&str]) -> CraftsEntry {
        CraftsEntry {
            apps: apps.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    #[test]
    fn view_lists_usable_and_missing_apps_and_sorts_the_rest() {
        let detection = Detection {
            apps: vec![
                detected("photocraft", InstallKind::Flatpak, "0.21.0"),
                detected("cadcraft", InstallKind::AppImage, "0.18.0"),
                detected("wordcraft", InstallKind::Flatpak, "0.16.0"),
            ],
            unknown: vec![("ai.storyteller.mystery".into(), "1.0".into())],
            warnings: vec![],
        };
        let v = apps_view(&detection, Some(&entry(&["photocraft", "soundcraft"])));

        assert!(v.enabled);
        let rows: Vec<_> = v
            .apps
            .iter()
            .map(|r| {
                (
                    r.id.as_str(),
                    r.ticked,
                    r.missing,
                    r.asks_each_action,
                    r.kind,
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("photocraft", true, false, false, Some(InstallKind::Flatpak)),
                ("soundcraft", true, true, false, None),
                ("cadcraft", false, false, true, Some(InstallKind::AppImage)),
            ]
        );
        assert_eq!(v.apps[0].version, "0.21.0");
        assert_eq!(v.apps[0].name, "PhotoCraft");
        assert_eq!(v.apps[0].purpose, "Images and sprites");
        assert_eq!(v.not_wired, ["WordCraft", "ai.storyteller.mystery"]);
        let missing: Vec<_> = v.not_installed.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(
            missing,
            [
                "vectorcraft",
                "deckcraft",
                "gridcraft",
                "texturecraft",
                "materialcraft",
                "filmcraft",
                "effectcraft",
                "figcraft",
                "designcraft",
                "lightcraft",
                "pdfcraft",
                "craftanything",
            ]
        );
        assert_eq!(v.defaults, ["photocraft"], "only detected, wired defaults");
    }

    #[test]
    fn view_without_an_entry_is_off_and_ticks_nothing() {
        let detection = Detection {
            apps: vec![
                detected("soundcraft", InstallKind::Flatpak, "0.18.0"),
                detected("vectorcraft", InstallKind::Package, "0.22.0"),
            ],
            ..Detection::default()
        };
        let v = apps_view(&detection, None);
        assert!(!v.enabled);
        assert!(v.apps.iter().all(|r| !r.ticked && !r.missing));
        assert_eq!(v.defaults, ["vectorcraft", "soundcraft"], "catalog order");
    }

    #[test]
    fn choices_are_deduplicated_in_catalog_order_and_empty_means_off() {
        let s = |v: &[&str]| v.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            validate_choice(&s(&["vectorcraft", "photocraft", "vectorcraft"])),
            Ok(Some(entry(&["photocraft", "vectorcraft"])))
        );
        assert_eq!(validate_choice(&[]), Ok(None));
        let not_wired = validate_choice(&s(&["photocraft", "wordcraft"])).unwrap_err();
        assert!(not_wired.contains("WordCraft"), "{not_wired}");
        assert!(validate_choice(&s(&["nope"])).unwrap_err().contains("nope"));
    }

    #[test]
    fn saving_writes_project_json_and_off_removes_the_entry() {
        use crate::domain::TemplateId;
        let dir = tempfile::tempdir().unwrap();
        ProjectMeta::new("Game", "game", TemplateId::TauriVue, "com.x.game")
            .write(dir.path())
            .unwrap();
        let s = |v: &[&str]| v.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();

        assert_eq!(
            save_choice(dir.path(), &s(&["soundcraft", "photocraft"])),
            Ok(Some(entry(&["photocraft", "soundcraft"])))
        );
        let meta = ProjectMeta::read(dir.path()).unwrap().unwrap();
        assert_eq!(meta.crafts, Some(entry(&["photocraft", "soundcraft"])));
        assert_eq!(meta.name, "Game", "other fields kept");

        assert_eq!(save_choice(dir.path(), &[]), Ok(None));
        assert_eq!(ProjectMeta::read(dir.path()).unwrap().unwrap().crafts, None);

        assert!(save_choice(dir.path(), &s(&["wordcraft"])).is_err());
        assert_eq!(
            ProjectMeta::read(dir.path()).unwrap().unwrap().crafts,
            None,
            "refused choice writes nothing"
        );

        let empty = tempfile::tempdir().unwrap();
        assert!(
            save_choice(empty.path(), &s(&["photocraft"])).is_err(),
            "no project.json"
        );
    }

    #[tokio::test]
    async fn unpacks_only_usr_bin_once_and_returns_the_cli() {
        let dir = tempfile::tempdir().unwrap();
        let (image, log) = (dir.path().join("cadcraft.AppImage"), dir.path().join("log"));
        fake_appimage(&image, &log, "cadcraft-cli", 0);
        let cache = ExtractCache::new(dir.path().join("cache"));
        let install = appimage_install(&image);

        let first = cache
            .ensure_extracted(spec("cadcraft"), &install, &unpack_env())
            .await
            .unwrap();
        let again = cache
            .ensure_extracted(spec("cadcraft"), &install, &unpack_env())
            .await
            .unwrap();

        assert_eq!(first, again);
        assert!(
            first.ends_with("usr/bin/cadcraft-cli"),
            "{}",
            first.display()
        );
        assert!(first.starts_with(dir.path().join("cache")));
        assert!(first.is_file());
        assert_eq!(
            lines(&log),
            ["--appimage-extract usr/bin/* 1"],
            "one unpack, glob passed literally, AppImageLauncher kept out"
        );
        let cached = entries(&dir.path().join("cache"));
        assert_eq!(cached.len(), 1, "no temp folder left: {cached:?}");
        assert!(cached[0].starts_with("cadcraft-0.18.0-"), "{cached:?}");
    }

    #[tokio::test]
    async fn two_runs_at_once_unpack_once() {
        let dir = tempfile::tempdir().unwrap();
        let (image, log) = (dir.path().join("cadcraft.AppImage"), dir.path().join("log"));
        fake_appimage(&image, &log, "cadcraft-cli", 0);
        let cache = ExtractCache::new(dir.path().join("cache"));
        let install = appimage_install(&image);
        let env = unpack_env();

        let (a, b) = tokio::join!(
            cache.ensure_extracted(spec("cadcraft"), &install, &env),
            cache.ensure_extracted(spec("cadcraft"), &install, &env),
        );
        assert_eq!(a.unwrap(), b.unwrap());
        assert_eq!(lines(&log).len(), 1);
    }

    #[tokio::test]
    async fn a_changed_appimage_is_unpacked_again() {
        let dir = tempfile::tempdir().unwrap();
        let (image, log) = (dir.path().join("cadcraft.AppImage"), dir.path().join("log"));
        fake_appimage(&image, &log, "cadcraft-cli", 0);
        let cache = ExtractCache::new(dir.path().join("cache"));
        let install = appimage_install(&image);
        let first = cache
            .ensure_extracted(spec("cadcraft"), &install, &unpack_env())
            .await
            .unwrap();

        let grown = std::fs::read_to_string(&image).unwrap() + "# rebuilt\n";
        std::fs::write(&image, grown).unwrap();
        let second = cache
            .ensure_extracted(spec("cadcraft"), &install, &unpack_env())
            .await
            .unwrap();

        assert_ne!(first, second);
        assert_eq!(lines(&log).len(), 2);
        assert!(
            first.is_file(),
            "the old unpack stays until start-up cleanup"
        );
    }

    #[tokio::test]
    async fn a_failed_unpack_reports_the_app_and_caches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (image, log) = (dir.path().join("cadcraft.AppImage"), dir.path().join("log"));
        fake_appimage(&image, &log, "cadcraft-cli", 3);
        let cache = ExtractCache::new(dir.path().join("cache"));

        let err = cache
            .ensure_extracted(spec("cadcraft"), &appimage_install(&image), &unpack_env())
            .await
            .unwrap_err();
        assert!(err.starts_with("Couldn't unpack CADCraft"), "{err}");
        assert_eq!(entries(&dir.path().join("cache")), Vec::<String>::new());
    }

    #[tokio::test]
    async fn an_image_without_the_cli_is_an_error_and_caches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (image, log) = (dir.path().join("cadcraft.AppImage"), dir.path().join("log"));
        fake_appimage(&image, &log, "", 0);
        let cache = ExtractCache::new(dir.path().join("cache"));

        let err = cache
            .ensure_extracted(spec("cadcraft"), &appimage_install(&image), &unpack_env())
            .await
            .unwrap_err();
        assert!(
            err.starts_with("Couldn't unpack CADCraft") && err.contains("cadcraft-cli"),
            "{err}"
        );
        assert_eq!(entries(&dir.path().join("cache")), Vec::<String>::new());
    }

    #[tokio::test]
    async fn a_missing_appimage_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let cache = ExtractCache::new(dir.path().join("cache"));
        let err = cache
            .ensure_extracted(
                spec("cadcraft"),
                &appimage_install(&dir.path().join("gone.AppImage")),
                &unpack_env(),
            )
            .await
            .unwrap_err();
        assert!(err.starts_with("Couldn't unpack CADCraft"), "{err}");
    }

    #[test]
    fn startup_prune_keeps_the_newest_version_per_app_and_drops_temps() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for d in [
            "cadcraft-0.7.0-100-1",
            "cadcraft-0.18.0-200-2",
            "cadcraft-0.18.0-201-3",
            "photocraft-0.4.1-dev.20261101-300-4",
            ".tmp-cadcraft-0.18.0-202-5",
            "something-else",
        ] {
            std::fs::create_dir_all(root.join(d).join("usr/bin")).unwrap();
        }
        std::fs::write(root.join("notes.txt"), "").unwrap();

        prune_extracts(root);

        assert_eq!(
            entries(root),
            [
                "cadcraft-0.18.0-201-3",
                "notes.txt",
                "photocraft-0.4.1-dev.20261101-300-4",
                "something-else"
            ],
            "same version twice: the later mtime wins; unknown entries are left alone"
        );
    }

    #[test]
    fn startup_prune_tolerates_a_missing_cache() {
        prune_extracts(Path::new("/nonexistent/ai-app-studio-artcraft-extract"));
    }

    fn spec(id: &str) -> &'static CraftSpec {
        CATALOG.iter().find(|s| s.id == id).unwrap()
    }

    fn install(kind: InstallKind, version: &str) -> Install {
        Install {
            kind,
            version: version.into(),
            location: format!("{kind:?}-{version}"),
        }
    }

    #[test]
    fn flatpak_list_keeps_artcraft_apps_and_skips_the_launcher() {
        let out = "org.gnome.Calculator\t46.1\nai.storyteller.photocraft\t0.21.0\n\
                   ai.storyteller.artcraft-launcher\t0.3.2\nai.storyteller.newcraft\t0.1.0\n\
                   ai.storyteller.photocraft\t0.21.0\n";
        assert_eq!(
            parse_flatpak_list(out),
            vec![
                ("photocraft".into(), "0.21.0".into()),
                ("newcraft".into(), "0.1.0".into())
            ]
        );
    }

    #[test]
    fn appimage_names_as_measured_in_applications() {
        let hash = "f2dcc56141cf6a67ca405e9b864b94c8";
        let parse = |f: &str| parse_appimage_name(f, "x86_64").map(|(s, v)| (s.id, v));
        assert_eq!(
            parse("cadcraft-0.18.0-linux-x86_64.AppImage"),
            Some(("cadcraft", "0.18.0".into()))
        );
        assert_eq!(
            parse(&format!("cadcraft-0.7.0-linux-x86_64_{hash}.AppImage")),
            Some(("cadcraft", "0.7.0".into()))
        );
        assert_eq!(
            parse(&format!(
                "photocraft-0.4.1-dev.20261101-linux-x86_64(3)_{hash}.AppImage"
            )),
            Some(("photocraft", "0.4.1-dev.20261101".into()))
        );
        assert_eq!(
            parse("cadcraft-0.18.0-linux-aarch64.AppImage"),
            None,
            "other architecture"
        );
        assert_eq!(
            parse("artcraft-launcher-0.4.1-linux-x86_64.AppImage"),
            None,
            "not a catalog app"
        );
        assert_eq!(parse("Clara-0.1.23.AppImage"), None);
        assert_eq!(parse("cadcraft-0.18.0-linux-x86_64.AppImage.zsync"), None);
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(version_key("0.18.0") > version_key("0.7.0"));
        assert!(version_key("1.0") > version_key("0.99.9"));
        assert_eq!(version_key("0.4.1-dev.20261101"), version_key("0.4.1"));
    }

    #[test]
    fn package_beats_flatpak_beats_appimage_and_newest_appimage_wins() {
        let photo = spec("photocraft");
        let cad = spec("cadcraft");
        let apps = assemble(vec![
            (photo, install(InstallKind::AppImage, "0.30.0")),
            (cad, install(InstallKind::AppImage, "0.7.0")),
            (photo, install(InstallKind::Flatpak, "0.21.0")),
            (cad, install(InstallKind::AppImage, "0.18.0")),
            (photo, install(InstallKind::Package, "0.20.0")),
        ]);
        let ids: Vec<_> = apps.iter().map(|a| a.spec.id).collect();
        assert_eq!(ids, ["photocraft", "cadcraft"], "catalog order");
        let kinds: Vec<_> = apps[0].installs.iter().map(|i| i.kind).collect();
        assert_eq!(
            kinds,
            [
                InstallKind::Package,
                InstallKind::Flatpak,
                InstallKind::AppImage
            ]
        );
        assert_eq!(apps[1].installs[0].version, "0.18.0");
    }

    #[test]
    fn catalog_ids_are_unique_and_lowercase() {
        let mut ids: Vec<_> = CATALOG.iter().map(|s| s.id).collect();
        assert!(ids.iter().all(|i| *i == i.to_lowercase()));
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
    }

    #[tokio::test]
    async fn detect_finds_packages_flatpaks_and_appimages_and_tolerates_no_flatpak() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        let apps = dir.path().join("Applications");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&apps).unwrap();
        let exe = |p: &Path, body: &str| {
            std::fs::write(p, body).unwrap();
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        exe(
            &bin.join("flatpak"),
            "#!/bin/sh\nprintf 'ai.storyteller.soundcraft\\t0.18.0\\nai.storyteller.mystery\\t1.0\\n'\n",
        );
        exe(&apps.join("cadcraft-0.18.0-linux-x86_64.AppImage"), "");
        let home = dir.path();
        let path = bin.display().to_string();
        let env = vec![("PATH".to_owned(), path.clone())];

        let d = detect_in(
            &env,
            &path,
            Some(home),
            &dir.path().join("usr-bin"),
            "x86_64",
        )
        .await;
        let found: Vec<_> = d
            .apps
            .iter()
            .map(|a| (a.spec.id, a.installs[0].kind))
            .collect();
        assert_eq!(
            found,
            [
                ("soundcraft", InstallKind::Flatpak),
                ("cadcraft", InstallKind::AppImage)
            ]
        );
        assert_eq!(
            d.unknown,
            [("ai.storyteller.mystery".to_owned(), "1.0".to_owned())]
        );
        assert!(d.warnings.is_empty(), "{:?}", d.warnings);

        let none = detect_in(&env, "", Some(home), &dir.path().join("usr-bin"), "x86_64").await;
        assert_eq!(none.apps.len(), 1, "no flatpak on PATH: only the AppImage");
        assert!(none.warnings.is_empty());
    }

    #[tokio::test]
    async fn detect_reads_package_cli_versions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let usr_bin = dir.path().join("usr-bin");
        std::fs::create_dir_all(&usr_bin).unwrap();
        let cli = usr_bin.join("photocraft-cli");
        std::fs::write(&cli, "#!/bin/sh\necho 'photocraft-cli 0.21.3'\n").unwrap();
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env = vec![("PATH".to_owned(), "/usr/bin:/bin".to_owned())];

        let d = detect_in(&env, "", Some(dir.path()), &usr_bin, "x86_64").await;
        assert_eq!(
            d.apps[0].installs[0],
            Install {
                kind: InstallKind::Package,
                version: "0.21.3".into(),
                location: cli.display().to_string(),
            }
        );
    }

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|a| (*a).to_owned()).collect()
    }

    #[test]
    fn flatpak_server_is_sandboxed_to_the_project_and_writes_only_assets() {
        let install = Install {
            kind: InstallKind::Flatpak,
            version: "0.21.0".into(),
            location: "ai.storyteller.photocraft".into(),
        };
        let s = spec("photocraft");
        let got = server_spec(
            s,
            s.mcp.unwrap(),
            &install,
            "flatpak",
            Path::new("/p/game"),
            Some(Path::new("/data/att/c1")),
        );
        assert_eq!(
            got,
            McpServerSpec {
                name: "craft_photocraft".into(),
                command: "flatpak".into(),
                args: args(&[
                    "run",
                    "--command=photocraft-cli",
                    "--filesystem=/p/game:ro",
                    "--filesystem=/p/game/assets",
                    "--filesystem=/data/att/c1:ro",
                    "ai.storyteller.photocraft",
                    "mcp",
                    "--automation-read-root",
                    "/p/game",
                    "--automation-read-root",
                    "/data/att/c1",
                    "--automation-write-root",
                    "/p/game/assets",
                ]),
            }
        );
    }

    #[test]
    fn package_server_runs_its_cli_and_vectorcraft_stays_headless() {
        let s = spec("vectorcraft");
        let install = Install {
            kind: InstallKind::Package,
            version: "0.22.0".into(),
            location: "/usr/bin/vectorcraft-cli".into(),
        };
        let got = server_spec(
            s,
            s.mcp.unwrap(),
            &install,
            "/usr/bin/vectorcraft-cli",
            Path::new("/p/game"),
            None,
        );
        assert_eq!(got.name, "craft_vectorcraft");
        assert_eq!(got.command, "/usr/bin/vectorcraft-cli");
        assert_eq!(
            got.args,
            args(&[
                "mcp",
                "--headless",
                "--automation-read-root",
                "/p/game",
                "--automation-write-root",
                "/p/game/assets",
            ])
        );
    }

    #[test]
    fn apps_without_a_write_root_get_no_root_flags() {
        let s = spec("cadcraft");
        let got = server_spec(
            s,
            s.mcp.unwrap(),
            &install(InstallKind::AppImage, "0.18.0"),
            "/cache/cadcraft/usr/bin/cadcraft-cli",
            Path::new("/p/game"),
            Some(Path::new("/data/att/c1")),
        );
        assert_eq!(got.command, "/cache/cadcraft/usr/bin/cadcraft-cli");
        assert_eq!(got.args, args(&["mcp"]));
    }

    #[tokio::test]
    async fn a_run_starts_ticked_apps_and_warns_for_missing_and_broken_ones() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("game");
        std::fs::create_dir(&project).unwrap();
        let (broken, log) = (dir.path().join("cadcraft.AppImage"), dir.path().join("log"));
        fake_appimage(&broken, &log, "cadcraft-cli", 1);
        let detection = Detection {
            apps: vec![
                detected("photocraft", InstallKind::Flatpak, "0.21.0"),
                DetectedApp {
                    spec: *spec("cadcraft"),
                    installs: vec![appimage_install(&broken)],
                },
                detected("vectorcraft", InstallKind::Flatpak, "0.22.0"),
            ],
            ..Detection::default()
        };
        let cache = ExtractCache::new(dir.path().join("cache"));

        let run = plan_run(
            &entry(&["photocraft", "soundcraft", "cadcraft"]),
            &detection,
            &cache,
            &project,
            None,
            &unpack_env(),
        )
        .await;

        let names: Vec<_> = run.servers.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            ["craft_photocraft"],
            "only ticked apps that can start"
        );
        assert_eq!(run.warnings.len(), 2, "{:?}", run.warnings);
        assert_eq!(
            run.warnings[0],
            (
                "SoundCraft isn't installed anymore. Reinstall it or untick it in ArtCraft."
                    .to_owned(),
                None
            )
        );
        assert_eq!(
            run.warnings[1].0,
            "CADCraft didn't start, so Claude can't use it in this run."
        );
        let why = run.warnings[1].1.as_deref().unwrap_or_default();
        assert!(why.starts_with("Couldn't unpack CADCraft"), "{why}");
        assert!(project.join("assets").is_dir(), "write root created");
    }

    #[tokio::test]
    async fn an_appimage_app_runs_its_unpacked_cli() {
        let dir = tempfile::tempdir().unwrap();
        let (image, log) = (dir.path().join("cadcraft.AppImage"), dir.path().join("log"));
        fake_appimage(&image, &log, "cadcraft-cli", 0);
        let detection = Detection {
            apps: vec![DetectedApp {
                spec: *spec("cadcraft"),
                installs: vec![appimage_install(&image)],
            }],
            ..Detection::default()
        };
        let cache = ExtractCache::new(dir.path().join("cache"));

        let run = plan_run(
            &entry(&["cadcraft"]),
            &detection,
            &cache,
            dir.path(),
            None,
            &unpack_env(),
        )
        .await;

        assert_eq!(run.warnings, vec![]);
        let cli = Path::new(&run.servers[0].command);
        assert!(
            cli.starts_with(dir.path().join("cache")),
            "{}",
            cli.display()
        );
        assert!(cli.ends_with("usr/bin/cadcraft-cli"));
    }

    #[tokio::test]
    async fn a_project_without_artcraft_gets_nothing() {
        use crate::domain::TemplateId;
        let dir = tempfile::tempdir().unwrap();
        let crafts = Crafts::new(dir.path().join("cache"), None);
        assert_eq!(
            crafts.for_run(dir.path(), None, &unpack_env()).await,
            ForRun::default(),
            "no project.json"
        );
        ProjectMeta::new("Game", "game", TemplateId::TauriVue, "com.x.game")
            .write(dir.path())
            .unwrap();
        assert_eq!(
            crafts.for_run(dir.path(), None, &unpack_env()).await,
            ForRun::default(),
            "no crafts entry"
        );
        assert!(!dir.path().join("assets").exists());
    }

    #[test]
    fn applying_adds_servers_warnings_and_the_mcp_timeout() {
        let mut req = test_request();
        ForRun {
            servers: vec![McpServerSpec {
                name: "craft_photocraft".into(),
                command: "flatpak".into(),
                args: vec![],
            }],
            warnings: vec![("SoundCraft isn't installed anymore.".into(), None)],
        }
        .apply(&mut req);
        assert_eq!(req.mcp_servers.len(), 1);
        assert_eq!(
            req.pre_warnings.len(),
            2,
            "appended after the checkpoint one"
        );
        assert!(
            req.spawn_env
                .contains(&("MCP_TIMEOUT".into(), "10000".into()))
        );

        let mut req = test_request();
        ForRun {
            servers: vec![],
            warnings: vec![("SoundCraft isn't installed anymore.".into(), None)],
        }
        .apply(&mut req);
        assert!(req.mcp_servers.is_empty());
        assert_eq!(req.pre_warnings.len(), 2);
        assert_eq!(req.spawn_env, unpack_env(), "no servers, env untouched");
    }

    #[test]
    fn agent_rules_name_the_apps_and_the_assets_folder() {
        assert_eq!(agent_rules(&[]), None);
        let server = |name: &str| McpServerSpec {
            name: name.into(),
            command: "flatpak".into(),
            args: vec![],
        };
        let rules = agent_rules(&[server("craft_photocraft"), server("craft_soundcraft")]).unwrap();
        assert!(rules.contains("PhotoCraft (images and sprites)"), "{rules}");
        assert!(
            rules.contains("SoundCraft (sound effects and music)"),
            "{rules}"
        );
        assert!(rules.contains("`assets/`"), "{rules}");
        assert_eq!(agent_rules(&[server("other")]), None, "not a Craft server");
    }

    fn test_request() -> RunRequest {
        RunRequest {
            project_dir: "/p/game".into(),
            prompt: String::new(),
            mode: crate::persistence::SessionMode::Changes,
            resume_session_id: None,
            allowed_tools: vec![],
            attachments_dir: None,
            attachments: vec![],
            auto_approve: None,
            spawn_env: unpack_env(),
            pre_warnings: vec![("No Git checkpoint for this run".into(), None)],
            mcp_servers: vec![],
            log_path: "/tmp/unused.jsonl".into(),
        }
    }
}
