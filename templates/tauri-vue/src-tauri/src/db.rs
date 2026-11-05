//! The app's SQLite database. All SQL lives here, in Rust; the frontend calls commands.
//!
//! The file is `<app data folder>/app.db` in an installed (release) app and
//! `<app data folder>/dev/app.db` in a Preview (debug) run, so Preview never touches real data.
//!
//! Schema changes are numbered files in `src-tauri/migrations/NNNN_name.sql`. Add each new one to
//! `MIGRATIONS` below. `PRAGMA user_version` is the highest applied number. Each migration runs in
//! its own transaction. A migration that rebuilds a table (create new, copy, drop old, rename)
//! must switch `PRAGMA foreign_keys` off and on outside that transaction, so run it by hand
//! before `migrate` instead of in the list. Never edit a migration that has shipped.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension};
use tauri::State;

const MIGRATIONS: &[(u32, &str)] = &[(1, include_str!("../migrations/0001_init.sql"))];

pub struct Db(pub Mutex<Connection>);

/// Opens (creating folders and file) the database under `data_dir` and brings it up to date.
pub fn open(data_dir: &Path) -> Result<Db, Box<dyn std::error::Error>> {
    let dir = if cfg!(debug_assertions) {
        data_dir.join("dev")
    } else {
        data_dir.to_path_buf()
    };
    std::fs::create_dir_all(&dir)?;
    let mut conn = Connection::open(dir.join("app.db"))?;
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
    migrate(&mut conn)?;
    Ok(Db(Mutex::new(conn)))
}

fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    let current: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (number, sql) in MIGRATIONS.iter().filter(|(n, _)| *n > current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", number)?;
        tx.commit()?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_setting(db: State<Db>, key: String) -> Result<Option<String>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.query_row("SELECT value FROM kv WHERE key = ?1", [&key], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_setting(db: State<Db>, key: String, value: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO kv (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [&key, &value],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_numbered_in_order_and_match_the_folder() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let mut files: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        files.sort();
        assert_eq!(files.len(), MIGRATIONS.len(), "list every migration file");
        for (i, (file, (n, _))) in files.iter().zip(MIGRATIONS).enumerate() {
            assert_eq!(*n as usize, i + 1);
            assert!(file.starts_with(&format!("{n:04}_")), "{file}");
        }
    }

    #[test]
    fn migrate_sets_user_version_and_is_repeatable() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        migrate(&mut conn).unwrap();
        let v: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, MIGRATIONS.last().unwrap().0);
    }
}
