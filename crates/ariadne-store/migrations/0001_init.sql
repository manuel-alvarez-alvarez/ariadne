-- Ariadne initial schema. Schema only: the built-in profiles and their default
-- prompts are seeded from Rust constants after the migrations run, so a prompt
-- default can change without a migration (see `ariadne_store::defaults`).
-- Ids are lowercase ULIDs (TEXT, 26 chars); timestamps are ISO-8601 UTC TEXT.

CREATE TABLE profiles (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL UNIQUE,
    role          TEXT NOT NULL CHECK (role IN ('planner', 'engineer', 'reviewer')),
    -- NULL = auto: resolved at spawn time to the first installed agent CLI
    -- (claude_code, then codex, then opencode).
    agent_kind    TEXT CHECK (agent_kind IN ('claude_code', 'codex', 'opencode')),
    model         TEXT,
    system_prompt TEXT NOT NULL,
    extra_flags   TEXT NOT NULL DEFAULT '[]',   -- JSON array of argv strings
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

-- Prompts a profile owns beside its system prompt: one row per briefing the
-- daemon renders for that profile's role. Which kinds are valid for which role
-- is enforced in Rust (`ariadne_core::PromptKind`), not here, so the set can
-- grow without a migration.
CREATE TABLE profile_prompts (
    profile_id TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    kind       TEXT NOT NULL,
    content    TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (profile_id, kind)
);

CREATE TABLE goals (
    id                  TEXT PRIMARY KEY,
    title               TEXT NOT NULL,
    description         TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'planning'
                        CHECK (status IN ('planning', 'active', 'completed', 'cancelled')),
    max_tasks           INTEGER,                -- NULL = unbounded
    required_approvals  INTEGER NOT NULL DEFAULT 1 CHECK (required_approvals >= 1),
    planner_profile_id  TEXT NOT NULL REFERENCES profiles (id),
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);

CREATE TABLE goal_repos (
    id          TEXT PRIMARY KEY,
    goal_id     TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    path        TEXT NOT NULL,                  -- absolute repo path
    base_branch TEXT NOT NULL
);
CREATE INDEX idx_goal_repos_goal ON goal_repos (goal_id);

CREATE TABLE tasks (
    id                  TEXT PRIMARY KEY,
    goal_id             TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    repo_id             TEXT NOT NULL REFERENCES goal_repos (id),
    title               TEXT NOT NULL,
    description         TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'pending'
                        CHECK (status IN ('pending', 'ready', 'in_progress', 'under_review',
                                          'changes_requested', 'approved', 'merging', 'merged',
                                          'cancelled', 'failed')),
    engineer_profile_id TEXT NOT NULL REFERENCES profiles (id),
    branch              TEXT NOT NULL,          -- ariadne/task-<id>
    worktree_path       TEXT,
    review_round        INTEGER NOT NULL DEFAULT 0,
    stalled             INTEGER NOT NULL DEFAULT 0,
    merge_commit        TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);
CREATE INDEX idx_tasks_goal ON tasks (goal_id);
CREATE INDEX idx_tasks_status ON tasks (status);

CREATE TABLE task_reviewers (
    task_id    TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    profile_id TEXT NOT NULL REFERENCES profiles (id),
    position   INTEGER NOT NULL,
    PRIMARY KEY (task_id, profile_id)
);

CREATE TABLE task_dependencies (
    task_id            TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    depends_on_task_id TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    PRIMARY KEY (task_id, depends_on_task_id),
    CHECK (task_id <> depends_on_task_id)
);
CREATE INDEX idx_task_deps_on ON task_dependencies (depends_on_task_id);

CREATE TABLE agent_sessions (
    id                  TEXT PRIMARY KEY,       -- == ARIADNE_SESSION_ID env of the agent
    goal_id             TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    task_id             TEXT REFERENCES tasks (id) ON DELETE CASCADE,  -- NULL = planner
    role                TEXT NOT NULL CHECK (role IN ('planner', 'engineer', 'reviewer')),
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
    ended_at            TEXT
);
CREATE INDEX idx_sessions_task ON agent_sessions (task_id);
CREATE INDEX idx_sessions_status ON agent_sessions (status);

CREATE TABLE messages (
    id                TEXT PRIMARY KEY,
    goal_id           TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    task_id           TEXT REFERENCES tasks (id) ON DELETE CASCADE,   -- NULL = goal-level thread
    author_role       TEXT NOT NULL
                      CHECK (author_role IN ('planner', 'engineer', 'reviewer', 'user', 'system')),
    author_session_id TEXT REFERENCES agent_sessions (id),
    body              TEXT NOT NULL,
    created_at        TEXT NOT NULL
);
CREATE INDEX idx_messages_task ON messages (task_id, id);
CREATE INDEX idx_messages_goal ON messages (goal_id, id);

CREATE TABLE reviews (
    id                  TEXT PRIMARY KEY,
    task_id             TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    round               INTEGER NOT NULL,
    reviewer_profile_id TEXT NOT NULL REFERENCES profiles (id),
    session_id          TEXT REFERENCES agent_sessions (id),
    verdict             TEXT NOT NULL CHECK (verdict IN ('approve', 'request_changes')),
    body                TEXT,
    created_at          TEXT NOT NULL,
    UNIQUE (task_id, round, reviewer_profile_id)
);
CREATE INDEX idx_reviews_task ON reviews (task_id, round);

CREATE TABLE agent_events (
    id         TEXT PRIMARY KEY,
    session_id TEXT REFERENCES agent_sessions (id) ON DELETE SET NULL,
    task_id    TEXT REFERENCES tasks (id) ON DELETE CASCADE,
    agent_kind TEXT CHECK (agent_kind IN ('claude_code', 'codex', 'opencode')),
    kind       TEXT NOT NULL,                   -- session_start | post_tool_use | stop | turn_complete | ...
    payload    TEXT NOT NULL,                   -- raw JSON
    created_at TEXT NOT NULL
);
CREATE INDEX idx_events_task ON agent_events (task_id, id);
CREATE INDEX idx_events_session ON agent_events (session_id, id);

CREATE TABLE task_transitions (
    id          TEXT PRIMARY KEY,
    task_id     TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    from_status TEXT NOT NULL,
    to_status   TEXT NOT NULL,
    actor       TEXT NOT NULL CHECK (actor IN ('planner', 'engineer', 'reviewer', 'daemon', 'user')),
    reason      TEXT,
    created_at  TEXT NOT NULL
);
CREATE INDEX idx_transitions_task ON task_transitions (task_id, id);
