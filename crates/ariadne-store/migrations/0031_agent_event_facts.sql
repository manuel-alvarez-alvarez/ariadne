-- The facts of an agent event that a trajectory read needs without its
-- deflated payload (spec 040): the lane, and a small JSON of the tool call,
-- the subagent, the question or the permission it is about. The store fills
-- both at insert. A row stored before this migration has NULL facts until
-- the store's one-time fill reads its payload.
ALTER TABLE agent_events ADD COLUMN lane TEXT;
ALTER TABLE agent_events ADD COLUMN facts TEXT;
CREATE INDEX idx_events_session_kind ON agent_events (session_id, kind, id);
-- The rows the fill has still to read, so that each batch finds them
-- without a scan of the table.
CREATE INDEX idx_events_unfilled ON agent_events (id) WHERE facts IS NULL;
-- The one-time fills the store has finished: a fill that has its row here
-- never runs again.
CREATE TABLE store_fills (
    name TEXT PRIMARY KEY,
    done_at TEXT NOT NULL
);
