-- Retain only sessions owned by Ariadne work.
DELETE FROM agent_sessions WHERE goal_id IS NULL AND task_id IS NULL AND seat IS NULL;
ALTER TABLE agent_sessions DROP COLUMN title;
