-- Milestone 3: previews are jobs (M3-D6) and runs may record a Git checkpoint ref (M3-D1).
--
-- jobs.kind gains 'preview', which needs a table rebuild (SQLite cannot alter a CHECK).
-- The migration runner runs this with foreign keys OFF (outside the transaction) and checks
-- PRAGMA foreign_key_check before committing. Rebuild order matters: create jobs_new, copy,
-- drop jobs, then rename jobs_new. Renaming the old table first would make SQLite rewrite
-- agent_sessions' REFERENCES jobs(id) to point at the renamed copy.

CREATE TABLE jobs_new (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind          TEXT NOT NULL DEFAULT 'agent' CHECK (kind IN ('agent', 'preview')),
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

-- refs/studio/checkpoints/<run id> in the project's Git repo, when one was recorded.
ALTER TABLE agent_sessions ADD COLUMN checkpoint_ref TEXT NULL;
