-- Milestone 4: builds are jobs (jobs.kind gains 'build'), and finished builds become
-- releases (M4-D5..D8). The installs table remembers the launcher entry per project.
--
-- jobs.kind needs a table rebuild (SQLite cannot alter a CHECK). The migration runner runs
-- this with foreign keys OFF (outside the transaction) and checks PRAGMA foreign_key_check
-- before committing; with foreign keys on, DROP TABLE jobs would cascade-delete
-- agent_sessions. Rebuild order matters: create jobs_new, copy, drop jobs, then rename
-- jobs_new (see 0003).

CREATE TABLE jobs_new (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind          TEXT NOT NULL DEFAULT 'agent' CHECK (kind IN ('agent', 'preview', 'build')),
    state         TEXT NOT NULL CHECK (state IN
        ('queued','running','waiting_for_approval','succeeded','failed','cancelled','interrupted')),
    started_at    TEXT NULL,
    completed_at  TEXT NULL,
    exit_code     INTEGER NULL,
    error_kind    TEXT NULL,
    error_message TEXT NULL
);
INSERT INTO jobs_new (id, project_id, kind, state, started_at, completed_at, exit_code,
                      error_kind, error_message)
    SELECT id, project_id, kind, state, started_at, completed_at, exit_code,
           error_kind, error_message FROM jobs;
DROP TABLE jobs;
ALTER TABLE jobs_new RENAME TO jobs;
CREATE INDEX jobs_project_started ON jobs(project_id, started_at);

-- One row per built AppImage. There is no "active" column: the current.AppImage symlink is
-- the only record of the active release (M4-D5). Rows outlive their files (retention).
CREATE TABLE releases (
    id                TEXT PRIMARY KEY,
    project_id        TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    job_id            TEXT NULL,
    version           TEXT NOT NULL,
    sha256            TEXT NOT NULL,
    size_bytes        INTEGER NOT NULL,
    architecture      TEXT NOT NULL,
    build_profile     TEXT NOT NULL DEFAULT 'local',
    product_name      TEXT NOT NULL,
    main_binary_name  TEXT NOT NULL,
    source_commit     TEXT NULL,
    dirty_source      INTEGER NOT NULL,
    snapshot_ref      TEXT NULL,
    smoke             TEXT NOT NULL CHECK (smoke IN ('passed', 'failed', 'unverified')),
    smoke_detail      TEXT NULL,
    notes             TEXT NULL,
    created_at        TEXT NOT NULL,
    last_activated_at TEXT NULL,
    files_removed_at  TEXT NULL
);
-- A version is unique per project among releases whose files are kept (M4-D8).
CREATE UNIQUE INDEX releases_project_version_kept
    ON releases(project_id, version) WHERE files_removed_at IS NULL;
CREATE INDEX releases_project_created ON releases(project_id, created_at);

CREATE TABLE installs (
    project_id   TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    desktop_file TEXT NOT NULL,
    installed_at TEXT NOT NULL
);
