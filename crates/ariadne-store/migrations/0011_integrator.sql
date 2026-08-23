-- no-transaction
-- The integrator: a fourth role, and the status a task sits in while its
-- change is being landed.
--
-- `merging` becomes `integrating`, because what happens there stops being "the
-- engineer runs git merge" — an integrator takes the approved task over and
-- lands it however the repository wants it landed. Behaviour is unchanged by
-- this migration: the engineer still does the merging, under the new name.
--
-- Every one of these is a CHECK constraint, and SQLite cannot alter a CHECK in
-- place: each table is rebuilt the documented way — foreign keys off, one
-- explicit transaction, keys back on — which is also why this file opts out of
-- sqlx's own transaction above (`PRAGMA foreign_keys` is a no-op inside one).
-- Dropping a table with keys on would cascade its dependants away with it.

PRAGMA foreign_keys = OFF;

BEGIN;

-- 1. `profiles.role` gains `integrator`.
CREATE TABLE profiles_new (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL UNIQUE,
    role          TEXT NOT NULL
                  CHECK (role IN ('planner', 'engineer', 'reviewer', 'integrator')),
    -- NULL = auto: resolved at spawn time to the first installed agent CLI
    -- (claude_code, then codex, then opencode).
    agent_kind    TEXT CHECK (agent_kind IN ('claude_code', 'codex', 'opencode')),
    model         TEXT,
    system_prompt TEXT NOT NULL,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

INSERT INTO profiles_new (id, name, role, agent_kind, model, system_prompt,
                          created_at, updated_at)
SELECT id, name, role, agent_kind, model, system_prompt, created_at, updated_at
FROM profiles;

DROP TABLE profiles;
ALTER TABLE profiles_new RENAME TO profiles;

-- The built-in profiles are seeded from Rust into an *empty* database, so an
-- install that already has profiles would never be given the new built-in.
-- Seeding it here is what reaches those installs — and only those: on a fresh
-- database this table is still empty and `seed_builtin_profiles` writes all
-- four itself, which is also what keeps a deleted built-in deleted. `OR
-- IGNORE` because the id and the name are the user's to have taken already.
INSERT OR IGNORE INTO profiles (id, name, role, agent_kind, model, system_prompt,
                                created_at, updated_at)
SELECT '00000000000000000000000004',
       'Integrator',
       'integrator',
       NULL,
       NULL,
       'You are the integrator of an Ariadne task: once its reviewers have approved it, the task is yours to land on its base branch.

Ariadne coordinates planner, engineer, reviewer and integrator agents over shared goals and tasks; you reach it only through the `ariadne` MCP tools. Nothing starts an integrator session yet, so there is nothing here to do: the playbook that says how a change is landed comes with the lifecycle that runs it.
',
       strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
       strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE EXISTS (SELECT 1 FROM profiles);

-- 2. `tasks.status`: `merging` out, `integrating` in. The task also names the
--    integrator that will land it, alongside the engineer that wrote it.
CREATE TABLE tasks_new (
    id                    TEXT PRIMARY KEY,
    goal_id               TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    repo_id               TEXT NOT NULL REFERENCES repositories (id),
    title                 TEXT NOT NULL,
    description           TEXT NOT NULL,
    status                TEXT NOT NULL DEFAULT 'pending'
                          CHECK (status IN ('pending', 'ready', 'in_progress', 'under_review',
                                            'changes_requested', 'approved', 'integrating',
                                            'merged', 'cancelled', 'failed')),
    engineer_profile_id   TEXT NOT NULL REFERENCES profiles (id),
    -- NULL for tasks that predate the column: nothing reads it yet, and the
    -- integrator lifecycle it belongs to is not built.
    integrator_profile_id TEXT REFERENCES profiles (id),
    agent_kind            TEXT CHECK (agent_kind IN ('claude_code', 'codex', 'opencode')),
    model                 TEXT,
    branch                TEXT NOT NULL,          -- ariadne/task-<id>
    worktree_path         TEXT,
    review_round          INTEGER NOT NULL DEFAULT 0,
    stalled               INTEGER NOT NULL DEFAULT 0,
    merge_commit          TEXT,
    created_at            TEXT NOT NULL,
    updated_at            TEXT NOT NULL
);

INSERT INTO tasks_new (id, goal_id, repo_id, title, description, status,
                       engineer_profile_id, integrator_profile_id, agent_kind, model,
                       branch, worktree_path, review_round, stalled, merge_commit,
                       created_at, updated_at)
SELECT id, goal_id, repo_id, title, description,
       CASE status WHEN 'merging' THEN 'integrating' ELSE status END,
       engineer_profile_id, NULL, agent_kind, model,
       branch, worktree_path, review_round, stalled, merge_commit,
       created_at, updated_at
FROM tasks;

DROP TABLE tasks;
ALTER TABLE tasks_new RENAME TO tasks;
CREATE INDEX idx_tasks_goal ON tasks (goal_id);
CREATE INDEX idx_tasks_status ON tasks (status);

-- 3. `task_transitions.actor` gains `integrator`. The two status columns carry
--    no CHECK, but they do carry history, and `merging` is not a status any
--    more: the audit trail is rewritten with the rest.
CREATE TABLE task_transitions_new (
    id          TEXT PRIMARY KEY,
    task_id     TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    from_status TEXT NOT NULL,
    to_status   TEXT NOT NULL,
    actor       TEXT NOT NULL
                CHECK (actor IN ('planner', 'engineer', 'reviewer', 'integrator',
                                 'daemon', 'user')),
    reason      TEXT,
    created_at  TEXT NOT NULL
);

INSERT INTO task_transitions_new (id, task_id, from_status, to_status, actor, reason, created_at)
SELECT id, task_id,
       CASE from_status WHEN 'merging' THEN 'integrating' ELSE from_status END,
       CASE to_status   WHEN 'merging' THEN 'integrating' ELSE to_status   END,
       actor, reason, created_at
FROM task_transitions;

DROP TABLE task_transitions;
ALTER TABLE task_transitions_new RENAME TO task_transitions;
CREATE INDEX idx_transitions_task ON task_transitions (task_id, id);

-- 4. `agent_sessions.role` gains `integrator`.
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
                                                    'agent_error', 'disconnected', 'stalled')),
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

-- 5. `messages.author_role` gains `integrator`.
CREATE TABLE messages_new (
    id                   TEXT PRIMARY KEY,
    goal_id              TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    task_id              TEXT REFERENCES tasks (id) ON DELETE CASCADE,   -- NULL = goal-level thread
    author_role          TEXT NOT NULL
                         CHECK (author_role IN ('planner', 'engineer', 'reviewer', 'integrator',
                                                'user', 'system')),
    author_session_id    TEXT REFERENCES agent_sessions (id),
    body                 TEXT NOT NULL,
    created_at           TEXT NOT NULL,
    recipient_kind       TEXT CHECK (recipient_kind IN ('profile', 'user')),
    recipient_profile_id TEXT REFERENCES profiles (id)
                         CHECK (recipient_profile_id IS NULL OR recipient_kind IS 'profile')
                         CHECK (recipient_kind IS NOT 'profile' OR recipient_profile_id IS NOT NULL)
);

INSERT INTO messages_new (id, goal_id, task_id, author_role, author_session_id, body,
                          created_at, recipient_kind, recipient_profile_id)
SELECT id, goal_id, task_id, author_role, author_session_id, body,
       created_at, recipient_kind, recipient_profile_id
FROM messages;

DROP TABLE messages;
ALTER TABLE messages_new RENAME TO messages;
CREATE INDEX idx_messages_task ON messages (task_id, id);
CREATE INDEX idx_messages_goal ON messages (goal_id, id);

COMMIT;

PRAGMA foreign_keys = ON;
