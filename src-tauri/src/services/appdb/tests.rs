//! appdb tests: real SQLite files in temp folders, a fake process table, a real slot table.
//! The data root is `<tmp>/data`, never the real `~/.local/share`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;
use tempfile::TempDir;

use super::*;
use crate::domain::{DatabaseEntry, ProjectMeta, TemplateId};
use crate::persistence::{InstallRow, NewProject, ReleaseRow, Smoke};

#[derive(Default)]
struct FakeProcs(Mutex<Vec<PathBuf>>);

impl ProcessScan for FakeProcs {
    fn appimage_vars(&self) -> Vec<PathBuf> {
        self.0.lock().unwrap().clone()
    }
}

struct Fx {
    tmp: TempDir,
    db: Arc<Db>,
    paths: StudioPaths,
    slots: Arc<Slots>,
    procs: Arc<FakeProcs>,
    app: AppDb,
}

const IDENT: &str = "com.test.app";

impl Fx {
    fn new() -> Self {
        Self::with(true, IDENT)
    }

    fn with(entry: bool, identifier: &str) -> Self {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path().join("ws");
        fs::create_dir_all(ws.join("src-tauri")).unwrap();
        fs::write(
            ws.join("src-tauri/tauri.conf.json"),
            format!(r#"{{"productName":"My App","identifier":"{identifier}"}}"#),
        )
        .unwrap();
        let mut meta = ProjectMeta::new("My App", "my-app", TemplateId::TauriVue, identifier);
        if entry {
            meta.database = Some(DatabaseEntry {
                engine: convention::ENGINE_SQLITE.into(),
            });
        }
        meta.write(&ws).unwrap();
        let db = Arc::new(Db::open_in_memory().unwrap());
        db.insert_project(&NewProject {
            id: "p1".into(),
            name: "My App".into(),
            slug: "my-app".into(),
            template_id: TemplateId::TauriVue,
            app_id: identifier.into(),
            workspace_path: ws.display().to_string(),
            pending_op: None,
        })
        .unwrap();
        let paths = StudioPaths {
            data_dir: tmp.path().join("data/ai-app-studio"),
            config_dir: tmp.path().join("config"),
            cache_dir: tmp.path().join("cache"),
            default_projects_dir: tmp.path().join("projects"),
            applications_dir: tmp.path().join("data/applications"),
        };
        let slots = Arc::new(Slots::default());
        let procs = Arc::new(FakeProcs::default());
        let app = AppDb::new(db.clone(), &paths)
            .with_slots(slots.clone())
            .with_procs(procs.clone());
        Self {
            tmp,
            db,
            paths,
            slots,
            procs,
            app,
        }
    }

    /// `<tmp>/data/<identifier>/<convention file>`
    fn file(&self, which: DataSet) -> PathBuf {
        let rel = match which {
            DataSet::Preview => convention::PREVIEW_DB,
            DataSet::Installed => convention::INSTALLED_DB,
        };
        self.tmp.path().join("data").join(IDENT).join(rel)
    }

    fn install(&self) {
        self.db
            .upsert_install(&InstallRow {
                project_id: "p1".into(),
                desktop_file: "/d".into(),
                installed_at: "t".into(),
            })
            .unwrap();
    }

    fn release(&self, id: &str, version: &str, schema: Option<i64>) {
        self.db
            .insert_release(&ReleaseRow {
                id: id.into(),
                project_id: "p1".into(),
                job_id: None,
                version: version.into(),
                sha256: "x".into(),
                size_bytes: 1,
                architecture: "x86_64".into(),
                build_profile: "local".into(),
                product_name: "My App".into(),
                main_binary_name: "my-app".into(),
                source_commit: None,
                dirty_source: false,
                snapshot_ref: None,
                smoke: Smoke::Passed,
                smoke_detail: None,
                notes: None,
                created_at: "2026-01-01T00:00:00Z".into(),
                last_activated_at: None,
                files_removed_at: None,
                schema_version: schema,
            })
            .unwrap();
    }

    fn installed_root(&self) -> PathBuf {
        AppLayout::new(&self.paths, "p1").root().to_owned()
    }
}

/// Creates (or extends) a database file with `sql`.
fn sqlite(path: &Path, sql: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    Connection::open(path).unwrap().execute_batch(sql).unwrap();
}

const SCORES: &str = "CREATE TABLE scores (id INTEGER PRIMARY KEY, player TEXT, points INTEGER);
    INSERT INTO scores (player, points) VALUES ('ada', 10), ('bob', 20), ('cy', 42);
    CREATE TABLE \"we\"\"ird table\" (a TEXT, b BLOB);
    INSERT INTO \"we\"\"ird table\" VALUES ('x', NULL);
    PRAGMA user_version = 4;";

fn scores(fx: &Fx, which: DataSet) {
    sqlite(&fx.file(which), SCORES);
}

fn text(v: &str) -> Cell {
    Cell::Text { value: v.into() }
}

fn int(v: i64) -> Cell {
    Cell::Integer {
        value: v.to_string(),
    }
}

fn read(path: &Path, sql: &str) -> String {
    Connection::open(path)
        .unwrap()
        .query_row(sql, [], |r| r.get::<_, String>(0))
        .unwrap()
}

// ------------------------------------------------------------ identifier

#[test]
fn identifiers_are_validated_before_they_become_paths() {
    for ok in ["com.test.app", "com.aiappstudio.my-app_2"] {
        assert!(validate_identifier(ok).is_ok(), "{ok}");
    }
    for bad in [
        "",
        "app",
        "..",
        "com..app",
        ".com.app",
        "com.app.",
        "../evil.x",
        "com.x/y",
        "com.x\\y",
        "com.x y",
    ] {
        assert!(
            matches!(validate_identifier(bad), Err(AppDbError::BadIdentifier(_))),
            "{bad:?}"
        );
    }
}

#[test]
fn an_invalid_identifier_is_refused_and_touches_nothing() {
    let fx = Fx::with(true, "../evil");
    for r in [
        fx.app.overview("p1", DataSet::Preview).map(|_| ()),
        fx.app.backup("p1", DataSet::Preview).map(|_| ()),
        fx.app.reset_preview("p1"),
    ] {
        assert!(matches!(r, Err(AppDbError::BadIdentifier(_))), "{r:?}");
    }
    assert!(!fx.tmp.path().join("evil").exists());
}

// -------------------------------------------------------------- overview

#[test]
fn overview_without_a_database_entry() {
    let fx = Fx::with(false, IDENT);
    let o = fx.app.overview("p1", DataSet::Preview).unwrap();
    assert_eq!(o.state, DbState::NoDatabase);
    assert_eq!(o.path, None);
}

#[test]
fn overview_without_a_file_names_the_path() {
    let fx = Fx::new();
    let o = fx.app.overview("p1", DataSet::Preview).unwrap();
    assert_eq!(o.state, DbState::NoFile);
    assert_eq!(
        o.path,
        Some(fx.file(DataSet::Preview).display().to_string())
    );
    assert!(o.tables.is_empty());
}

#[test]
fn installed_data_of_an_app_that_is_not_installed() {
    let fx = Fx::new();
    scores(&fx, DataSet::Installed);
    let o = fx.app.overview("p1", DataSet::Installed).unwrap();
    assert_eq!(o.state, DbState::NotInstalled);
    fx.install();
    let o = fx.app.overview("p1", DataSet::Installed).unwrap();
    assert_eq!(o.state, DbState::Ok);
    // Installed but never opened: no file yet.
    fs::remove_file(fx.file(DataSet::Installed)).unwrap();
    let o = fx.app.overview("p1", DataSet::Installed).unwrap();
    assert_eq!(o.state, DbState::NoFile);
}

#[test]
fn overview_of_a_readable_file() {
    let fx = Fx::new();
    scores(&fx, DataSet::Preview);
    let o = fx.app.overview("p1", DataSet::Preview).unwrap();
    assert_eq!(o.state, DbState::Ok);
    assert_eq!(o.schema_version, Some(4));
    assert!(o.size_bytes.unwrap() > 0.0);
    assert!(o.modified_at.unwrap().ends_with('Z'));
    let t: Vec<_> = o
        .tables
        .iter()
        .map(|t| (t.name.as_str(), t.row_count, t.many_rows))
        .collect();
    assert_eq!(
        t,
        [
            ("scores", Some(3.0), false),
            ("we\"ird table", Some(1.0), false)
        ]
    );
}

#[test]
fn overview_of_a_damaged_file_carries_the_raw_error() {
    let fx = Fx::new();
    let f = fx.file(DataSet::Preview);
    fs::create_dir_all(f.parent().unwrap()).unwrap();
    fs::write(&f, "this is not a database, just some text ".repeat(50)).unwrap();
    let o = fx.app.overview("p1", DataSet::Preview).unwrap();
    assert_eq!(o.state, DbState::Damaged);
    assert!(o.error.unwrap().contains("not a database"));
    assert!(o.size_bytes.unwrap() > 0.0, "still says what the file is");
}

#[test]
fn reading_works_while_the_app_has_the_database_open_in_wal_mode() {
    let fx = Fx::new();
    let f = fx.file(DataSet::Preview);
    scores(&fx, DataSet::Preview);
    let writer = Connection::open(&f).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer
        .execute("INSERT INTO scores (player, points) VALUES ('dee', 1)", [])
        .unwrap();
    let o = fx.app.overview("p1", DataSet::Preview).unwrap();
    assert_eq!(o.tables[0].row_count, Some(4.0), "sees the WAL content");
    drop(writer);
}

// ------------------------------------------------------------------ rows

fn rows(fx: &Fx, table: &str, offset: u32, filter: Option<&str>) -> RowsPage {
    fx.app
        .rows("p1", DataSet::Preview, table, offset, filter)
        .unwrap()
}

#[test]
fn rows_of_a_table_named_with_a_quote_and_a_space() {
    let fx = Fx::new();
    scores(&fx, DataSet::Preview);
    let p = rows(&fx, "we\"ird table", 0, None);
    assert_eq!(p.columns, ["a", "b"]);
    assert_eq!(p.rows, [[text("x"), Cell::Null]]);
    assert_eq!(p.total, Some(1.0));
}

#[test]
fn an_unknown_or_injected_table_name_is_refused() {
    let fx = Fx::new();
    scores(&fx, DataSet::Preview);
    for name in [
        "nope",
        "scores\"; DROP TABLE scores; --",
        "sqlite_master",
        "scores\" UNION SELECT 1 --",
    ] {
        let r = fx.app.rows("p1", DataSet::Preview, name, 0, None);
        assert!(
            matches!(r, Err(AppDbError::NoSuchTable(_))),
            "{name}: {r:?}"
        );
    }
    assert_eq!(rows(&fx, "scores", 0, None).total, Some(3.0));
}

#[test]
fn rows_carry_typed_cells_and_never_blob_bytes() {
    let fx = Fx::new();
    sqlite(
        &fx.file(DataSet::Preview),
        "CREATE TABLE t (n INTEGER, r REAL, s TEXT, b BLOB, z);
         INSERT INTO t VALUES (9223372036854775807, 1.5, 'héllo', x'00FF10', NULL);",
    );
    let p = rows(&fx, "t", 0, None);
    assert_eq!(
        p.rows,
        [[
            int(i64::MAX),
            Cell::Real { value: 1.5 },
            text("héllo"),
            Cell::Blob { size: 3.0 },
            Cell::Null
        ]]
    );
    let json = serde_json::to_string(&p.rows).unwrap();
    assert!(json.contains(r#"{"kind":"blob","size":3.0}"#), "{json}");
    assert!(!json.contains("255") && !json.contains("\\u0000"));
}

#[test]
fn paging_is_50_rows_with_a_total() {
    let fx = Fx::new();
    sqlite(
        &fx.file(DataSet::Preview),
        "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT);
         WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c WHERE x < 120)
         INSERT INTO t SELECT x, 'v' || x FROM c;",
    );
    let p = rows(&fx, "t", 0, None);
    assert_eq!((p.rows.len(), p.total, p.page_size), (50, Some(120.0), 50));
    assert_eq!(p.rows[0][0], int(1));
    let p = rows(&fx, "t", 100, None);
    assert_eq!(p.rows.len(), 20);
    assert_eq!(p.rows[0][0], int(101));
    assert_eq!(rows(&fx, "t", 500, None).rows.len(), 0);
}

#[test]
fn the_filter_is_a_bound_like_over_every_column() {
    let fx = Fx::new();
    sqlite(
        &fx.file(DataSet::Preview),
        "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, note TEXT, score INTEGER);
         INSERT INTO t (name, note, score) VALUES
           ('Ada', NULL, 7), ('bob', '100% sure', 42), ('cy', 'a_b', 3), ('dee', 'a\\b', 5);",
    );
    let names = |f: &str| -> Vec<Cell> {
        rows(&fx, "t", 0, Some(f))
            .rows
            .into_iter()
            .map(|r| r[1].clone())
            .collect()
    };
    assert_eq!(names("ada"), [text("Ada")], "case-insensitive");
    assert_eq!(names("42"), [text("bob")], "numbers match as text");
    assert_eq!(names("100%"), [text("bob")], "% is literal");
    assert_eq!(names("a_b"), [text("cy")], "_ is literal");
    assert_eq!(names("a\\b"), [text("dee")], "backslash is literal");
    assert!(names("' OR 1=1 --").is_empty(), "no injection");
    let none = rows(&fx, "t", 0, Some("zzz"));
    assert_eq!((none.rows.len(), none.total), (0, Some(0.0)));
    let all = rows(&fx, "t", 0, Some(""));
    assert_eq!(all.total, Some(4.0), "an empty filter is no filter");
}

// ----------------------------------------------------------------- limit

const SLOW: &str =
    "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c WHERE x < 500000000)
     SELECT COUNT(*) FROM c";

#[test]
fn a_slow_count_is_interrupted_at_the_limit_and_zero_means_none() {
    let mut conn = Connection::open_in_memory().unwrap();
    let started = std::time::Instant::now();
    let n = inspect::count_for_test(&mut conn, SLOW, Some(Duration::from_millis(20)));
    assert_eq!(n, None, "interrupted");
    assert!(started.elapsed() < Duration::from_secs(5));
    let fast = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c WHERE x < 1000)
         SELECT COUNT(*) FROM c";
    assert_eq!(inspect::count_for_test(&mut conn, fast, None), Some(1000.0));
    assert_eq!(
        inspect::count_for_test(&mut conn, fast, Some(Duration::from_secs(30))),
        Some(1000.0)
    );
}

#[test]
fn a_filter_over_the_limit_is_flagged_and_the_connection_stays_usable() {
    let fx = Fx::new();
    sqlite(
        &fx.file(DataSet::Preview),
        "CREATE TABLE big (id INTEGER PRIMARY KEY, v TEXT);
         WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c WHERE x < 400000)
         INSERT INTO big SELECT x, 'row ' || x FROM c;",
    );
    let slow = fx
        .app
        .rows_with(
            "p1",
            DataSet::Preview,
            "big",
            0,
            Some("zzz"),
            Some(Duration::from_millis(1)),
        )
        .unwrap();
    assert!(slow.timed_out && slow.total_over_limit);
    assert!(slow.rows.is_empty() && slow.total.is_none());
    // Without a limit the same filter finishes (and matches nothing).
    let full = fx
        .app
        .rows_with("p1", DataSet::Preview, "big", 0, Some("zzz"), None)
        .unwrap();
    assert!(!full.timed_out);
    assert_eq!(full.total, Some(0.0));
    // A plain page is fast and inside any limit.
    let page = fx
        .app
        .rows_with(
            "p1",
            DataSet::Preview,
            "big",
            0,
            None,
            Some(Duration::from_secs(30)),
        )
        .unwrap();
    assert_eq!((page.rows.len(), page.total), (50, Some(400_000.0)));
}

// --------------------------------------------------------------- backups

#[test]
fn backup_copies_the_data_under_studios_own_folder() {
    let fx = Fx::new();
    scores(&fx, DataSet::Preview);
    let b = fx.app.backup("p1", DataSet::Preview).unwrap();
    assert_eq!(b.reason, BackupReason::Manual);
    assert_eq!(b.which, DataSet::Preview);
    assert_eq!(b.schema_version, Some(4), "keeps user_version");
    assert!(b.size_bytes > 0.0 && b.file_exists);
    let row = fx.db.get_backup(&b.id).unwrap().unwrap();
    let dir = fx.paths.data_dir.join("app-data-backups/p1");
    assert!(Path::new(&row.path).starts_with(&dir));
    assert_eq!(
        read(
            Path::new(&row.path),
            "SELECT player FROM scores WHERE points = 42"
        ),
        "cy"
    );
    assert_eq!(fx.app.list_backups("p1", None).unwrap(), vec![b]);
    assert!(
        fx.app
            .list_backups("p1", Some(DataSet::Installed))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn backup_includes_changes_still_in_the_wal_of_a_running_app() {
    let fx = Fx::new();
    let f = fx.file(DataSet::Installed);
    scores(&fx, DataSet::Installed);
    let writer = Connection::open(&f).unwrap();
    writer.pragma_update(None, "journal_mode", "WAL").unwrap();
    writer
        .execute("INSERT INTO scores (player, points) VALUES ('wal', 99)", [])
        .unwrap();
    let b = fx.app.backup("p1", DataSet::Installed).unwrap();
    let row = fx.db.get_backup(&b.id).unwrap().unwrap();
    assert_eq!(
        read(
            Path::new(&row.path),
            "SELECT player FROM scores WHERE points = 99"
        ),
        "wal"
    );
    drop(writer);
}

#[test]
fn backup_without_a_file_or_database_is_refused() {
    let fx = Fx::new();
    assert!(matches!(
        fx.app.backup("p1", DataSet::Preview),
        Err(AppDbError::NoFile)
    ));
    let none = Fx::with(false, IDENT);
    assert!(matches!(
        none.app.backup("p1", DataSet::Preview),
        Err(AppDbError::NoDatabase)
    ));
}

#[test]
fn delete_backup_removes_manual_and_restore_but_not_automatic_ones() {
    let fx = Fx::new();
    scores(&fx, DataSet::Preview);
    scores(&fx, DataSet::Installed);
    fx.release("r1", "1.0.0", Some(4));
    let manual = fx.app.backup("p1", DataSet::Preview).unwrap();
    let path = fx.db.get_backup(&manual.id).unwrap().unwrap().path;
    fx.app.delete_backup("p1", &manual.id).unwrap();
    assert!(!Path::new(&path).exists());
    assert!(fx.app.list_backups("p1", None).unwrap().is_empty());
    let auto = fx
        .app
        .backup_before_activation("p1", "1.1.0", "r1")
        .unwrap()
        .unwrap();
    assert!(matches!(
        fx.app.delete_backup("p1", &auto.id),
        Err(AppDbError::AutomaticBackup)
    ));
    assert!(matches!(
        fx.app.delete_backup("p1", "missing"),
        Err(AppDbError::NoSuchBackup)
    ));
}

// --------------------------------------------------------------- restore

#[test]
fn restore_saves_the_current_data_first_then_replaces_the_file() {
    let fx = Fx::new();
    let f = fx.file(DataSet::Installed);
    sqlite(
        &f,
        "CREATE TABLE t (v TEXT); INSERT INTO t VALUES ('old'); PRAGMA user_version = 2;",
    );
    let old = fx.app.backup("p1", DataSet::Installed).unwrap();
    // The app moved on: new data, a stale journal beside the file.
    fs::remove_file(&f).unwrap();
    sqlite(
        &f,
        "CREATE TABLE t (v TEXT); INSERT INTO t VALUES ('new'); PRAGMA user_version = 3;",
    );
    fs::write(sibling_of(&f, "-wal"), "stale").unwrap();
    fs::write(sibling_of(&f, "-shm"), "stale").unwrap();

    let before = fx.app.restore("p1", &old.id).unwrap().unwrap();

    assert_eq!(read(&f, "SELECT v FROM t"), "old");
    assert!(!sibling_of(&f, "-wal").exists() && !sibling_of(&f, "-shm").exists());
    assert_eq!(before.reason, BackupReason::Restore);
    assert_eq!(before.label.as_deref(), Some("before restore"));
    assert_eq!(before.schema_version, Some(3));
    let row = fx.db.get_backup(&before.id).unwrap().unwrap();
    assert_eq!(read(Path::new(&row.path), "SELECT v FROM t"), "new");
    // The restored backup stays, and so does the new one.
    assert_eq!(fx.app.list_backups("p1", None).unwrap().len(), 2);
    assert!(!fs::read_dir(f.parent().unwrap()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".restoring")
    }));
}

fn sibling_of(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

#[test]
fn restore_into_a_set_with_no_file_makes_no_before_backup() {
    let fx = Fx::new();
    scores(&fx, DataSet::Preview);
    let b = fx.app.backup("p1", DataSet::Preview).unwrap();
    fs::remove_file(fx.file(DataSet::Preview)).unwrap();
    assert_eq!(fx.app.restore("p1", &b.id).unwrap(), None);
    assert_eq!(
        fx.app.overview("p1", DataSet::Preview).unwrap().state,
        DbState::Ok
    );
    assert_eq!(fx.app.list_backups("p1", None).unwrap().len(), 1);
}

#[test]
fn restore_with_a_missing_backup_file_changes_nothing() {
    let fx = Fx::new();
    scores(&fx, DataSet::Preview);
    let b = fx.app.backup("p1", DataSet::Preview).unwrap();
    fs::remove_file(fx.db.get_backup(&b.id).unwrap().unwrap().path).unwrap();
    assert!(!fx.app.list_backups("p1", None).unwrap()[0].file_exists);
    assert!(matches!(
        fx.app.restore("p1", &b.id),
        Err(AppDbError::BackupFileMissing(_))
    ));
    assert_eq!(fx.app.list_backups("p1", None).unwrap().len(), 1);
}

// ---------------------------------------------------------------- guards

#[test]
fn restore_copy_and_reset_refuse_while_preview_runs() {
    let fx = Fx::new();
    scores(&fx, DataSet::Preview);
    scores(&fx, DataSet::Installed);
    let b = fx.app.backup("p1", DataSet::Preview).unwrap();
    assert!(fx.slots.claim("p1", JobKind::Preview, "job"));
    assert!(fx.app.guard("p1").unwrap().preview_running);
    let before = fs::read(fx.file(DataSet::Preview)).unwrap();
    for r in [
        fx.app.restore("p1", &b.id).map(|_| ()),
        fx.app.copy_installed_to_preview("p1"),
        fx.app.reset_preview("p1"),
    ] {
        assert!(matches!(r, Err(AppDbError::PreviewRunning)), "{r:?}");
    }
    assert_eq!(fs::read(fx.file(DataSet::Preview)).unwrap(), before);
    assert_eq!(
        fx.app.list_backups("p1", None).unwrap().len(),
        1,
        "no before-restore copy"
    );
    // The installed data is not what Preview uses: restoring it is allowed.
    let ib = fx.app.backup("p1", DataSet::Installed).unwrap();
    fx.app.restore("p1", &ib.id).unwrap();
    // Stopped: everything works again.
    fx.slots.release("p1", JobKind::Preview);
    assert!(!fx.app.guard("p1").unwrap().preview_running);
    fx.app.reset_preview("p1").unwrap();
}

#[test]
fn restore_refuses_while_the_installed_app_runs() {
    let fx = Fx::new();
    fx.release("r1", "1.0.0", None);
    scores(&fx, DataSet::Installed);
    scores(&fx, DataSet::Preview);
    let b = fx.app.backup("p1", DataSet::Installed).unwrap();
    // Another app's AppImage and Studio's own do not count.
    *fx.procs.0.lock().unwrap() = vec![
        PathBuf::from("/home/u/Apps/Studio.AppImage"),
        fx.paths
            .data_dir
            .join("installed/other/releases/1.0.0/app.AppImage"),
    ];
    assert!(!fx.app.guard("p1").unwrap().installed_running);
    fx.app.restore("p1", &b.id).unwrap();
    // Our release's AppImage (even the symlink path) does.
    fx.procs
        .0
        .lock()
        .unwrap()
        .push(fx.installed_root().join("releases/1.0.0/app.AppImage"));
    let g = fx.app.guard("p1").unwrap();
    assert!(g.installed_running);
    assert_eq!(g.app_name, "My App");
    let n = fx.app.list_backups("p1", None).unwrap().len();
    match fx.app.restore("p1", &b.id) {
        Err(e @ AppDbError::InstalledRunning { .. }) => {
            assert_eq!(e.to_string(), "Quit My App first.")
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(fx.app.list_backups("p1", None).unwrap().len(), n);
    // Preview actions are not blocked by the installed app.
    fx.app.reset_preview("p1").unwrap();
}

#[test]
fn procfs_scan_reads_the_appimage_variable_of_each_process() {
    let proc = TempDir::new().unwrap();
    for (pid, env) in [
        ("101", &b"HOME=/h\0APPIMAGE=/a/app.AppImage\0X=1"[..]),
        ("102", &b"HOME=/h\0NOTAPPIMAGE=/x\0"[..]),
    ] {
        fs::create_dir(proc.path().join(pid)).unwrap();
        fs::write(proc.path().join(pid).join("environ"), env).unwrap();
    }
    fs::create_dir(proc.path().join("103")).unwrap(); // unreadable: no environ
    fs::write(proc.path().join("self-note"), "x").unwrap();
    assert_eq!(
        Procfs::at(proc.path()).appimage_vars(),
        vec![PathBuf::from("/a/app.AppImage")]
    );
    assert!(
        Procfs::at("/definitely/not/here")
            .appimage_vars()
            .is_empty()
    );
}

// ------------------------------------------------------- copy and reset

#[test]
fn copy_installed_to_preview_replaces_preview_and_leaves_installed_alone() {
    let fx = Fx::new();
    sqlite(
        &fx.file(DataSet::Installed),
        "CREATE TABLE t (v TEXT); INSERT INTO t VALUES ('real'); PRAGMA user_version = 5;",
    );
    sqlite(
        &fx.file(DataSet::Preview),
        "CREATE TABLE t (v TEXT); INSERT INTO t VALUES ('dev');",
    );
    let p = fx.file(DataSet::Preview);
    fs::write(sibling_of(&p, "-wal"), "stale").unwrap();
    fs::write(sibling_of(&p, "-shm"), "stale").unwrap();
    let installed_before = fs::read(fx.file(DataSet::Installed)).unwrap();

    fx.app.copy_installed_to_preview("p1").unwrap();

    assert_eq!(read(&p, "SELECT v FROM t"), "real");
    assert_eq!(
        fx.app
            .overview("p1", DataSet::Preview)
            .unwrap()
            .schema_version,
        Some(5)
    );
    assert!(!sibling_of(&p, "-wal").exists() && !sibling_of(&p, "-shm").exists());
    assert_eq!(
        fs::read(fx.file(DataSet::Installed)).unwrap(),
        installed_before
    );
    assert!(!sibling_of(&p, ".copying").exists());
}

#[test]
fn copy_installed_to_preview_creates_the_dev_folder_and_needs_a_source() {
    let fx = Fx::new();
    assert!(matches!(
        fx.app.copy_installed_to_preview("p1"),
        Err(AppDbError::NoFile)
    ));
    scores(&fx, DataSet::Installed);
    fx.app.copy_installed_to_preview("p1").unwrap();
    assert!(fx.file(DataSet::Preview).exists());
}

#[test]
fn reset_preview_deletes_the_file_and_its_wal_files_only() {
    let fx = Fx::new();
    scores(&fx, DataSet::Preview);
    scores(&fx, DataSet::Installed);
    let p = fx.file(DataSet::Preview);
    fs::write(sibling_of(&p, "-wal"), "w").unwrap();
    fs::write(sibling_of(&p, "-shm"), "s").unwrap();
    fx.app.reset_preview("p1").unwrap();
    assert!(!p.exists() && !sibling_of(&p, "-wal").exists() && !sibling_of(&p, "-shm").exists());
    assert!(fx.file(DataSet::Installed).exists());
    assert_eq!(
        fx.app.overview("p1", DataSet::Preview).unwrap().state,
        DbState::NoFile
    );
    fx.app.reset_preview("p1").unwrap(); // nothing left: still fine
}

// ------------------------------------------------- release hooks, rollback

#[test]
fn backup_before_activation_is_labelled_owned_by_a_release_and_deleted_with_it() {
    let fx = Fx::new();
    fx.release("r1", "1.0.0", Some(3));
    fx.release("r2", "1.1.0", Some(4));
    // No file yet: nothing to back up.
    assert_eq!(
        fx.app
            .backup_before_activation("p1", "1.0.0", "r1")
            .unwrap(),
        None
    );
    scores(&fx, DataSet::Installed);
    let auto = fx
        .app
        .backup_before_activation("p1", "1.1.0", "r1")
        .unwrap()
        .unwrap();
    assert_eq!(auto.reason, BackupReason::Release);
    assert_eq!(auto.label.as_deref(), Some("before 1.1.0"));
    assert_eq!(auto.release_id.as_deref(), Some("r1"));
    let manual = fx.app.backup("p1", DataSet::Installed).unwrap();
    let auto_path = fx.db.get_backup(&auto.id).unwrap().unwrap().path;

    // A different release's pruning leaves it alone.
    fx.app.delete_release_backups("r2").unwrap();
    assert!(Path::new(&auto_path).exists());
    // Its own release's pruning removes the file and the row, and only that.
    fx.app.delete_release_backups("r1").unwrap();
    assert!(!Path::new(&auto_path).exists());
    let left: Vec<_> = fx
        .app
        .list_backups("p1", None)
        .unwrap()
        .into_iter()
        .map(|b| b.id)
        .collect();
    assert_eq!(left, [manual.id]);
}

#[test]
fn a_project_without_the_entry_makes_no_activation_backup() {
    let fx = Fx::with(false, IDENT);
    fx.release("r1", "1.0.0", None);
    scores(&fx, DataSet::Installed);
    assert_eq!(
        fx.app
            .backup_before_activation("p1", "1.0.0", "r1")
            .unwrap(),
        None
    );
    assert!(fx.app.list_backups("p1", None).unwrap().is_empty());
}

#[test]
fn rollback_check_reports_versions_and_the_backup_of_the_target() {
    let fx = Fx::new();
    fx.release("r1", "1.0.0", Some(3));
    fx.release("r2", "1.1.0", Some(4));
    scores(&fx, DataSet::Installed); // user_version 4
    let auto = fx
        .app
        .backup_before_activation("p1", "1.1.0", "r1")
        .unwrap()
        .unwrap();
    let c = fx.app.rollback_check("p1", "r1").unwrap();
    assert_eq!(c.data_schema_version, Some(4));
    assert_eq!(c.target_schema_version, Some(3));
    assert!(c.older_than_data);
    assert_eq!(c.backup.map(|b| b.id), Some(auto.id));
    // Going forward is not a warning, and r2 has no backup of its own data.
    let c = fx.app.rollback_check("p1", "r2").unwrap();
    assert!(!c.older_than_data && c.backup.is_none());
    // Unknown schema: no claim either way.
    fx.release("r3", "0.9.0", None);
    assert!(!fx.app.rollback_check("p1", "r3").unwrap().older_than_data);
    assert!(matches!(
        fx.app.rollback_check("p1", "nope"),
        Err(AppDbError::NoSuchRelease)
    ));
}

#[test]
fn test_launch_copy_lands_in_the_temp_data_home_and_only_for_installed_apps() {
    let fx = Fx::new();
    let temp_home = fx.tmp.path().join("temp-home");
    scores(&fx, DataSet::Installed);
    // Data exists but the app is not installed: nothing to rehearse on.
    assert!(!fx.app.copy_installed_for_test("p1", &temp_home).unwrap());
    fx.install();
    assert!(fx.app.copy_installed_for_test("p1", &temp_home).unwrap());
    let copy = temp_home.join(IDENT).join("app.db");
    assert_eq!(
        read(&copy, "SELECT player FROM scores WHERE points = 42"),
        "cy"
    );
    let none = Fx::with(false, IDENT);
    none.install();
    assert!(!none.app.copy_installed_for_test("p1", &temp_home).unwrap());
}

#[test]
fn test_launch_copy_of_a_damaged_file_is_an_error() {
    let fx = Fx::new();
    fx.install();
    let f = fx.file(DataSet::Installed);
    fs::create_dir_all(f.parent().unwrap()).unwrap();
    fs::write(&f, "garbage ".repeat(500)).unwrap();
    let temp_home = fx.tmp.path().join("temp-home");
    let e = fx
        .app
        .copy_installed_for_test("p1", &temp_home)
        .unwrap_err();
    assert!(e.to_string().contains("not a database"), "{e}");
    assert!(
        !temp_home.join(IDENT).join("app.db").exists(),
        "no partial copy"
    );
}
