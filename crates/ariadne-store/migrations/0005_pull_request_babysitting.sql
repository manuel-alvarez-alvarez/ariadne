-- Babysitting a request of the user's own by hand (032): a session of seat
-- `keeper` sits on one open request, no goal and no task, and keeps it the
-- way a task's `pr` column agent does.
ALTER TABLE pull_requests ADD COLUMN babysit_asked INTEGER NOT NULL DEFAULT 0
    CHECK (babysit_asked IN (0, 1));
ALTER TABLE pull_requests ADD COLUMN babysit_model TEXT;
ALTER TABLE pull_requests ADD COLUMN babysit_effort TEXT;
ALTER TABLE pull_requests ADD COLUMN babysit_skills TEXT NOT NULL DEFAULT '[]';

-- SQLite cannot widen a CHECK constraint in place: the table is rebuilt with
-- `keeper` added to the seats it allows, foreign keys off for the rebuild
-- alone (`Store::open` turns them back on right after every migration).
-- `legacy_alter_table` keeps the rename from silently rewriting every other
-- table's own foreign key clause to name `agent_sessions_old`: with it on,
-- those clauses stay spelled `agent_sessions`, which is the new table's name
-- again by the time this migration ends.
PRAGMA legacy_alter_table = ON;
ALTER TABLE agent_sessions RENAME TO agent_sessions_old;
CREATE TABLE agent_sessions (
    id                  TEXT PRIMARY KEY,
    goal_id             TEXT REFERENCES goals (id) ON DELETE CASCADE,
    task_id             TEXT REFERENCES tasks (id) ON DELETE CASCADE,
    seat                TEXT CHECK (seat IN ('orchestrator', 'agent', 'reviewer', 'keeper')),
    task_agent_id       TEXT REFERENCES task_agents (id) ON DELETE CASCADE,
    internal_session_id TEXT,
    worktree_path       TEXT,
    status              TEXT NOT NULL DEFAULT 'starting'
                        CHECK (status IN ('starting', 'running', 'idle', 'exited', 'failed')),
    last_activity_at    TEXT,
    created_at          TEXT NOT NULL,
    ended_at            TEXT,
    attention_reason    TEXT
                        CHECK (attention_reason IN ('waiting_permission', 'waiting_input',
                                                    'waiting_user', 'agent_error',
                                                    'disconnected', 'stalled', 'exhausted')),
    attention_since     TEXT,
    model               TEXT NOT NULL,
    effort              TEXT,
    context_used        INTEGER,
    context_size        INTEGER,
    launched_at         TEXT,
    launch_id           TEXT,
    title               TEXT,
    switched_from       TEXT REFERENCES agent_sessions (id) ON DELETE SET NULL,
    pull_request_id     TEXT REFERENCES pull_requests (id) ON DELETE CASCADE
);
INSERT INTO agent_sessions SELECT * FROM agent_sessions_old;
DROP TABLE agent_sessions_old;
CREATE INDEX idx_sessions_task ON agent_sessions (task_id);
CREATE INDEX idx_sessions_status ON agent_sessions (status);
CREATE INDEX idx_sessions_attention ON agent_sessions (attention_reason);
CREATE UNIQUE INDEX idx_sessions_switched_from ON agent_sessions (switched_from);
CREATE INDEX agent_sessions_pull_request ON agent_sessions (pull_request_id);
PRAGMA legacy_alter_table = OFF;
