-- Goal-free task execution. Store::open disables foreign keys during rebuilds.

CREATE TABLE tasks_new (
    id            TEXT PRIMARY KEY,
    goal_id       TEXT NOT NULL,
    repo_id       TEXT NOT NULL REFERENCES repositories (id),
    title         TEXT NOT NULL,
    description   TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'pending'
                  CHECK (status IN ('pending', 'ready', 'in_progress', 'finished',
                                    'cancelled', 'failed')),
    branch        TEXT NOT NULL,
    worktree_path TEXT,
    stalled       INTEGER NOT NULL DEFAULT 0,
    merge_commit  TEXT,
    -- The pull or merge request the task's `pr` column opened, where it did.
    pr_url        TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    -- The column the task is in while it is `in_progress`.
    step          TEXT,
    host TEXT,
    far_goal_id TEXT,
    far_task_id TEXT,
    host_unreachable INTEGER NOT NULL DEFAULT 0,
    origin_task_id TEXT
);

INSERT INTO tasks_new SELECT * FROM tasks;

DROP TABLE tasks;

ALTER TABLE tasks_new RENAME TO tasks;

CREATE INDEX idx_tasks_goal ON tasks (goal_id);

CREATE INDEX idx_tasks_status ON tasks (status);

CREATE TRIGGER attention_task_changed AFTER UPDATE OF status ON tasks
WHEN OLD.status IS NOT NEW.status
BEGIN
    INSERT INTO attention_source_versions (kind, source_id, version)
    VALUES ('task', NEW.id, 1)
    ON CONFLICT(kind, source_id) DO UPDATE SET version = version + 1;
END;

CREATE INDEX idx_tasks_host ON tasks (host) WHERE host IS NOT NULL;

CREATE UNIQUE INDEX idx_tasks_origin ON tasks (origin_task_id)
    WHERE origin_task_id IS NOT NULL;

CREATE TABLE agent_sessions_new (
    id                  TEXT PRIMARY KEY,       -- == ARIADNE_SESSION_ID env of the agent
    goal_id             TEXT,
    task_id             TEXT REFERENCES tasks (id) ON DELETE CASCADE,  -- NULL = orchestrator or loose session
    seat                TEXT CHECK (seat IN ('orchestrator', 'agent', 'reviewer', 'keeper')),
    -- Which staffed agent this session runs; NULL for an orchestrator,
    -- which is the one agent type Ariadne defines rather than one a task
    -- staffs.
    task_agent_id       TEXT REFERENCES task_agents (id) ON DELETE CASCADE,
    internal_session_id TEXT,                   -- the ACP agent's own session id
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
    -- Copied off the pin the session's seat carries, beside its model.
    effort              TEXT,
    -- The latest context-window position an ACP agent reported. Both are
    -- NULL until it has reported one; a zero would claim an empty window.
    context_used        INTEGER,
    context_size        INTEGER,
    launched_at         TEXT,
    launch_id           TEXT,                   -- == ARIADNE_LAUNCH_ID env of that run
    -- A loose session's own title: the first prompt of the conversation it
    -- resumed, or the first one typed into it. NULL on a task's or a goal's
    -- session, which goes by its work's title.
    switched_from       TEXT REFERENCES agent_sessions (id) ON DELETE SET NULL,
    pull_request_id     TEXT REFERENCES pull_requests (id) ON DELETE CASCADE,
    place TEXT CHECK (place IN ('none', 'tmux', 'herdr'))
);

INSERT INTO agent_sessions_new SELECT * FROM agent_sessions;

DROP TABLE agent_sessions;

ALTER TABLE agent_sessions_new RENAME TO agent_sessions;

CREATE INDEX idx_sessions_task ON agent_sessions (task_id);

CREATE INDEX idx_sessions_status ON agent_sessions (status);

CREATE INDEX idx_sessions_attention ON agent_sessions (attention_reason);

CREATE UNIQUE INDEX idx_sessions_switched_from ON agent_sessions (switched_from);

CREATE INDEX agent_sessions_pull_request ON agent_sessions (pull_request_id);

CREATE TRIGGER attention_session_changed AFTER UPDATE OF attention_reason ON agent_sessions
WHEN OLD.attention_reason IS NOT NEW.attention_reason
BEGIN
    INSERT INTO attention_source_versions (kind, source_id, version)
    VALUES ('session', NEW.id, 1)
    ON CONFLICT(kind, source_id) DO UPDATE SET version = version + 1;
END;

CREATE TABLE messages_new (
    id            TEXT PRIMARY KEY,
    goal_id       TEXT NOT NULL,
    -- NULL for a message about the goal rather than about one task.
    task_id       TEXT REFERENCES tasks (id) ON DELETE CASCADE,
    kind          TEXT NOT NULL CHECK (kind IN ('message')),
    from_actor    TEXT NOT NULL
                  CHECK (from_actor IN ('orchestrator', 'agent', 'daemon', 'user')),
    from_agent_id TEXT REFERENCES task_agents (id) ON DELETE CASCADE,
    -- The session that actually sent it, for the record; a message outlives it.
    from_session  TEXT REFERENCES agent_sessions (id) ON DELETE SET NULL,
    to_actor      TEXT NOT NULL
                  CHECK (to_actor IN ('orchestrator', 'agent', 'daemon', 'user')),
    to_agent_id   TEXT REFERENCES task_agents (id) ON DELETE CASCADE,
    body          TEXT NOT NULL,
    -- When it was handed to the recipient's agent. NULL while it is still waiting.
    delivered_at  TEXT,
    created_at    TEXT NOT NULL,
    relay_id TEXT,
    relayed_at TEXT
);

INSERT INTO messages_new SELECT * FROM messages;

DROP TABLE messages;

ALTER TABLE messages_new RENAME TO messages;

CREATE INDEX idx_messages_task ON messages (task_id, id);

CREATE INDEX idx_messages_goal ON messages (goal_id, id);

CREATE UNIQUE INDEX idx_messages_relay ON messages (task_id, relay_id);

CREATE TABLE daemon_identity (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    id TEXT NOT NULL UNIQUE
);
CREATE TABLE remote_task_contexts (
    task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    source_daemon_id TEXT NOT NULL,
    source_goal_id TEXT NOT NULL,
    source_title TEXT NOT NULL,
    source_description TEXT NOT NULL,
    workflow TEXT NOT NULL,
    request TEXT NOT NULL CHECK (json_valid(request)),
    revision INTEGER NOT NULL DEFAULT 1,
    authorized_revision INTEGER,
    CHECK (revision > 0)
);
CREATE TABLE remote_task_steps (
    task_id TEXT NOT NULL REFERENCES remote_task_contexts(task_id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    id TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    skills TEXT NOT NULL CHECK (json_valid(skills)),
    ranks TEXT NOT NULL CHECK (json_valid(ranks)),
    gate TEXT,
    PRIMARY KEY (task_id, ordinal),
    UNIQUE (task_id, id)
);
-- Preserve goal deletion semantics for local and legacy tasks.
CREATE TRIGGER delete_goal_execution AFTER DELETE ON goals BEGIN
    DELETE FROM messages WHERE goal_id = OLD.id AND
        (task_id IS NULL OR task_id NOT IN (SELECT task_id FROM remote_task_contexts));
    DELETE FROM agent_sessions WHERE goal_id = OLD.id AND
        (task_id IS NULL OR task_id NOT IN (SELECT task_id FROM remote_task_contexts));
    DELETE FROM tasks WHERE goal_id = OLD.id AND
        id NOT IN (SELECT task_id FROM remote_task_contexts);
END;

-- Keep reference validation after allowing remote source-goal references.
CREATE TRIGGER task_goal_reference BEFORE INSERT ON tasks
WHEN NOT EXISTS (SELECT 1 FROM goals WHERE id = NEW.goal_id)
 AND NOT EXISTS (SELECT 1 FROM remote_task_contexts WHERE task_id = NEW.id AND source_goal_id = NEW.goal_id)
BEGIN SELECT RAISE(ABORT, 'task needs a local goal or saved remote context'); END;
CREATE TRIGGER session_goal_reference BEFORE INSERT ON agent_sessions
WHEN NEW.goal_id IS NOT NULL
 AND NOT EXISTS (SELECT 1 FROM goals WHERE id = NEW.goal_id)
 AND NOT EXISTS (SELECT 1 FROM remote_task_contexts WHERE task_id = NEW.task_id AND source_goal_id = NEW.goal_id)
BEGIN SELECT RAISE(ABORT, 'session needs a local goal or saved remote context'); END;
CREATE TRIGGER message_goal_reference BEFORE INSERT ON messages
WHEN NOT EXISTS (SELECT 1 FROM goals WHERE id = NEW.goal_id)
 AND NOT EXISTS (SELECT 1 FROM remote_task_contexts WHERE task_id = NEW.task_id AND source_goal_id = NEW.goal_id)
BEGIN SELECT RAISE(ABORT, 'message needs a local goal or saved remote context'); END;
