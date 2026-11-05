//! Rules for Claude in projects whose app keeps its data in a database (D7). Appended to the
//! system prompt only when project.json has the `database` entry. Built from the shared
//! convention constants so the text cannot disagree with them.

use crate::services::appdb::convention::{INSTALLED_DB, MIGRATIONS_DIR, PREVIEW_DB};

pub(crate) fn database_rules() -> String {
    format!(
        "This app stores its data in a SQLite database.\n\
         - The database is used with rusqlite in the app's Rust code (src-tauri/src/db.rs). The frontend never runs SQL; it calls Tauri commands.\n\
         - Change the schema only with a new numbered migration file `{MIGRATIONS_DIR}/NNNN_name.sql` (the next number) and add it to the MIGRATIONS list in db.rs. Never edit a migration that was already applied.\n\
         - PRAGMA user_version equals the highest migration number. The db module sets it; do not set it in a migration.\n\
         - The data files are `{INSTALLED_DB}` (installed app) and `{PREVIEW_DB}` (Preview) in the app's data folder. Never delete or overwrite the user's data files.\n\
         - Do not store passwords or secrets in the database."
    )
}
