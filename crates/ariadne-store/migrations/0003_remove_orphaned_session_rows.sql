-- Finish what 0002 began. Migrations run with foreign keys off, so its
-- DELETE fired none of the ON DELETE actions on the tables that reference
-- agent_sessions and left the removed sessions' rows behind, which the
-- foreign key check after the migrations refuses. Apply each action by hand.
DELETE FROM agent_events
    WHERE session_id IS NOT NULL
      AND session_id NOT IN (SELECT id FROM agent_sessions);
DELETE FROM session_usage
    WHERE session_id NOT IN (SELECT id FROM agent_sessions);
DELETE FROM agent_requests
    WHERE session_id NOT IN (SELECT id FROM agent_sessions);
UPDATE messages SET from_session = NULL
    WHERE from_session IS NOT NULL
      AND from_session NOT IN (SELECT id FROM agent_sessions);
UPDATE agent_sessions SET switched_from = NULL
    WHERE switched_from IS NOT NULL
      AND switched_from NOT IN (SELECT id FROM agent_sessions);
