-- Chat attachments: the files a run's prompt carried. Table only (no rebuild of existing tables).
-- `stored_path` and `converted_path` are relative to the conversation folder
-- `<data>/attachments/<conversation-id>/`. `removed` is set when pruning deleted the files.
CREATE TABLE run_attachments (
    session_id     TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
    ordinal        INTEGER NOT NULL,
    name           TEXT NOT NULL,
    stored_path    TEXT NOT NULL,
    kind           TEXT NOT NULL CHECK (kind IN ('image','pdf','text','docx')),
    size_bytes     INTEGER NOT NULL,
    converted_path TEXT NULL,
    removed        INTEGER NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL,
    PRIMARY KEY (session_id, ordinal)
);
