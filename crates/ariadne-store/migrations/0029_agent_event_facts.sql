-- The facts of an agent event that a trajectory read needs without its
-- deflated payload (spec 040): the lane, and a small JSON of the tool call,
-- the subagent, the question or the permission it is about. The store fills
-- both at insert. A row stored before this migration has NULL facts until
-- the store opens and fills them from the payload.
ALTER TABLE agent_events ADD COLUMN lane TEXT;
ALTER TABLE agent_events ADD COLUMN facts TEXT;
CREATE INDEX idx_events_session_kind ON agent_events (session_id, kind, id);
