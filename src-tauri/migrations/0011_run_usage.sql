-- Token usage per run, for the usage summary on Settings › AI Agents. Read once from the
-- run's raw log after the run ends; NULL input_tokens means not read yet, so runs from
-- before this migration are filled in the first time their profile's usage is shown.
-- Columns only (no rebuild).
ALTER TABLE agent_sessions ADD COLUMN input_tokens INTEGER NULL;
ALTER TABLE agent_sessions ADD COLUMN output_tokens INTEGER NULL;
ALTER TABLE agent_sessions ADD COLUMN cache_read_tokens INTEGER NULL;
ALTER TABLE agent_sessions ADD COLUMN cache_write_tokens INTEGER NULL;
ALTER TABLE agent_sessions ADD COLUMN cost_usd REAL NULL;
