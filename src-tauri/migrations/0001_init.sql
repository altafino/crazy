CREATE TABLE projects (
    id               TEXT PRIMARY KEY,
    name             TEXT NOT NULL,
    slug             TEXT NOT NULL UNIQUE,
    template_id      TEXT NOT NULL,
    app_id           TEXT NOT NULL,
    workspace_path   TEXT NOT NULL UNIQUE,
    archived         INTEGER NOT NULL DEFAULT 0,
    pending_op       TEXT NULL CHECK (pending_op IN ('create', 'copy')),
    -- No foreign key until agent_profiles exists (Milestone 2).
    default_agent_id TEXT NULL,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL,
    last_opened_at   TEXT NULL
);

-- value is JSON text.
CREATE TABLE app_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
