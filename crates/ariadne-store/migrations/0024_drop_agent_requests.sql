-- An agent asks the user with its native question tool now (021), and the
-- `wait_for_user` request is gone. Its table and both indexes go with it.
-- No other table refers to `agent_requests`, so no other row changes.
DROP INDEX IF EXISTS idx_agent_requests_pending_summary;
DROP INDEX IF EXISTS idx_agent_requests_pending;
DROP TABLE agent_requests;
