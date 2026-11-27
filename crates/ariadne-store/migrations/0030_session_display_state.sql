-- What an ACP agent last said about its session, for display (021 rule 24):
-- its modes, config options, title, cost and commands, as one JSON object.
-- `display_launch_id` names the launch that wrote it: the first write of a
-- later launch starts from an empty state, so no earlier launch's value
-- survives into it.
ALTER TABLE agent_sessions ADD COLUMN display_launch_id TEXT;
ALTER TABLE agent_sessions ADD COLUMN display_state TEXT;
