-- A repository can turn its webhook off and choose its poll interval (spec
-- 027). Every row so far keeps its webhook on, and an empty interval means
-- the default of five minutes.
ALTER TABLE forge_integrations ADD COLUMN webhook_enabled INTEGER NOT NULL DEFAULT 1
    CHECK (webhook_enabled IN (0, 1));
ALTER TABLE forge_integrations ADD COLUMN poll_interval_minutes INTEGER
    CHECK (poll_interval_minutes IS NULL OR poll_interval_minutes BETWEEN 1 AND 60);
