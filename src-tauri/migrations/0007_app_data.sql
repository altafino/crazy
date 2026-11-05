-- App databases (Phase 1, SQLite): the schema version of each built release (the highest
-- migration number in the built source; NULL when the app has no migrations) and the backups
-- Studio keeps of an app's database files. A column and a table only (no rebuild).
ALTER TABLE releases ADD COLUMN schema_version INTEGER NULL;

-- `path` is the backup file under `<data>/app-data-backups/<project-id>/`. A `release` backup is
-- the automatic "before <version>" copy; it belongs to the release whose data it holds and is
-- deleted with that release's files (DR-8). `manual` and `restore` backups stay until deleted.
CREATE TABLE app_data_backups (
    id             TEXT PRIMARY KEY,
    project_id     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    which          TEXT NOT NULL CHECK (which IN ('preview', 'installed')),
    reason         TEXT NOT NULL CHECK (reason IN ('manual', 'release', 'restore')),
    release_id     TEXT NULL REFERENCES releases(id) ON DELETE CASCADE,
    label          TEXT NULL,
    path           TEXT NOT NULL,
    size_bytes     INTEGER NOT NULL,
    schema_version INTEGER NULL,
    created_at     TEXT NOT NULL
);
CREATE INDEX app_data_backups_project ON app_data_backups(project_id, created_at);
CREATE INDEX app_data_backups_release ON app_data_backups(release_id);
