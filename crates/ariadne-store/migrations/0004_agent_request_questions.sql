-- A request can carry the whole question an agent asks the user (031), and
-- keeps how it closed and what the user answered.
--
-- `question` is the JSON of `AgentQuestion`, NULL for a request that is a
-- hint alone. `resolution` is `answered`, `withdrawn` or `expired` once
-- `answered_at` is set, and `claimed` or `claimed_expired` while an answer
-- is on its way to the agent; a row closed before this migration keeps NULL.
-- `answer` is the JSON of the recorded answer: `{"option_id"?, "text"}`.
ALTER TABLE agent_requests ADD COLUMN question TEXT;
ALTER TABLE agent_requests ADD COLUMN resolution TEXT;
ALTER TABLE agent_requests ADD COLUMN answer TEXT;
