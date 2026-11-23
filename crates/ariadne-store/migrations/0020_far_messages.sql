-- Keep the sender's message ID to make retries and event replay idempotent.
ALTER TABLE messages ADD COLUMN relay_id TEXT;
CREATE UNIQUE INDEX idx_messages_relay ON messages (task_id, relay_id);
-- A host's acceptance is separate from delivery to its agent.
ALTER TABLE messages ADD COLUMN relayed_at TEXT;
