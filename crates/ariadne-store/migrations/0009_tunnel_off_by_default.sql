-- 0001's own seed never changes, once shipped: it still writes
-- `tunnel_enabled = 1`. A new installation wants it off instead (027 rule
-- 11). `Store::open` writes `fresh_install_marker` before any migration
-- runs, the one time this database has never had a migration committed, so
-- this update applies only there: an existing database upgrading past 0008
-- never wrote that row, and its stored choice, on or off, is untouched.
UPDATE forge_settings
SET tunnel_enabled = 0
WHERE id = 1 AND EXISTS (SELECT 1 FROM fresh_install_marker WHERE id = 1);

DROP TABLE fresh_install_marker;
