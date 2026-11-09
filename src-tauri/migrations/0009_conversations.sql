-- Conversations continue by default (journey UX D6, D14, R1, R2). `conversation_start` marks a
-- run that began a new conversation on purpose ('new': New conversation or Plan Start) or
-- because the agent profile changed ('profile'); the resume target is never searched past it.
-- `resume_fell_back` marks a resumed run whose `--resume` failed before Claude started and was
-- respawned without it. Columns only.
ALTER TABLE agent_sessions ADD COLUMN conversation_start TEXT NULL;
ALTER TABLE agent_sessions ADD COLUMN resume_fell_back INTEGER NOT NULL DEFAULT 0;
