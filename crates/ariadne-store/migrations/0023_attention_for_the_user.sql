-- no-transaction
-- `agent_sessions.attention_reason` gains `waiting_user`.
--
-- The five reasons a session could carry were all things that had happened to
-- the agent: a dialog it put up, an error it reported, a pane that went away,
-- a silence. What the daemon had no word for was the other kind — something
-- addressed to the user that no agent can do for them, a message written to
-- them or a published request that is theirs to merge — so it borrowed
-- `waiting_input`, which an agent's own next event takes straight back down.
-- Under its own name it is nobody's to clear but the user's.
--
-- A CHECK constraint, and SQLite cannot alter one in place: the table is
-- rebuilt the documented way — foreign keys off, one explicit transaction,
-- keys back on — which is also why this file opts out of sqlx's own
-- transaction above (`PRAGMA foreign_keys` is a no-op inside one). Dropping a
-- table with keys on would cascade its dependants away with it.

PRAGMA foreign_keys = OFF;

BEGIN;

CREATE TABLE agent_sessions_new (
    id                  TEXT PRIMARY KEY,       -- == ARIADNE_SESSION_ID env of the agent
    goal_id             TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    task_id             TEXT REFERENCES tasks (id) ON DELETE CASCADE,  -- NULL = planner
    role                TEXT NOT NULL
                        CHECK (role IN ('planner', 'engineer', 'reviewer', 'integrator')),
    profile_id          TEXT NOT NULL REFERENCES profiles (id),
    agent_kind          TEXT NOT NULL CHECK (agent_kind IN ('claude_code', 'codex', 'opencode')),
    internal_session_id TEXT,                   -- claude session uuid / codex thread_id / opencode session id
    tmux_session        TEXT NOT NULL,
    worktree_path       TEXT,
    review_round        INTEGER,
    status              TEXT NOT NULL DEFAULT 'starting'
                        CHECK (status IN ('starting', 'running', 'idle', 'exited', 'failed')),
    last_activity_at    TEXT,
    created_at          TEXT NOT NULL,
    ended_at            TEXT,
    attention_reason    TEXT
                        CHECK (attention_reason IN ('waiting_permission', 'waiting_input',
                                                    'waiting_user', 'agent_error',
                                                    'disconnected', 'stalled')),
    attention_since     TEXT,
    model               TEXT,
    launched_at         TEXT
);

INSERT INTO agent_sessions_new (id, goal_id, task_id, role, profile_id, agent_kind,
                                internal_session_id, tmux_session, worktree_path, review_round,
                                status, last_activity_at, created_at, ended_at,
                                attention_reason, attention_since, model, launched_at)
SELECT id, goal_id, task_id, role, profile_id, agent_kind,
       internal_session_id, tmux_session, worktree_path, review_round,
       status, last_activity_at, created_at, ended_at,
       attention_reason, attention_since, model, launched_at
FROM agent_sessions;

DROP TABLE agent_sessions;
ALTER TABLE agent_sessions_new RENAME TO agent_sessions;
CREATE INDEX idx_sessions_task ON agent_sessions (task_id);
CREATE INDEX idx_sessions_status ON agent_sessions (status);
CREATE INDEX idx_sessions_attention ON agent_sessions (attention_reason);

COMMIT;

PRAGMA foreign_keys = ON;
