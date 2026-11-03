-- Milestone 5: the last remote-sync result per project (R4). One row, replaced by each
-- fetch/pull/merge/push/create. The message is stored already redacted. Table only: no
-- rebuild of existing tables.
CREATE TABLE git_sync (
    project_id  TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    operation   TEXT NOT NULL CHECK (operation IN ('fetch','pull','merge','push','create')),
    ok          INTEGER NOT NULL,
    message     TEXT,
    finished_at TEXT NOT NULL
);
