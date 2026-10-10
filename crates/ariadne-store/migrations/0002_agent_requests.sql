CREATE TABLE agent_requests (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES agent_sessions (id) ON DELETE CASCADE,
    summary     TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    answered_at TEXT
);
CREATE INDEX idx_agent_requests_pending ON agent_requests (session_id, answered_at);
CREATE UNIQUE INDEX idx_agent_requests_pending_summary
    ON agent_requests (session_id, summary) WHERE answered_at IS NULL;
