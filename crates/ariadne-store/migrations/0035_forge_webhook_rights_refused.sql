-- The last hook registration of a repository was refused for rights: the
-- account lacks admin rights or the token lacks the hook scope (spec 027).
-- Each timer fetch retries the registration while it holds.
ALTER TABLE forge_integrations ADD COLUMN webhook_rights_refused INTEGER NOT NULL DEFAULT 0
    CHECK (webhook_rights_refused IN (0, 1));
