//! The read side: what is in an app's database. Every connection is read-only (works while the
//! app writes, WAL included) and every query runs under the Settings time limit (R9/D13).

use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use rusqlite::types::ValueRef;
use rusqlite::{Connection, ErrorCode, OpenFlags};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{AppDb, AppDbError, DataSet, PAGE_SIZE};
use crate::platform::time::rfc3339;

/// The DR-6 situation of one data file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DbState {
    /// The project has no `database` entry: "This app doesn't store data yet".
    NoDatabase,
    /// Set up, but the file does not exist: "No preview data yet" / "hasn't created its data".
    NoFile,
    /// Installed data was asked for and the app is not installed.
    NotInstalled,
    /// The file exists and cannot be read; `error` has SQLite's message.
    Damaged,
    Ok,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TableInfo {
    pub name: String,
    /// `None` when counting took longer than the time limit.
    pub row_count: Option<f64>,
    /// The count hit the limit: show "many rows".
    pub many_rows: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppDbOverview {
    pub which: DataSet,
    pub state: DbState,
    /// SQLite's raw message when `state` is `damaged`.
    pub error: Option<String>,
    /// The data file (set whenever the project has a database).
    pub path: Option<String>,
    pub size_bytes: Option<f64>,
    /// `PRAGMA user_version`: the highest migration the app applied.
    pub schema_version: Option<i32>,
    /// When the file (or its WAL) last changed, RFC 3339 UTC.
    pub modified_at: Option<String>,
    pub tables: Vec<TableInfo>,
}

impl AppDbOverview {
    fn bare(which: DataSet, state: DbState) -> Self {
        Self {
            which,
            state,
            error: None,
            path: None,
            size_bytes: None,
            schema_version: None,
            modified_at: None,
            tables: vec![],
        }
    }
}

/// One cell. BLOB bytes are never sent (R7/D10); integers are text because they can exceed
/// what a JavaScript number holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum Cell {
    Null,
    Integer { value: String },
    Real { value: f64 },
    Text { value: String },
    Blob { size: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RowsPage {
    pub table: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
    pub offset: u32,
    pub page_size: u32,
    /// Rows in the table, or matching the filter. `None` when counting hit the time limit.
    pub total: Option<f64>,
    /// Counting hit the limit: show "many rows".
    pub total_over_limit: bool,
    /// Reading the page hit the limit (with a filter: "Filter took too long; narrow it").
    /// `rows` is empty then.
    pub timed_out: bool,
}

pub(super) fn open_ro(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
}

/// Interrupts whatever the connection runs next once `limit` has passed (`None`: no limit).
fn arm(conn: &mut Connection, limit: Option<Duration>) -> rusqlite::Result<()> {
    match limit {
        Some(d) => {
            let deadline = Instant::now() + d;
            conn.progress_handler(1000, Some(move || Instant::now() >= deadline))
        }
        None => conn.progress_handler(0, None::<fn() -> bool>),
    }
}

fn interrupted(e: &rusqlite::Error) -> bool {
    matches!(e, rusqlite::Error::SqliteFailure(f, _) if f.code == ErrorCode::OperationInterrupted)
}

/// `Ok(None)` when the query ran into the limit.
fn limited<T>(r: rusqlite::Result<T>) -> rusqlite::Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if interrupted(&e) => Ok(None),
        Err(e) => Err(e),
    }
}

/// A name as an SQL identifier: double quotes around it, inner quotes doubled.
pub(super) fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

const TABLES_SQL: &str = "SELECT name FROM sqlite_master
     WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name";

fn count(
    conn: &mut Connection,
    sql: &str,
    args: &[&dyn rusqlite::ToSql],
    limit: Option<Duration>,
) -> rusqlite::Result<Option<f64>> {
    arm(conn, limit)?;
    limited(
        conn.query_row(sql, args, |r| r.get::<_, i64>(0))
            .map(|n| n as f64),
    )
}

#[cfg(test)]
pub(super) fn count_for_test(
    conn: &mut Connection,
    sql: &str,
    limit: Option<Duration>,
) -> Option<f64> {
    count(conn, sql, &[], limit).unwrap()
}

fn read_overview(
    conn: &mut Connection,
    limit: Option<Duration>,
) -> rusqlite::Result<(i32, Vec<TableInfo>)> {
    arm(conn, limit)?;
    let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let names: Vec<String> = conn
        .prepare(TABLES_SQL)?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let mut tables = Vec::with_capacity(names.len());
    for name in names {
        let n = count(
            conn,
            &format!("SELECT COUNT(*) FROM {}", quote_ident(&name)),
            &[],
            limit,
        )?;
        tables.push(TableInfo {
            name,
            many_rows: n.is_none(),
            row_count: n,
        });
    }
    Ok((version, tables))
}

/// `%text%` for LIKE with `\` as the escape character.
fn like_pattern(filter: &str) -> String {
    let mut p = String::with_capacity(filter.len() + 2);
    p.push('%');
    for c in filter.chars() {
        if matches!(c, '%' | '_' | '\\') {
            p.push('\\');
        }
        p.push(c);
    }
    p.push('%');
    p
}

fn cell(v: ValueRef<'_>) -> Cell {
    match v {
        ValueRef::Null => Cell::Null,
        ValueRef::Integer(i) => Cell::Integer {
            value: i.to_string(),
        },
        ValueRef::Real(f) => Cell::Real { value: f },
        ValueRef::Text(t) => Cell::Text {
            value: String::from_utf8_lossy(t).into_owned(),
        },
        ValueRef::Blob(b) => Cell::Blob {
            size: b.len() as f64,
        },
    }
}

fn read_page(
    conn: &mut Connection,
    table: &str,
    offset: u32,
    filter: Option<&str>,
    limit: Option<Duration>,
) -> Result<RowsPage, AppDbError> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master
         WHERE type = 'table' AND name = ?1 AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\')",
        [table],
        |r| r.get(0),
    )?;
    if !exists {
        return Err(AppDbError::NoSuchTable(table.to_owned()));
    }
    let quoted = quote_ident(table);
    let columns: Vec<String> = conn
        .prepare(&format!("SELECT * FROM {quoted} LIMIT 0"))?
        .column_names()
        .map(str::to_owned)
        .collect();
    let (where_sql, pattern) = match filter.filter(|f| !f.is_empty()) {
        Some(f) => (
            format!(
                " WHERE {}",
                columns
                    .iter()
                    .map(|c| format!("CAST({} AS TEXT) LIKE ?1 ESCAPE '\\'", quote_ident(c)))
                    .collect::<Vec<_>>()
                    .join(" OR ")
            ),
            Some(like_pattern(f)),
        ),
        None => (String::new(), None),
    };
    let args: Vec<&dyn rusqlite::ToSql> = pattern.iter().map(|p| p as _).collect();

    let mut page = RowsPage {
        table: table.to_owned(),
        columns,
        rows: vec![],
        offset,
        page_size: PAGE_SIZE,
        total: None,
        total_over_limit: false,
        timed_out: false,
    };

    arm(conn, limit)?;
    let rows = limited((|| {
        let mut stmt = conn.prepare(&format!(
            "SELECT * FROM {quoted}{where_sql} LIMIT {PAGE_SIZE} OFFSET {offset}"
        ))?;
        let n = stmt.column_count();
        let rows = stmt.query_map(args.as_slice(), |r| {
            (0..n).map(|i| r.get_ref(i).map(cell)).collect()
        })?;
        rows.collect::<rusqlite::Result<Vec<Vec<Cell>>>>()
    })())?;
    let Some(rows) = rows else {
        page.timed_out = true;
        page.total_over_limit = true;
        return Ok(page);
    };
    page.rows = rows;

    let total = count(
        conn,
        &format!("SELECT COUNT(*) FROM {quoted}{where_sql}"),
        &args,
        limit,
    )?;
    page.total_over_limit = total.is_none();
    page.total = total;
    Ok(page)
}

/// Newest of the file's and its WAL's modification time.
fn modified(path: &Path) -> Option<String> {
    let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    [mtime(path), mtime(Path::new(&wal))]
        .into_iter()
        .flatten()
        .max()
        .and_then(|t: SystemTime| rfc3339(t))
}

impl AppDb {
    /// DR-6: the state of the project's Preview or Installed data, and when it is readable the
    /// version and the tables with their row counts. Reading problems are a state, not an error.
    pub(crate) fn overview(
        &self,
        project_id: &str,
        which: DataSet,
    ) -> Result<AppDbOverview, AppDbError> {
        self.overview_with(project_id, which, self.query_limit())
    }

    pub(super) fn overview_with(
        &self,
        project_id: &str,
        which: DataSet,
        limit: Option<Duration>,
    ) -> Result<AppDbOverview, AppDbError> {
        let Some(ctx) = self.context(project_id)? else {
            return Ok(AppDbOverview::bare(which, DbState::NoDatabase));
        };
        if which == DataSet::Installed && self.db.get_install(project_id)?.is_none() {
            return Ok(AppDbOverview::bare(which, DbState::NotInstalled));
        }
        let path = self.data_file(&ctx, which);
        let mut out = AppDbOverview::bare(which, DbState::NoFile);
        out.path = Some(path.display().to_string());
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => {
                out.state = DbState::Damaged;
                out.error = Some(e.to_string());
                return Ok(out);
            }
        };
        out.size_bytes = Some(meta.len() as f64);
        out.modified_at = modified(&path);
        match open_ro(&path).and_then(|mut c| read_overview(&mut c, limit)) {
            Ok((version, tables)) => {
                out.state = DbState::Ok;
                out.schema_version = Some(version);
                out.tables = tables;
            }
            Err(e) => {
                out.state = DbState::Damaged;
                out.error = Some(e.to_string());
            }
        }
        Ok(out)
    }

    /// One page (50 rows) of a table, optionally only rows where any column contains `filter`
    /// (case-insensitive for ASCII), with the total. The table name must exist; it is quoted.
    pub(crate) fn rows(
        &self,
        project_id: &str,
        which: DataSet,
        table: &str,
        offset: u32,
        filter: Option<&str>,
    ) -> Result<RowsPage, AppDbError> {
        self.rows_with(project_id, which, table, offset, filter, self.query_limit())
    }

    pub(super) fn rows_with(
        &self,
        project_id: &str,
        which: DataSet,
        table: &str,
        offset: u32,
        filter: Option<&str>,
        limit: Option<Duration>,
    ) -> Result<RowsPage, AppDbError> {
        let ctx = self.require(project_id)?;
        let path = self.data_file(&ctx, which);
        if !path.exists() {
            return Err(AppDbError::NoFile);
        }
        let mut conn = open_ro(&path)?;
        read_page(&mut conn, table, offset, filter, limit)
    }
}
