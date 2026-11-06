-- Exports for other systems (F22, eng review D8/D14). An export is a `build` job in the
-- project's Build slot, marked by `export_release_id`; `export_targets` is the comma-separated
-- list of targets asked for ("linux,windows") and `export_results` the JSON per-target outcome
-- written once when the job ends. Build status ignores rows with `export_release_id`. Columns
-- only (no rebuild of `jobs`).
ALTER TABLE jobs ADD COLUMN export_release_id TEXT NULL;
ALTER TABLE jobs ADD COLUMN export_targets TEXT NULL;
ALTER TABLE jobs ADD COLUMN export_results TEXT NULL;
