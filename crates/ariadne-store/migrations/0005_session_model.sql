-- The model a session was actually launched with.
--
-- Until now the model lived only on the profile, so the only answer to "what
-- is this session running?" was a profile lookup — which starts lying the
-- moment the profile is edited, and says nothing about a session launched
-- before the edit. The launcher snapshots the model it hands the adapter onto
-- the row instead, on every spawn and every resume.
--
-- NULL means the launch asked for no model at all: the agent CLI's own
-- default, which is also what every session predating this column recorded.

ALTER TABLE agent_sessions ADD COLUMN model TEXT;
