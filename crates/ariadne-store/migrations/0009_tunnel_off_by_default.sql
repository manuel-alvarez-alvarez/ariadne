-- 0001's own seed never changes, once shipped: it still writes
-- `tunnel_enabled = 1`. A new installation wants it off instead (027 rule
-- 11), and a migration is the one place that runs exactly once per
-- database, atomically with its own completion record, so no run killed
-- midway can leave a database stuck on the old default.
UPDATE forge_settings SET tunnel_enabled = 0 WHERE id = 1;
