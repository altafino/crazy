//! The export job (F22, D7-D9, D14): claim the Build slot, extract the release's saved source,
//! run the targets in the Docker container, and put each target that worked into
//! `releases/<ver>/exports/<target>/` with `SHA256SUMS`, replacing older files only on success.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use tokio::sync::watch;

use super::docker::{self, Captured, RunSpec};
use super::{
    DockerState, ExportCacheSize, ExportFailure, ExportFile, ExportFileKind, ExportJob,
    ExportReadiness, ExportResult, ExportTarget, ExportTargetView, ReleaseExports,
};
use crate::persistence::{Db, JobRow, JobState, JobUpdate, ProjectRow, ReleaseRow};
use crate::platform::paths::AppLayout;
use crate::services::builds::adapter::{
    self, BundleSpec, LINUX_BUNDLES, TauriAppImageAdapter, WINDOWS_BUNDLES,
};
use crate::services::builds::store;
use crate::services::builds::{
    BuildError, BuildHandle, BuildPhase, BuildService, BuildStatus, Inner, LOG_DIR, Run,
    STOP_GRACE, iso_now, iso_of, lock, version,
};
use crate::services::jobs::JobKind;
use crate::services::jobs::output::{self, LogSink};
use crate::services::jobs::proc::{LineSink, ProcEnd, ProcSpec, run_proc};
use crate::services::preview::phase::{PhaseParser, PreviewPhase};
use crate::services::projects::tauri::{self as project, PackageManager};
use crate::services::settings;

/// Shown when Windows is ticked and the license was never accepted (D10).
const LICENSE_NEEDED: &str =
    "Accept Microsoft's license for the Windows build tools to export for Windows.";
const RELEASE_GONE: &str = "The release was removed while exporting, so the files were discarded.";

fn label(t: ExportTarget) -> &'static str {
    match t {
        ExportTarget::Linux => "Linux",
        ExportTarget::Windows => "Windows",
    }
}

fn io_text(what: &str, e: &std::io::Error) -> String {
    format!("{what}: {e}")
}

// ----------------------------------------------------------------- the files

/// `<release>/exports`.
fn exports_dir(release_dir: &Path) -> PathBuf {
    release_dir.join("exports")
}

fn kind_of(name: &str) -> Option<ExportFileKind> {
    let ext = Path::new(name).extension()?.to_str()?;
    match ext {
        "AppImage" => Some(ExportFileKind::AppImage),
        "deb" => Some(ExportFileKind::Deb),
        "rpm" => Some(ExportFileKind::Rpm),
        "exe" => Some(ExportFileKind::Nsis),
        _ => None,
    }
}

/// Copies the built files into `exports/.tmp-<job>/<target>/`, writes `SHA256SUMS`, and
/// renames that folder over `exports/<target>/`, but only while the release still has files
/// (D9); the old files stay if anything fails (design 15). Blocking.
fn place_export(
    db: &Db,
    release: &ReleaseRow,
    release_dir: &Path,
    job_id: &str,
    target: ExportTarget,
    files: &[PathBuf],
) -> Result<(), String> {
    let kept = || {
        release_dir.is_dir()
            && db
                .get_release(&release.id)
                .ok()
                .flatten()
                .is_some_and(|r| r.files_removed_at.is_none())
    };
    // Checked before anything is created: creating folders would revive a deleted release.
    if !kept() {
        return Err(RELEASE_GONE.into());
    }
    let exports = exports_dir(release_dir);
    let stage_root = exports.join(format!(".tmp-{job_id}"));
    let stage = stage_root.join(target.as_str());
    let staged = (|| {
        fs::create_dir_all(&stage)
            .map_err(|e| io_text("Could not create the export folder", &e))?;
        let mut sums = String::new();
        for src in files {
            let name = src
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or("A built file has no usable name")?;
            let mode = if kind_of(name) == Some(ExportFileKind::AppImage) {
                0o755
            } else {
                0o644
            };
            let put = store::copy_atomic(src, &stage.join(name), mode)
                .map_err(|e| io_text("Could not copy a built file", &e))?;
            sums.push_str(&format!("{}  {name}\n", put.sha256));
        }
        fs::write(stage.join("SHA256SUMS"), sums)
            .map_err(|e| io_text("Could not write SHA256SUMS", &e))
    })();
    let replaced = staged.and_then(|()| {
        if !kept() {
            return Err(RELEASE_GONE.to_owned());
        }
        let final_dir = exports.join(target.as_str());
        let old = exports.join(format!(".old-{job_id}-{}", target.as_str()));
        let had_old = final_dir.exists();
        if had_old {
            fs::rename(&final_dir, &old)
                .map_err(|e| io_text("Could not replace the old export", &e))?;
        }
        if let Err(e) = fs::rename(&stage, &final_dir) {
            if had_old {
                let _ = fs::rename(&old, &final_dir);
            }
            return Err(io_text("Could not store the export", &e));
        }
        if had_old {
            let _ = fs::remove_dir_all(&old);
        }
        Ok(())
    });
    let _ = fs::remove_dir_all(&stage_root);
    replaced
}

/// App startup: removes the temporary folders a killed export left (source folders, image
/// build folders, `.tmp-*` stages) and puts back old files a crash moved aside.
pub(crate) fn remove_leftovers(data: &Path) {
    if let Ok(rd) = fs::read_dir(data.join("tmp")) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with("export-") {
                let p = e.path();
                let _ = fs::remove_dir_all(&p).or_else(|_| fs::remove_file(&p));
            }
        }
    }
    let Ok(apps) = fs::read_dir(data.join("installed")) else {
        return;
    };
    for app in apps.flatten() {
        let Ok(releases) = fs::read_dir(app.path().join("releases")) else {
            continue;
        };
        for rel in releases.flatten() {
            let exports = exports_dir(&rel.path());
            let Ok(entries) = fs::read_dir(&exports) else {
                continue;
            };
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with(".tmp-") {
                    let _ = fs::remove_dir_all(e.path());
                } else if let Some(rest) = name.strip_prefix(".old-") {
                    // `.old-<job>-<target>`: back in place unless a newer export took over.
                    let target = rest.rsplit('-').next().unwrap_or("");
                    let back = exports.join(target);
                    if ExportTarget::parse(target).is_some() && !back.exists() {
                        let _ = fs::rename(e.path(), back);
                    } else {
                        let _ = fs::remove_dir_all(e.path());
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------------ listing

/// Files named in a target folder's `SHA256SUMS` that exist, and when they were made.
fn read_target_files(dir: &Path) -> (Vec<ExportFile>, Option<String>) {
    let sums_path = dir.join("SHA256SUMS");
    let Ok(sums) = fs::read_to_string(&sums_path) else {
        return (Vec::new(), None);
    };
    let files = sums
        .lines()
        .filter_map(|l| l.split_once("  "))
        .filter_map(|(sha, name)| {
            let path = dir.join(name);
            let meta = fs::metadata(&path).ok().filter(fs::Metadata::is_file)?;
            Some(ExportFile {
                name: name.to_owned(),
                kind: kind_of(name)?,
                path: path.display().to_string(),
                size_bytes: meta.len() as f64,
                sha256: sha.to_owned(),
            })
        })
        .collect();
    let built = fs::metadata(&sums_path)
        .and_then(|m| m.modified())
        .ok()
        .map(iso_of);
    (files, built)
}

/// The newest export job that has a result for `target` decides whether it failed (design 6).
fn latest_failure(
    jobs: &[JobRow],
    release_id: &str,
    target: ExportTarget,
) -> Option<ExportFailure> {
    for job in jobs
        .iter()
        .filter(|j| j.export_release_id.as_deref() == Some(release_id))
    {
        let results: Vec<ExportResult> = job
            .export_results
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok())
            .unwrap_or_default();
        if let Some(r) = results.iter().find(|r| r.target == target) {
            return (!r.ok).then(|| ExportFailure {
                at: job.completed_at.clone().unwrap_or_default(),
                message: r.message.clone().unwrap_or_else(|| "Export failed".into()),
            });
        }
    }
    None
}

// ------------------------------------------------------------------ service

/// What `drive_export` needs to know about the request.
struct ExportReq {
    project: ProjectRow,
    release: ReleaseRow,
    targets: Vec<ExportTarget>,
}

/// How the pipeline stopped before it finished every target.
enum Stop {
    Cancelled,
    /// A failure that applies to every target not yet done.
    Failed(String),
}

/// A process's output into the job's log.
struct ExportSink<'a> {
    svc: &'a Inner,
    run: &'a Run,
}

impl LineSink for ExportSink<'_> {
    fn line(&self, raw: &str) {
        self.svc.on_line(self.run, raw);
    }
    fn flush(&self) {
        self.svc.flush(self.run);
    }
}

impl BuildService {
    fn docker_timeouts(&self) -> (Duration, Duration) {
        *lock(&self.inner.docker_timeouts)
    }

    #[cfg(test)]
    pub(crate) fn set_docker_timeouts(&self, probe: Duration, df: Duration) {
        *lock(&self.inner.docker_timeouts) = (probe, df);
    }

    /// Docker state, the export image, the license answer and the remembered targets.
    pub(crate) async fn export_readiness(&self) -> ExportReadiness {
        let d = &self.inner.deps;
        let p = docker::probe(d.toolchain.as_ref(), self.docker_timeouts().0).await;
        ExportReadiness {
            docker: p.state,
            docker_version: p.version,
            detail: p.detail,
            image_present: p.image_present,
            license_accepted_at: settings::export_license_accepted_at(&d.db),
            last_targets: settings::export_last_targets(&d.db),
        }
    }

    /// Starts an export of a kept release and returns at once with the `running` status.
    /// Refusals leave nothing behind (slot freed, no job row).
    pub(crate) async fn start_export(
        &self,
        project: &ProjectRow,
        release_id: &str,
        mut targets: Vec<ExportTarget>,
        accept_license: bool,
    ) -> Result<BuildStatus, BuildError> {
        let inner = &self.inner;
        let d = &inner.deps;
        if !project.template_id.supports_operations() {
            return Err(BuildError::Unsupported);
        }
        targets.sort();
        targets.dedup();
        if targets.is_empty() {
            return Err(BuildError::Export(
                "Choose at least one system to export for.".into(),
            ));
        }
        let job_id = uuid::Uuid::new_v4().to_string();
        if d.db.has_live_job(&project.id, JobKind::Build.as_str())?
            || !d.slots.claim(&project.id, JobKind::Build, &job_id)
        {
            return Err(BuildError::Busy);
        }
        // The slot is ours (like `start`): look for a Make-changes run only now.
        let prepared = self
            .prepare_export(project, release_id, &targets, accept_license, &job_id)
            .await;
        let (release, job, limits, image_present) = match prepared {
            Ok(p) => p,
            Err(e) => {
                d.slots.release(&project.id, JobKind::Build);
                return Err(e);
            }
        };

        let log = output::log_path(&d.paths.data_dir, LOG_DIR, &job_id);
        let status = BuildStatus {
            job_id: job_id.clone(),
            project_id: project.id.clone(),
            version: Some(release.version.clone()),
            state: JobState::Running,
            phase: Some(BuildPhase::Preflight),
            started_at: job.and_then(|j| j.started_at),
            completed_at: None,
            error_kind: None,
            error_message: None,
            release_id: Some(release.id.clone()),
            live: true,
            has_log: false,
            export: Some(ExportJob {
                release_id: release.id.clone(),
                version: release.version.clone(),
                targets: targets.clone(),
                results: Vec::new(),
                first_run: !image_present,
            }),
        };
        let run = Arc::new(Run {
            job_id: job_id.clone(),
            project_id: project.id.clone(),
            status: Mutex::new(status.clone()),
            parser: Mutex::new(PhaseParser::new(PreviewPhase::Starting)),
            sink: Mutex::new(LogSink::new(&log, &limits)),
        });
        lock(&inner.runs).insert(project.id.clone(), run.clone());
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let (finished_tx, finished_rx) = watch::channel(false);
        d.slots.attach(
            &project.id,
            JobKind::Build,
            Arc::new(BuildHandle { cancel: cancel_tx }),
            finished_rx,
        );
        let _ = inner
            .events
            .send(crate::services::builds::BuildEvent::Status(Box::new(
                status.clone(),
            )));
        let req = ExportReq {
            project: project.clone(),
            release,
            targets,
        };
        tokio::spawn(inner.clone().drive_export(run, req, cancel_rx, finished_tx));
        Ok(status)
    }

    /// Everything that can refuse an export, then the job row. Runs with the slot claimed.
    async fn prepare_export(
        &self,
        project: &ProjectRow,
        release_id: &str,
        targets: &[ExportTarget],
        accept_license: bool,
        job_id: &str,
    ) -> Result<
        (
            ReleaseRow,
            Option<JobRow>,
            crate::services::settings::PreviewLimits,
            bool,
        ),
        BuildError,
    > {
        let inner = &self.inner;
        let d = &inner.deps;
        if inner.modifying_run_live(&project.id)? {
            return Err(BuildError::AgentRunning);
        }
        let release =
            d.db.get_release(release_id)?
                .filter(|r| r.project_id == project.id)
                .ok_or(BuildError::NotFound)?;
        if release.files_removed_at.is_some() {
            return Err(BuildError::Export(
                "This release's files were removed, so it can't be exported.".into(),
            ));
        }
        // The folder is named after the version; check again before it becomes a path.
        if version::Version::parse(&release.version).is_none() {
            return Err(BuildError::Io(format!("bad version {:?}", release.version)));
        }
        if release.snapshot_ref.is_none() {
            return Err(BuildError::NoSnapshot);
        }
        let accepted = settings::export_license_accepted_at(&d.db).is_some();
        let windows = targets.contains(&ExportTarget::Windows);
        if windows && !accepted && !accept_license {
            return Err(BuildError::Export(LICENSE_NEEDED.into()));
        }
        let probe = docker::probe(d.toolchain.as_ref(), self.docker_timeouts().0).await;
        if probe.state != DockerState::Ready {
            return Err(BuildError::Export(
                docker::not_ready_message(probe.state).into(),
            ));
        }
        let setting = |e: settings::SettingsError| BuildError::Export(e.to_string());
        if windows && !accepted {
            settings::set_export_license_accepted_at(&d.db, Some(&iso_now())).map_err(setting)?;
        }
        settings::set_export_last_targets(&d.db, targets).map_err(setting)?;
        d.db.insert_job_of_kind(
            job_id,
            &project.id,
            JobKind::Build.as_str(),
            JobState::Running,
        )?;
        let csv = targets
            .iter()
            .map(|t| t.as_str())
            .collect::<Vec<_>>()
            .join(",");
        d.db.set_job_export(job_id, &release.id, &csv)?;
        let ids: Vec<String> =
            d.db.list_jobs_of_kind(&project.id, JobKind::Build.as_str())?
                .into_iter()
                .map(|j| j.id)
                .collect();
        let limits = settings::load_preview_limits(&d.db);
        output::prune_logs(&d.paths.data_dir, LOG_DIR, &ids, limits.keep_logs as usize);
        Ok((release, d.db.get_job(job_id)?, limits, probe.image_present))
    }

    /// The exports of the project's kept releases: files with SHA-256 from `SHA256SUMS`, and
    /// the latest failure per target. Releases with neither are left out. Blocking.
    pub(crate) fn list_release_exports(
        &self,
        project_id: &str,
    ) -> Result<Vec<ReleaseExports>, BuildError> {
        let d = &self.inner.deps;
        let layout = AppLayout::new(&d.paths, project_id);
        let jobs = d.db.list_export_jobs(project_id)?;
        let mut out = Vec::new();
        for release in d.db.list_releases(project_id, false)? {
            let exports = exports_dir(&layout.release_dir(&release.version));
            let targets: Vec<ExportTargetView> = [ExportTarget::Linux, ExportTarget::Windows]
                .into_iter()
                .filter_map(|target| {
                    let dir = exports.join(target.as_str());
                    let (files, built_at) = read_target_files(&dir);
                    let failed = latest_failure(&jobs, &release.id, target);
                    (!files.is_empty() || failed.is_some()).then(|| ExportTargetView {
                        target,
                        built_at: if files.is_empty() { None } else { built_at },
                        dir: dir.display().to_string(),
                        files,
                        failed,
                    })
                })
                .collect();
            if !targets.is_empty() {
                out.push(ReleaseExports {
                    release_id: release.id,
                    dir: exports.display().to_string(),
                    targets,
                });
            }
        }
        Ok(out)
    }

    /// Space used by the export image and caches. No Docker means no caches: 0.
    pub(crate) async fn export_cache_size(&self) -> Result<ExportCacheSize, BuildError> {
        let d = &self.inner.deps;
        let Some(docker) = d.toolchain.resolve_binary("docker") else {
            return Ok(ExportCacheSize { bytes: 0.0 });
        };
        docker::cache_size(&docker, &d.toolchain.spawn_env(), self.docker_timeouts().1)
            .await
            .map(|bytes| ExportCacheSize { bytes })
            .map_err(|m| BuildError::Export(m.into()))
    }

    /// Removes the export caches and image. Refused while any export runs.
    pub(crate) async fn clear_export_caches(&self) -> Result<(), BuildError> {
        let d = &self.inner.deps;
        if d.db.has_live_export(None)? {
            return Err(BuildError::Exporting);
        }
        let Some(docker) = d.toolchain.resolve_binary("docker") else {
            return Ok(());
        };
        docker::clear_caches(&docker, &d.toolchain.spawn_env())
            .await
            .map_err(BuildError::Export)
    }

    /// Forgets the Microsoft license acceptance.
    pub(crate) fn withdraw_export_license(&self) -> Result<(), BuildError> {
        settings::set_export_license_accepted_at(&self.inner.deps.db, None)
            .map_err(|e| BuildError::Export(e.to_string()))
    }
}

/// The reason line for a job that failed for some targets (D14).
fn failure_summary(failed: &[&ExportResult]) -> String {
    let msg = |r: &ExportResult| r.message.clone().unwrap_or_else(|| "Export failed".into());
    match failed {
        [one] => format!("Export failed for {}: {}", label(one.target), msg(one)),
        many => format!(
            "Export failed for {}: {}",
            many.iter()
                .map(|r| label(r.target))
                .collect::<Vec<_>>()
                .join(" and "),
            many.iter()
                .map(|r| format!("{}: {}", label(r.target), msg(r)))
                .collect::<Vec<_>>()
                .join("; ")
        ),
    }
}

impl Inner {
    async fn drive_export(
        self: Arc<Self>,
        run: Arc<Run>,
        req: ExportReq,
        mut cancel: watch::Receiver<bool>,
        finished: watch::Sender<bool>,
    ) {
        let d = &self.deps;
        let mut results: Vec<ExportResult> = Vec::new();
        let end = self
            .export_pipeline(&run, &req, &mut cancel, &mut results)
            .await;
        let cancelled = matches!(end, Err(Stop::Cancelled));
        if let Err(Stop::Failed(message)) = &end {
            for t in &req.targets {
                if !results.iter().any(|r| r.target == *t) {
                    results.push(ExportResult {
                        target: *t,
                        ok: false,
                        message: Some(message.clone()),
                    });
                }
            }
        }
        // Nothing temporary stays: the source folder, the image build folder, the stage.
        let (data, job_id) = (d.paths.data_dir.clone(), run.job_id.clone());
        let release_dir =
            AppLayout::new(&d.paths, &req.project.id).release_dir(&req.release.version);
        let _ = tokio::task::spawn_blocking(move || {
            let tmp = data.join("tmp");
            let _ = fs::remove_dir_all(tmp.join(format!("export-{job_id}")));
            let _ = fs::remove_dir_all(tmp.join(format!("export-image-{job_id}")));
            let _ = fs::remove_file(tmp.join(format!("export-{job_id}.tar")));
            let _ = fs::remove_dir_all(exports_dir(&release_dir).join(format!(".tmp-{job_id}")));
        })
        .await;
        self.flush(&run);

        let failed: Vec<&ExportResult> = results.iter().filter(|r| !r.ok).collect();
        let (state, update) = if d.slots.is_shutting_down() {
            (
                JobState::Interrupted,
                JobUpdate {
                    set_completed: true,
                    ..JobUpdate::default()
                },
            )
        } else if cancelled {
            (
                JobState::Cancelled,
                JobUpdate {
                    set_completed: true,
                    error_kind: Some("cancelled".into()),
                    ..JobUpdate::default()
                },
            )
        } else if failed.is_empty() {
            (
                JobState::Succeeded,
                JobUpdate {
                    set_completed: true,
                    exit_code: Some(0),
                    ..JobUpdate::default()
                },
            )
        } else {
            (
                JobState::Failed,
                JobUpdate {
                    set_completed: true,
                    error_kind: Some("exportFailed".into()),
                    error_message: Some(failure_summary(&failed)),
                    ..JobUpdate::default()
                },
            )
        };
        // The per-target outcome is written once, at the end (D14).
        if !results.is_empty() {
            let json = serde_json::to_string(&results).unwrap_or_else(|_| "[]".into());
            if let Err(e) = d.db.set_job_export_results(&run.job_id, &json) {
                eprintln!("export {}: could not record the results: {e}", run.job_id);
            }
        }
        // Free the slot before the final state is visible, so a finished export never reads
        // as busy.
        d.slots.release(&run.project_id, JobKind::Build);
        if let Err(e) =
            d.db.transition_job(&run.job_id, &[JobState::Running], state, &update)
        {
            eprintln!("export {}: could not record {state:?}: {e}", run.job_id);
        }
        let completed_at =
            d.db.get_job(&run.job_id)
                .ok()
                .flatten()
                .and_then(|j| j.completed_at);
        {
            let mut s = lock(&run.status);
            s.state = state;
            s.live = false;
            s.phase = None;
            s.completed_at = completed_at;
            s.error_kind = update.error_kind.clone();
            s.error_message = update.error_message.clone();
            s.has_log = output::log_path(&d.paths.data_dir, LOG_DIR, &run.job_id).exists();
            if let Some(e) = s.export.as_mut() {
                e.results = results;
            }
        }
        self.emit_status(&run);
        let _ = finished.send(true);
    }

    /// One docker process into the log; Stop also stops the container, because killing the
    /// client does not.
    async fn docker_proc(
        &self,
        run: &Run,
        docker: &Path,
        env: &[(String, String)],
        args: &[String],
        cwd: &Path,
        cancel: &mut watch::Receiver<bool>,
    ) -> ProcEnd {
        let end = run_proc(
            ProcSpec {
                program: docker,
                args,
                env,
                cwd,
            },
            &ExportSink { svc: self, run },
            cancel,
            STOP_GRACE,
        )
        .await;
        if matches!(end, ProcEnd::Cancelled) {
            docker::stop_container(docker, env, &run.job_id).await;
        }
        end
    }

    /// Why a docker process failed: the first error line of its output, else the exit code.
    fn proc_failure(&self, run: &Run, end: &ProcEnd) -> Result<(), String> {
        match end {
            ProcEnd::Exited(st) if st.success() => Ok(()),
            ProcEnd::SpawnFailed(e) => Err(format!("Could not start Docker: {e}")),
            ProcEnd::Exited(st) => {
                let first = {
                    let p = lock(&run.parser);
                    p.first_error().or(p.crash()).map(str::to_owned)
                };
                // Package managers and docker word their errors in ways the parser does not
                // know; the last line of output is the next best reason.
                let last = || lock(&run.sink).tail().lines.last().cloned();
                Err(first.or_else(last).unwrap_or_else(|| match st.code() {
                    Some(c) => format!("The build stopped with exit code {c}."),
                    None => "The build was stopped by a signal.".into(),
                }))
            }
            ProcEnd::Cancelled => Err("Stopped".into()),
        }
    }

    fn fresh_parser(run: &Run) {
        *lock(&run.parser) = PhaseParser::new(PreviewPhase::Starting);
    }

    /// The export, step by step. A target that fails goes into `results` and the next one
    /// still runs; `Err` is Stop or a failure before the targets (source, image, install).
    async fn export_pipeline(
        self: &Arc<Self>,
        run: &Run,
        req: &ExportReq,
        cancel: &mut watch::Receiver<bool>,
        results: &mut Vec<ExportResult>,
    ) -> Result<(), Stop> {
        let d = &self.deps;
        let fail = |m: &str| Stop::Failed(m.to_owned());
        let Some(docker) = d.toolchain.resolve_binary("docker") else {
            return Err(fail(docker::not_ready_message(DockerState::Missing)));
        };
        let env = d.toolchain.spawn_env();
        let (uid, gid) = docker::host_ids();

        // D7: the release's saved source, never the live project folder.
        let src = d
            .paths
            .data_dir
            .join("tmp")
            .join(format!("export-{}", run.job_id));
        let snapshot_ref = req.release.snapshot_ref.clone().unwrap_or_default();
        let (snapshots, pid, dest) = (d.snapshots.clone(), req.project.id.clone(), src.clone());
        let extracted = tokio::task::spawn_blocking(move || {
            let _ = fs::remove_dir_all(&dest);
            fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
            snapshots
                .archive_build(&pid, &snapshot_ref, &dest)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| Stop::Failed(e.to_string()))?;
        if let Err(e) = extracted {
            return Err(Stop::Failed(format!(
                "Could not read the release's saved source: {e}"
            )));
        }
        if *cancel.borrow() {
            return Err(Stop::Cancelled);
        }
        if project::read_tauri_config(&src).is_none() {
            return Err(fail(
                "Could not read src-tauri/tauri.conf.json in the saved source",
            ));
        }
        let manager = project::detect_package_manager(&src);
        if manager == PackageManager::Bun {
            return Err(fail(
                "Exports support pnpm, npm and yarn projects; this one uses bun.",
            ));
        }

        // Environment: the image, built on first use.
        let tag = docker::image_tag();
        let probe_timeout = *lock(&self.docker_timeouts);
        let present = matches!(
            docker::capture(&docker, &["image", "inspect", &tag], &env, probe_timeout.0).await,
            Captured::Done { ok: true, .. }
        );
        if !present {
            self.build_image(run, &docker, &env, &tag, cancel).await?;
        }

        // Installing, once for both targets (node_modules stays in the temporary folder).
        self.set_phase(run, BuildPhase::Installing);
        Self::fresh_parser(run);
        let mut install = vec![manager.name().to_owned()];
        install.extend(manager.install_args().iter().map(|s| (*s).to_owned()));
        let args = docker::run_args(&RunSpec {
            job_id: &run.job_id,
            src: &src,
            uid,
            gid,
            image: &tag,
            cmd: &install,
        });
        let end = self
            .docker_proc(run, &docker, &env, &args, &src, cancel)
            .await;
        if matches!(end, ProcEnd::Cancelled) {
            return Err(Stop::Cancelled);
        }
        if let Err(m) = self.proc_failure(run, &end) {
            return Err(Stop::Failed(format!("Installing dependencies failed: {m}")));
        }

        for &target in &req.targets {
            if *cancel.borrow() {
                return Err(Stop::Cancelled);
            }
            let result = self
                .export_target(run, req, target, &docker, &env, &src, &tag, manager, cancel)
                .await?;
            results.push(result);
        }
        Ok(())
    }

    /// `docker build` of the embedded Dockerfile (phase Environment).
    async fn build_image(
        &self,
        run: &Run,
        docker: &Path,
        env: &[(String, String)],
        tag: &str,
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<(), Stop> {
        self.set_phase(run, BuildPhase::Environment);
        self.note(
            run,
            "Preparing the build environment (first export only; about 3 GB is downloaded)",
        );
        Self::fresh_parser(run);
        let ctx = self
            .deps
            .paths
            .data_dir
            .join("tmp")
            .join(format!("export-image-{}", run.job_id));
        let written = fs::create_dir_all(&ctx)
            .and_then(|()| fs::write(ctx.join("Dockerfile"), docker::DOCKERFILE));
        if let Err(e) = written {
            return Err(Stop::Failed(format!(
                "Could not prepare the build environment: {e}"
            )));
        }
        let args = vec![
            "build".to_owned(),
            "-t".to_owned(),
            tag.to_owned(),
            ctx.display().to_string(),
        ];
        let end = self
            .docker_proc(run, docker, env, &args, &ctx, cancel)
            .await;
        if matches!(end, ProcEnd::Cancelled) {
            return Err(Stop::Cancelled);
        }
        self.proc_failure(run, &end)
            .map_err(|m| Stop::Failed(format!("Preparing the build environment failed: {m}")))
    }

    /// One target: the build in the container, locating its bundles, placing them. `Err` only
    /// for Stop; every other failure is the target's result.
    #[allow(clippy::too_many_arguments)] // one call site; a struct would only rename them
    async fn export_target(
        self: &Arc<Self>,
        run: &Run,
        req: &ExportReq,
        target: ExportTarget,
        docker: &Path,
        env: &[(String, String)],
        src: &Path,
        tag: &str,
        manager: PackageManager,
        cancel: &mut watch::Receiver<bool>,
    ) -> Result<ExportResult, Stop> {
        let d = &self.deps;
        let failed = |message: String| ExportResult {
            target,
            ok: false,
            message: Some(message),
        };
        let (phase, bundles, specs, extra): (_, &str, &[BundleSpec], &[&str]) = match target {
            ExportTarget::Linux => (
                BuildPhase::ExportLinux,
                "appimage,deb,rpm",
                &LINUX_BUNDLES,
                &[],
            ),
            ExportTarget::Windows => (
                BuildPhase::ExportWindows,
                "nsis",
                &WINDOWS_BUNDLES,
                &[
                    "--runner",
                    "cargo-xwin",
                    "--target",
                    "x86_64-pc-windows-msvc",
                ],
            ),
        };
        self.set_phase(run, phase);
        Self::fresh_parser(run);
        let mut cmd = vec![manager.name().to_owned()];
        cmd.extend(manager.tauri_build_args(bundles));
        cmd.extend(extra.iter().map(|s| (*s).to_owned()));
        cmd.extend([
            "--config".to_owned(),
            TauriAppImageAdapter::version_config(&req.release.version),
        ]);
        let (uid, gid) = docker::host_ids();
        let args = docker::run_args(&RunSpec {
            job_id: &run.job_id,
            src,
            uid,
            gid,
            image: tag,
            cmd: &cmd,
        });
        let started = SystemTime::now();
        let end = self.docker_proc(run, docker, env, &args, src, cancel).await;
        if matches!(end, ProcEnd::Cancelled) {
            return Err(Stop::Cancelled);
        }
        if let Err(m) = self.proc_failure(run, &end) {
            return Ok(failed(m));
        }

        let mut files = Vec::new();
        for spec in specs {
            match adapter::locate_bundle(src, &[], started, spec) {
                Ok(p) => files.push(p),
                Err(f) => return Ok(failed(f.message)),
            }
        }
        let (db, release, job_id) = (d.db.clone(), req.release.clone(), run.job_id.clone());
        let release_dir =
            AppLayout::new(&d.paths, &req.project.id).release_dir(&req.release.version);
        let placed = tokio::task::spawn_blocking(move || {
            place_export(&db, &release, &release_dir, &job_id, target, &files)
        })
        .await;
        Ok(match placed {
            Ok(Ok(())) => ExportResult {
                target,
                ok: true,
                message: None,
            },
            Ok(Err(m)) => failed(m),
            Err(e) => failed(e.to_string()),
        })
    }
}
