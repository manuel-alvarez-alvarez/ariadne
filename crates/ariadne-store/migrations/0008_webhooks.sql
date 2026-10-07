ALTER TABLE forge_integrations ADD COLUMN webhook_id INTEGER;
ALTER TABLE forge_integrations ADD COLUMN webhook_secret TEXT;
ALTER TABLE forge_integrations ADD COLUMN webhook_url TEXT;
ALTER TABLE forge_integrations ADD COLUMN webhook_state TEXT NOT NULL DEFAULT 'polling'
    CHECK (webhook_state IN ('live', 'polling', 'failed'));
ALTER TABLE forge_integrations ADD COLUMN webhook_error TEXT;
ALTER TABLE forge_integrations ADD COLUMN webhook_last_delivery_at TEXT;
