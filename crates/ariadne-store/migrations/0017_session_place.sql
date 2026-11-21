ALTER TABLE agent_sessions ADD COLUMN place TEXT CHECK (place IN ('none', 'tmux', 'herdr'));

CREATE TABLE relay_inputs (
    session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
    launch_id TEXT NOT NULL,
    seq INTEGER NOT NULL CHECK (seq > 0),
    data TEXT NOT NULL,
    PRIMARY KEY (session_id, launch_id, seq)
);

CREATE TABLE relay_outputs (
    session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
    launch_id TEXT NOT NULL,
    seq INTEGER NOT NULL CHECK (seq > 0),
    frame TEXT NOT NULL,
    processed INTEGER NOT NULL DEFAULT 0 CHECK (processed IN (0, 1)),
    PRIMARY KEY (session_id, launch_id, seq)
);

ALTER TABLE agent_events ADD COLUMN relay_key TEXT;
CREATE UNIQUE INDEX idx_agent_events_relay_key ON agent_events(relay_key);
