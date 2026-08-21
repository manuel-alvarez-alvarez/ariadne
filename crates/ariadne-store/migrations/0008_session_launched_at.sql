-- When this session's agent process was last started.
--
-- Not `created_at`, which is the row's, and not `last_activity_at`, which the
-- agent moves: a session is relaunched under its own id on every resume, so
-- the only way to ask "has this run of the agent started its turn yet?" is to
-- know when this run began and compare it against what the session has
-- reported since.
--
-- NULL for sessions that predate the column, and for rows created but never
-- launched: nothing is concluded from a launch that is not recorded.

ALTER TABLE agent_sessions ADD COLUMN launched_at TEXT;
