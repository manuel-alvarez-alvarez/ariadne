-- Expand the task lifecycle. Run with foreign keys disabled before the transaction.

ALTER TABLE goals ADD COLUMN workflow TEXT REFERENCES workflows(name);

ALTER TABLE repositories ADD COLUMN default_workflow TEXT REFERENCES workflows(name);

ALTER TABLE tasks ADD COLUMN step TEXT;

CREATE TABLE goal_steps (
    goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    id TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    skills TEXT NOT NULL CHECK (json_valid(skills)),
    rank TEXT,
    gate TEXT,
    PRIMARY KEY (goal_id, ordinal),
    UNIQUE (goal_id, id)
);

CREATE TABLE task_agents_stepped (
    id         TEXT PRIMARY KEY,
    task_id    TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    seat       TEXT NOT NULL CHECK (seat IN ('author', 'agent', 'reviewer')),
    ordinal    INTEGER NOT NULL,
    model      TEXT NOT NULL,
    effort     TEXT,
    -- What this agent is told beyond the task itself, where the orchestrator
    -- has something to add. NULL = the task is the whole of it.
    brief      TEXT,
    step       TEXT,
    UNIQUE (task_id, seat, ordinal)
);

INSERT INTO task_agents_stepped (id, task_id, seat, ordinal, model, effort, brief) SELECT id, task_id, seat, ordinal, model, effort, brief FROM task_agents;

DROP TABLE task_agents;

ALTER TABLE task_agents_stepped RENAME TO task_agents;

CREATE INDEX idx_task_agents_task ON task_agents (task_id);

CREATE TABLE agent_sessions_stepped (
    id                  TEXT PRIMARY KEY,       -- == ARIADNE_SESSION_ID env of the agent
    goal_id             TEXT REFERENCES goals (id) ON DELETE CASCADE,
    task_id             TEXT REFERENCES tasks (id) ON DELETE CASCADE,  -- NULL = orchestrator or loose session
    seat                TEXT CHECK (seat IN ('orchestrator', 'author', 'agent', 'reviewer')),
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
    title               TEXT,
    -- The session this one replaced on its seat, on another pin, in a new
    -- conversation. NULL for every session no switch started.
    switched_from       TEXT REFERENCES agent_sessions (id) ON DELETE SET NULL,
    pull_request_id     TEXT REFERENCES pull_requests(id) ON DELETE CASCADE
);

INSERT INTO agent_sessions_stepped (id, goal_id, task_id, seat, task_agent_id, internal_session_id, worktree_path, status, last_activity_at, created_at, ended_at, attention_reason, attention_since, model, effort, context_used, context_size, launched_at, launch_id, title, switched_from, pull_request_id) SELECT id, goal_id, task_id, seat, task_agent_id, internal_session_id, worktree_path, status, last_activity_at, created_at, ended_at, attention_reason, attention_since, model, effort, context_used, context_size, launched_at, launch_id, title, switched_from, pull_request_id FROM agent_sessions;

DROP TABLE agent_sessions;

ALTER TABLE agent_sessions_stepped RENAME TO agent_sessions;

CREATE INDEX idx_sessions_task ON agent_sessions (task_id);

CREATE INDEX idx_sessions_status ON agent_sessions (status);

CREATE INDEX idx_sessions_attention ON agent_sessions (attention_reason);

CREATE UNIQUE INDEX idx_sessions_switched_from ON agent_sessions (switched_from);

CREATE INDEX agent_sessions_pull_request ON agent_sessions (pull_request_id);

CREATE TABLE messages_stepped (
    id            TEXT PRIMARY KEY,
    goal_id       TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    -- NULL for a message about the goal rather than about one task.
    task_id       TEXT REFERENCES tasks (id) ON DELETE CASCADE,
    kind          TEXT NOT NULL
                  CHECK (kind IN ('message', 'review_request',
                                  'approve', 'request_changes')),
    from_actor    TEXT NOT NULL
                  CHECK (from_actor IN ('orchestrator', 'author', 'agent', 'reviewer',
                                        'daemon', 'user')),
    from_agent_id TEXT REFERENCES task_agents (id) ON DELETE CASCADE,
    -- The session that actually sent it, for the record; a message outlives it.
    from_session  TEXT REFERENCES agent_sessions (id) ON DELETE SET NULL,
    to_actor      TEXT NOT NULL
                  CHECK (to_actor IN ('orchestrator', 'author', 'agent', 'reviewer',
                                      'daemon', 'user')),
    to_agent_id   TEXT REFERENCES task_agents (id) ON DELETE CASCADE,
    body          TEXT NOT NULL,
    -- When it was handed to the recipient's agent. NULL while it is still waiting.
    delivered_at  TEXT,
    created_at    TEXT NOT NULL
);

INSERT INTO messages_stepped (id, goal_id, task_id, kind, from_actor, from_agent_id, from_session, to_actor, to_agent_id, body, delivered_at, created_at) SELECT id, goal_id, task_id, kind, from_actor, from_agent_id, from_session, to_actor, to_agent_id, body, delivered_at, created_at FROM messages;

DROP TABLE messages;

ALTER TABLE messages_stepped RENAME TO messages;

CREATE INDEX idx_messages_task ON messages (task_id, id);

CREATE INDEX idx_messages_goal ON messages (goal_id, id);

CREATE TABLE task_transitions_stepped (
    id          TEXT PRIMARY KEY,
    task_id     TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    from_status TEXT NOT NULL,
    to_status   TEXT NOT NULL,
    actor       TEXT NOT NULL
                CHECK (actor IN ('orchestrator', 'author', 'agent', 'reviewer', 'daemon', 'user')),
    reason      TEXT,
    created_at  TEXT NOT NULL,
    from_step   TEXT,
    to_step     TEXT
);

INSERT INTO task_transitions_stepped (id, task_id, from_status, to_status, actor, reason, created_at) SELECT id, task_id, from_status, to_status, actor, reason, created_at FROM task_transitions;

DROP TABLE task_transitions;

ALTER TABLE task_transitions_stepped RENAME TO task_transitions;

CREATE INDEX idx_transitions_task ON task_transitions (task_id, id);

ALTER TABLE task_transitions ADD COLUMN step_briefed_at TEXT;
