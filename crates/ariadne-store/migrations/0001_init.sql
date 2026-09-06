-- Ariadne initial schema, and the only migration: what came before it was 29
-- files, most of them prompt text, and none of that history says anything a
-- fresh database needs. A lifecycle briefing is Ariadne's own constant and a
-- skill document is an override (`skills.document` holds a text only where
-- somebody wrote one), so a reworded default never touches the database again
-- and this file never has to grow a successor for one.
--
-- Schema only: the built-in skills and the per-agent launch flags are seeded
-- from Rust constants after the migrations run (`seed_builtin_skills`,
-- `seed_agent_configs`), so a default can change without a migration.
--
-- Ids are lowercase ULIDs (TEXT, 26 chars); timestamps are ISO-8601 UTC TEXT.

-- A skill: one document that tells a generic agent how to do one kind of
-- work. Ariadne defines exactly one agent type, the orchestrator; every other
-- agent is generic and becomes what its task needs by loading skills.
--
-- `document` is the whole SKILL.md — YAML frontmatter naming the skill and
-- describing it, then the body — in the format Claude Code and Codex both
-- read, so one text serves every agent CLI.
--
-- NULL `document` means a built-in still on the text Ariadne ships
-- (`ariadne_store::defaults`), which is what a reset goes back to by clearing
-- the column, and why rewording a shipped skill reaches every database without
-- touching a row. A skill the user wrote has nowhere to fall back to, so it
-- must carry its own text.
CREATE TABLE skills (
    name       TEXT PRIMARY KEY,                -- kebab-case; how an agent loads it
    document   TEXT,                            -- NULL = the shipped default of `name`
    builtin    INTEGER NOT NULL DEFAULT 0 CHECK (builtin IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (builtin = 1 OR document IS NOT NULL)
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
-- How a change reaches `base_branch` is not here. That is the task's own
-- `landing`, agreed with the user task by task, and the procedure it names is
-- Ariadne's (`ariadne_store::defaults::default_landing_prompt`). A repository
-- is a checkout and a base branch, and nothing else about how work ends.
CREATE TABLE repositories (
    id          TEXT PRIMARY KEY,
    path        TEXT NOT NULL,                  -- absolute repo path
    base_branch TEXT NOT NULL,
    description TEXT,                           -- NULL = none given
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    -- The same checkout can be registered once per base branch.
    UNIQUE (path, base_branch)
);

-- The agent, model and effort columns on `goals` and `task_agents` are pins:
-- the orchestrator sizes each agent it staffs and writes the answer here, and
-- the row is what the launcher reads from there on. All three NULLs are
-- meaningful — NULL agent_kind means auto (resolved at spawn time to the first
-- installed CLI), NULL model means that CLI's own default, NULL effort means
-- whatever the CLI runs the model at. The user's later choice overwrites them,
-- while the task has not started.
CREATE TABLE goals (
    id                  TEXT PRIMARY KEY,
    title               TEXT NOT NULL,
    description         TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'planning'
                        CHECK (status IN ('planning', 'active', 'completed', 'cancelled')),
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
                                          'changes_requested', 'approved', 'finished',
                                          'cancelled', 'failed')),
    branch              TEXT NOT NULL,
    -- How this task ends: a change on the base branch, a request somebody
    -- else merges, or nothing at all. Taken from the repository's merge
    -- strategy unless whoever wrote the task said otherwise.
    landing             TEXT NOT NULL DEFAULT 'merge'
                        CHECK (landing IN ('merge', 'pull_request', 'none')),
    worktree_path       TEXT,
    review_round        INTEGER NOT NULL DEFAULT 0,
    stalled             INTEGER NOT NULL DEFAULT 0,
    merge_commit        TEXT,
    -- The pull or merge request the author published, where it published one.
    pr_url              TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);
CREATE INDEX idx_tasks_goal ON tasks (goal_id);
CREATE INDEX idx_tasks_status ON tasks (status);

-- The agents staffed on a task. An agent has no identity of its own: it is an
-- agent CLI, a model, an effort, a brief and a set of skills, and its seat
-- says only where it sits — the one author that carries the task from its
-- first commit to the end, or one of the reviewers that vote on it.
--
-- `ordinal` is the order the orchestrator listed them in. There is exactly one
-- author per task (the partial index below), because the task never leaves it.
CREATE TABLE task_agents (
    id         TEXT PRIMARY KEY,
    task_id    TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    seat       TEXT NOT NULL CHECK (seat IN ('author', 'reviewer')),
    ordinal    INTEGER NOT NULL,
    agent_kind TEXT
               CHECK (agent_kind IN ('claude_code', 'codex', 'opencode')),
    model      TEXT,
    effort     TEXT,
    -- What this agent is told beyond the task itself, where the orchestrator
    -- has something to add. NULL = the task is the whole of it.
    brief      TEXT,
    UNIQUE (task_id, seat, ordinal)
);
CREATE INDEX idx_task_agents_task ON task_agents (task_id);
CREATE UNIQUE INDEX idx_task_one_author ON task_agents (task_id) WHERE seat = 'author';

-- The skills an agent loads, in the order they are listed to it.
CREATE TABLE task_agent_skills (
    agent_id   TEXT NOT NULL REFERENCES task_agents (id) ON DELETE CASCADE,
    skill_name TEXT NOT NULL REFERENCES skills (name),
    ordinal    INTEGER NOT NULL,
    PRIMARY KEY (agent_id, skill_name)
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
--
-- `launch_id` names that run. A relaunch kills a pane and starts another under
-- the same row, so for a moment two processes share one ARIADNE_SESSION_ID:
-- the one being torn down still has its exit hook to fire, and the report of
-- it would otherwise land on the process that replaced it and retire a session
-- that is running. Every launch is given a fresh id, the agent carries it in
-- ARIADNE_LAUNCH_ID, and an event that names another one is a dead process
-- talking.
CREATE TABLE agent_sessions (
    id                  TEXT PRIMARY KEY,       -- == ARIADNE_SESSION_ID env of the agent
    goal_id             TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    task_id             TEXT REFERENCES tasks (id) ON DELETE CASCADE,  -- NULL = orchestrator
    seat                TEXT NOT NULL CHECK (seat IN ('orchestrator', 'author', 'reviewer')),
    -- Which staffed agent this session runs; NULL for an orchestrator,
    -- which is the one agent type Ariadne defines rather than one a task
    -- staffs.
    task_agent_id       TEXT REFERENCES task_agents (id) ON DELETE CASCADE,
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
    -- Copied off the pin the session's seat carries, beside its model.
    effort              TEXT,
    launched_at         TEXT,
    launch_id           TEXT,                   -- == ARIADNE_LAUNCH_ID env of that run
    -- When the daemon last decided this session owes a compaction of its
    -- agent conversation — set at every hand-off (a plan finalized, a review
    -- requested, a verdict given) and cleared once the CLI reports the
    -- compaction done, or the daemon stops waiting for it. NULL = nothing
    -- owed.
    compact_owed_at     TEXT
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

-- What one agent said to another.
--
-- One channel for everything the agents say between themselves: a question and
-- its answer, an author asking for a review, a reviewer's verdict, a note. A
-- verdict used to be a table of its own, which is why the only thing a
-- reviewer could ever say was approve or request changes.
--
-- Every message has exactly one recipient, so a request that goes to three
-- reviewers is three rows: `delivered_at` is per recipient, and one row with
-- three readers could not say which of them has seen it.
--
-- `to_agent_id` names the staffed agent a message is for. It is NULL for the
-- orchestrator, which is staffed on no task, and `to_actor` says which of the
-- two it is.
CREATE TABLE messages (
    id            TEXT PRIMARY KEY,
    goal_id       TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    -- NULL for a message about the goal rather than about one task.
    task_id       TEXT REFERENCES tasks (id) ON DELETE CASCADE,
    -- The review round this belongs to, so a verdict is counted in its own.
    round         INTEGER NOT NULL DEFAULT 0,
    kind          TEXT NOT NULL
                  CHECK (kind IN ('question', 'answer', 'review_request',
                                  'approve', 'request_changes', 'note')),
    from_actor    TEXT NOT NULL
                  CHECK (from_actor IN ('orchestrator', 'author', 'reviewer',
                                        'daemon', 'user')),
    from_agent_id TEXT REFERENCES task_agents (id) ON DELETE CASCADE,
    -- The session that actually sent it, for the record; a message outlives it.
    from_session  TEXT REFERENCES agent_sessions (id) ON DELETE SET NULL,
    to_actor      TEXT NOT NULL
                  CHECK (to_actor IN ('orchestrator', 'author', 'reviewer',
                                      'daemon', 'user')),
    to_agent_id   TEXT REFERENCES task_agents (id) ON DELETE CASCADE,
    -- The message this answers, where it answers one.
    in_reply_to   TEXT REFERENCES messages (id) ON DELETE SET NULL,
    body          TEXT NOT NULL,
    -- When it reached the recipient's pane. NULL while it is still waiting.
    delivered_at  TEXT,
    created_at    TEXT NOT NULL
);
CREATE INDEX idx_messages_task ON messages (task_id, id);
CREATE INDEX idx_messages_goal ON messages (goal_id, id);
-- One verdict per reviewer per round, which is what closes a round.
CREATE UNIQUE INDEX idx_messages_one_verdict
    ON messages (task_id, round, from_agent_id)
    WHERE kind IN ('approve', 'request_changes');

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
                CHECK (actor IN ('orchestrator', 'author', 'reviewer', 'daemon', 'user')),
    reason      TEXT,
    created_at  TEXT NOT NULL
);
CREATE INDEX idx_transitions_task ON task_transitions (task_id, id);
