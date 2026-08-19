-- Why a live agent session needs the user's attention.
--
-- Orthogonal to `status`: a session blocked on a permission prompt is still
-- `running`, it just cannot make progress until someone looks at it. Keeping
-- the two columns apart means detectors can raise and clear attention without
-- touching the lifecycle status the scheduler reasons about.
--
-- `attention_since` is when the current reason was first raised: re-raising
-- the same reason leaves it alone, so the UI can show how long an agent has
-- been stuck.

ALTER TABLE agent_sessions ADD COLUMN attention_reason TEXT
    CHECK (attention_reason IN ('waiting_permission', 'waiting_input',
                                'agent_error', 'disconnected', 'stalled'));
ALTER TABLE agent_sessions ADD COLUMN attention_since TEXT;

CREATE INDEX idx_sessions_attention ON agent_sessions (attention_reason);
