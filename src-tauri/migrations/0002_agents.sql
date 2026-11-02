-- Milestone 2: agent profiles, jobs, agent sessions (D30). `jobs.state` is the only run status.

CREATE TABLE agent_profiles (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    provider        TEXT NOT NULL DEFAULT 'claude' CHECK (provider IN ('claude')),
    executable      TEXT NOT NULL,
    -- JSON array of [name, value] pairs.
    env_json        TEXT NOT NULL DEFAULT '[]',
    model           TEXT NULL,
    inactivity_secs INTEGER NOT NULL DEFAULT 600,
    instructions    TEXT NOT NULL,
    is_default      INTEGER NOT NULL DEFAULT 0,
    enabled         INTEGER NOT NULL DEFAULT 1,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE TABLE jobs (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind          TEXT NOT NULL DEFAULT 'agent' CHECK (kind IN ('agent')),
    state         TEXT NOT NULL CHECK (state IN
        ('queued','running','waiting_for_approval','succeeded','failed','cancelled','interrupted')),
    started_at    TEXT NULL,
    completed_at  TEXT NULL,
    exit_code     INTEGER NULL,
    error_kind    TEXT NULL,
    error_message TEXT NULL
);
CREATE INDEX jobs_project_started ON jobs(project_id, started_at);

CREATE TABLE agent_sessions (
    id                  TEXT PRIMARY KEY,
    job_id              TEXT NOT NULL UNIQUE REFERENCES jobs(id) ON DELETE CASCADE,
    project_id          TEXT NOT NULL,
    profile_id          TEXT NULL REFERENCES agent_profiles(id) ON DELETE SET NULL,
    mode                TEXT NOT NULL CHECK (mode IN ('plan','changes')),
    prompt              TEXT NOT NULL,
    provider_session_id TEXT NULL,
    resumed_from        TEXT NULL,
    log_path            TEXT NOT NULL,
    before_ref          TEXT NULL,
    after_ref           TEXT NULL,
    created_at          TEXT NOT NULL
);
CREATE INDEX agent_sessions_project_created ON agent_sessions(project_id, created_at);

-- projects.default_agent_id stays a plain column: SQLite cannot add a foreign key to an
-- existing column without a table rebuild. The reference is enforced in code
-- (deleting a profile clears every project's default_agent_id; see persistence/agents.rs).
