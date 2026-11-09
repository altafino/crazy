-- "What's new" toast (settings/about UX D9, R1). app_settings is key/value, so there is no
-- column to add. A database that already holds projects or settings came from 0.2.0 and
-- gets that version stored; a fresh install gets nothing and the app stores the running
-- version on first start.
INSERT OR IGNORE INTO app_settings (key, value)
SELECT 'last_seen_version', '"0.2.0"'
WHERE EXISTS (SELECT 1 FROM projects) OR EXISTS (SELECT 1 FROM app_settings);
