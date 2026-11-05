//! The app database convention shared by the template, the Database tab and Claude's rules.
//! One copy: the template code and the rules text are checked against these by a sync test.

/// Engine name stored in `.ai-app-studio/project.json` under `database.engine`.
pub const ENGINE_SQLITE: &str = "sqlite";
/// Installed (release) app's database, relative to `<XDG_DATA_HOME>/<identifier>/`.
pub const INSTALLED_DB: &str = "app.db";
/// Preview (debug) app's database, relative to `<XDG_DATA_HOME>/<identifier>/`.
pub const PREVIEW_DB: &str = "dev/app.db";
/// Folder of numbered migrations inside the project.
pub const MIGRATIONS_DIR: &str = "src-tauri/migrations";

/// True for a migration file name of the form `NNNN_name.sql` (four digits, underscore, `.sql`).
pub fn is_migration_file(name: &str) -> bool {
    let b = name.as_bytes();
    name.len() > 9
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'_'
        && name.ends_with(".sql")
}

/// Highest migration number in `<project>/src-tauri/migrations`, or None when there is none.
pub fn highest_migration(project_dir: &std::path::Path) -> Option<u32> {
    std::fs::read_dir(project_dir.join(MIGRATIONS_DIR))
        .ok()?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| is_migration_file(n))
        .filter_map(|n| n[..4].parse().ok())
        .max()
}
