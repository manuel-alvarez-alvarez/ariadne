-- 0001's own seed never changes, once shipped: it still writes
-- `tunnel_enabled = 1`. New installations want it off instead (027 rule
-- 11), and a migration runs exactly once per database, ever — this moves
-- every database, fresh or upgrading, to the new default alike.
UPDATE forge_settings SET tunnel_enabled = 0 WHERE id = 1;
