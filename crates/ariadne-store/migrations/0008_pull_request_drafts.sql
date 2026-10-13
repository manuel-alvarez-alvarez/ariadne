-- A reply, a review or a resolve an agent wrote, held for the user's own
-- publish or discard where the repository supervises its posts (026): the
-- forge sees none of it until the user publishes it.
CREATE TABLE pull_request_drafts (
    id               TEXT PRIMARY KEY,
    pull_request_id  TEXT NOT NULL REFERENCES pull_requests (id) ON DELETE CASCADE,
    session_id       TEXT NOT NULL REFERENCES agent_sessions (id) ON DELETE CASCADE,
    kind             TEXT NOT NULL CHECK (kind IN ('reply', 'review', 'resolve')),
    payload          TEXT NOT NULL,
    state            TEXT NOT NULL DEFAULT 'pending'
                     CHECK (state IN ('pending', 'published', 'discarded')),
    feedback         TEXT,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);
CREATE INDEX idx_pull_request_drafts_pull_request ON pull_request_drafts (pull_request_id, state);
