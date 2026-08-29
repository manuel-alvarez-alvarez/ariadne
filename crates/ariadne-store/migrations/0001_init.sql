-- Ariadne initial schema, and the only migration: what came before it was 29
-- files, most of them prompt text, and none of that history says anything a
-- fresh database needs. Prompts are overrides now (`profile_prompts` holds a
-- row only where somebody wrote one), so a reworded default never touches the
-- database again and this file never has to grow a successor for one.
--
-- Schema only: the built-in profiles and the per-agent launch flags are seeded
-- from Rust constants after the migrations run (`seed_builtin_profiles`,
-- `seed_agent_configs`), so a default can change without a migration.
--
-- Ids are lowercase ULIDs (TEXT, 26 chars); timestamps are ISO-8601 UTC TEXT.

-- Who an agent session runs as: its role, the CLI and model it is launched
-- with, and the system prompt it is briefed with.
--
-- NULL `system_prompt` is the default of the role (see `ariadne_store::
-- defaults`); text is what its user wrote instead, which is also what a reset
-- goes back to by clearing.
CREATE TABLE profiles (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL UNIQUE,
    role          TEXT NOT NULL CHECK (role IN ('planner', 'engineer', 'reviewer')),
    -- NULL = auto: resolved at spawn time to the first installed agent CLI
    -- (claude_code, then codex, then opencode).
    agent_kind    TEXT CHECK (agent_kind IN ('claude_code', 'codex', 'opencode')),
    -- NULL = the agent CLI's own default.
    model         TEXT,
    -- NULL = whatever the agent CLI runs that model at on its own; otherwise
    -- one of the efforts the model accepts, checked against the catalog
    -- (`GET /v1/models`) when it is written.
    effort        TEXT,
    -- NULL = the default system prompt of `role`.
    system_prompt TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

-- The briefings a profile owns beside its system prompt, one row per kind it
-- was given text of its own for. Which kinds are valid for which role is
-- enforced in Rust (`ariadne_core::PromptKind`), not here, so the set can grow
-- without a migration — and a kind with no row is briefed with its default.
CREATE TABLE profile_prompts (
    profile_id TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    kind       TEXT NOT NULL,
    content    TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (profile_id, kind)
);

-- Per-agent-kind launch configuration: how an agent CLI is allowed to run is
-- a property of that CLI, not of the persona a profile describes. Read on
-- every spawn and resume.
CREATE TABLE agent_configs (
    agent_kind  TEXT PRIMARY KEY CHECK (agent_kind IN ('claude_code', 'codex', 'opencode')),
    extra_flags TEXT NOT NULL,                  -- JSON array of argv strings
    updated_at  TEXT NOT NULL
);

-- A checkout, registered once globally and named by id from there on, so that
-- editing it moves every goal that works in it.
--
-- `merge_strategy` is how a task lands on `base_branch`: `direct` squashes and
-- fast-forwards with git alone, `pull_request` publishes a request for a human
-- to merge.
CREATE TABLE repositories (
    id             TEXT PRIMARY KEY,
    path           TEXT NOT NULL,               -- absolute repo path
    base_branch    TEXT NOT NULL,
    description    TEXT,                        -- NULL = none given
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    merge_strategy TEXT NOT NULL DEFAULT 'direct'
                   CHECK (merge_strategy IN ('direct', 'pull_request')),
    -- The same checkout can be registered once per base branch.
    UNIQUE (path, base_branch)
);

-- The agent, model and effort columns on `goals`, `tasks` and
-- `task_reviewers` are pins: creation snapshots them off the profile it names,
-- and the row is what the launcher reads from there on, so a profile edit only
-- reaches work created after it. All three NULLs are meaningful — NULL
-- agent_kind means auto, NULL model means the CLI's own default, NULL effort
-- means whatever that CLI runs the model at — exactly as on `profiles`. An
-- effort is snapshotted off the profile only where the model is the profile's
-- too: an override that moves the row to another model runs it at that CLI's
-- own default, since the profile's effort may not exist on the new model. The
-- system prompt is deliberately not pinned: rewording a briefing is meant to
-- reach running work.
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
    updated_at          TEXT NOT NULL,
    agent_kind          TEXT
                        CHECK (agent_kind IN ('claude_code', 'codex', 'opencode')),
    model               TEXT,
    effort              TEXT
);

-- Which repositories a goal works in, by reference.
CREATE TABLE goal_repositories (
    goal_id       TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    repository_id TEXT NOT NULL REFERENCES repositories (id),
    PRIMARY KEY (goal_id, repository_id)
);
-- Deleting a repository asks who still holds it, which reads this way round.
CREATE INDEX idx_goal_repositories_repository ON goal_repositories (repository_id);

CREATE TABLE tasks (
    id                  TEXT PRIMARY KEY,
    goal_id             TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    repo_id             TEXT NOT NULL REFERENCES repositories (id),
    title               TEXT NOT NULL,
    description         TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'pending'
                        CHECK (status IN ('pending', 'ready', 'in_progress', 'under_review',
                                          'changes_requested', 'approved', 'merged',
                                          'cancelled', 'failed')),
    engineer_profile_id TEXT NOT NULL REFERENCES profiles (id),
    agent_kind          TEXT CHECK (agent_kind IN ('claude_code', 'codex', 'opencode')),
    model               TEXT,
    effort              TEXT,
    branch              TEXT NOT NULL,
    worktree_path       TEXT,
    review_round        INTEGER NOT NULL DEFAULT 0,
    stalled             INTEGER NOT NULL DEFAULT 0,
    merge_commit        TEXT,
    -- The pull or merge request the engineer published, where it published one.
    pr_url              TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);
CREATE INDEX idx_tasks_goal ON tasks (goal_id);
CREATE INDEX idx_tasks_status ON tasks (status);

CREATE TABLE task_reviewers (
    task_id    TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    profile_id TEXT NOT NULL REFERENCES profiles (id),
    position   INTEGER NOT NULL,
    agent_kind TEXT
               CHECK (agent_kind IN ('claude_code', 'codex', 'opencode')),
    model      TEXT,
    effort     TEXT,
    PRIMARY KEY (task_id, profile_id)
);

CREATE TABLE task_dependencies (
    task_id            TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    depends_on_task_id TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    PRIMARY KEY (task_id, depends_on_task_id),
    CHECK (task_id <> depends_on_task_id)
);
CREATE INDEX idx_task_deps_on ON task_dependencies (depends_on_task_id);

-- One run of one agent. `attention_reason` is orthogonal to `status`: a
-- session blocked on a permission prompt is still `running`, it just cannot
-- make progress until someone looks at it, and `waiting_user` is the one
-- nobody but the user clears. `attention_since` is when the current reason was
-- first raised, so re-raising the same reason leaves it alone.
--
-- `launched_at` is when this run of the agent process started — not the row's
-- `created_at`, and not the `last_activity_at` the agent moves — since a
-- session is relaunched under its own id on every resume.
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
    ended_at            TEXT,
    attention_reason    TEXT
                        CHECK (attention_reason IN ('waiting_permission', 'waiting_input',
                                                    'waiting_user', 'agent_error',
                                                    'disconnected', 'stalled')),
    attention_since     TEXT,
    model               TEXT,
    -- Copied off the pin the session's role carries, beside its model.
    effort              TEXT,
    launched_at         TEXT
);
CREATE INDEX idx_sessions_task ON agent_sessions (task_id);
CREATE INDEX idx_sessions_status ON agent_sessions (status);
CREATE INDEX idx_sessions_attention ON agent_sessions (attention_reason);

-- What each agent session has spent, as the transcripts under it report it.
--
-- A `source` is one transcript the totals were read from — the JSONL file a
-- Claude session writes, a Codex rollout, an OpenCode session — and its three
-- counters are that transcript's *cumulative* totals, never a delta: a fresh
-- report for the same source replaces the previous one. A session accumulates
-- several sources when its agent is resumed into a new transcript, so the
-- session's usage is the sum over its rows, and a task's or a goal's is the
-- sum over the sessions under it.
--
-- `input_tokens` counts every prompt token, cache reads and cache writes
-- included, and `cached_input_tokens` is the subset of it served from the
-- prompt cache — so the two are never added together. `output_tokens` counts
-- completion tokens, thinking and reasoning included.
CREATE TABLE session_usage (
    session_id          TEXT NOT NULL REFERENCES agent_sessions (id) ON DELETE CASCADE,
    source              TEXT NOT NULL,
    input_tokens        INTEGER NOT NULL,
    cached_input_tokens INTEGER NOT NULL,
    output_tokens       INTEGER NOT NULL,
    updated_at          TEXT NOT NULL,
    PRIMARY KEY (session_id, source)
);

-- A conversation line. NULL `recipient_kind` is said to the thread, addressed
-- to nobody in particular; a profile addressee carries its id and the user has
-- none, hence the two checks tying the id to the kind (written with `IS`,
-- since a comparison against a NULL kind would be NULL, and a NULL check
-- passes).
CREATE TABLE messages (
    id                   TEXT PRIMARY KEY,
    goal_id              TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    task_id              TEXT REFERENCES tasks (id) ON DELETE CASCADE,  -- NULL = goal-level thread
    author_role          TEXT NOT NULL
                         CHECK (author_role IN ('planner', 'engineer', 'reviewer',
                                                'user', 'system')),
    author_session_id    TEXT REFERENCES agent_sessions (id),
    body                 TEXT NOT NULL,
    created_at           TEXT NOT NULL,
    recipient_kind       TEXT CHECK (recipient_kind IN ('profile', 'user')),
    recipient_profile_id TEXT REFERENCES profiles (id)
                         CHECK (recipient_profile_id IS NULL OR recipient_kind IS 'profile')
                         CHECK (recipient_kind IS NOT 'profile' OR recipient_profile_id IS NOT NULL)
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
    actor       TEXT NOT NULL
                CHECK (actor IN ('planner', 'engineer', 'reviewer', 'daemon', 'user')),
    reason      TEXT,
    created_at  TEXT NOT NULL
);
CREATE INDEX idx_transitions_task ON task_transitions (task_id, id);
