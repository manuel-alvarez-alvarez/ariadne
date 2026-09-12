-- Ariadne initial schema, and the only migration: what came before it was 29
-- files, most of them prompt text, and none of that history says anything a
-- fresh database needs. A lifecycle briefing is Ariadne's own constant and a
-- skill document is an override (`skills.document` holds a text only where
-- somebody wrote one), so a reworded default never touches the database again
-- and this file never has to grow a successor for one.
--
-- Schema only: the built-in skills are seeded from Rust constants after the
-- migrations run (`seed_builtin_skills`), so a default can change without a
-- migration.
--
-- Every agent is an ACP agent in the daemon's registry, named by its registry
-- id. A model is chosen by one string, `<agent>:<model>`
-- (`ariadne_core::models::ModelRef`), whose first segment is that id: the
-- `model` columns below hold it whole, and nothing else names the agent.
--
-- Ids are lowercase ULIDs (TEXT, 26 chars); timestamps are ISO-8601 UTC TEXT.

-- A skill: one document that tells a generic agent how to do one kind of
-- work. Ariadne defines exactly one agent type, the orchestrator; every other
-- agent is generic and becomes what its task needs by loading skills.
--
-- `document` is the whole SKILL.md — YAML frontmatter naming the skill and
-- describing it, then the body — so one text serves every agent.
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

-- The flags a registry agent is launched with, appended to its registry
-- command. Keyed by the registry id; an agent with no row is launched with
-- its command alone. Read on every spawn and resume.
CREATE TABLE agent_configs (
    agent_id    TEXT PRIMARY KEY,
    extra_flags TEXT NOT NULL,                  -- JSON array of argv strings
    updated_at  TEXT NOT NULL
);

-- What a registry agent's `session/new` offered — its models and efforts —
-- read once per agent version, so a daemon start opens no session on an
-- agent it has already read. A row stands only for the command and version
-- it was read from; an upgrade, or another command under the id, reads
-- again and replaces it.
CREATE TABLE acp_catalogs (
    agent_id TEXT PRIMARY KEY,
    command  TEXT NOT NULL,                     -- JSON array of argv strings
    version  TEXT NOT NULL,                     -- the agent's `agentInfo.version`
    catalog  TEXT NOT NULL,                     -- JSON, written and read by discovery
    read_at  TEXT NOT NULL
);

-- The models the user has turned off. A model is available unless a row here
-- says otherwise, so the catalog — discovered live from each registry agent —
-- keeps every entry it grows usable without a write here.
--
-- The id is `<agent>:<model>`, the one string a model is chosen by. The
-- catalog itself is discovery, so nothing joins on this: it is read as a set
-- and subtracted.
CREATE TABLE disabled_models (
    id          TEXT PRIMARY KEY,
    disabled_at TEXT NOT NULL
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

-- What past work learned about one repository. Source ids are kept as text,
-- not foreign keys: a memory survives deletion of the session, task or goal
-- that taught it. Deleting the repository removes its memories.
CREATE TABLE memories (
    id                TEXT PRIMARY KEY,
    repository_id     TEXT NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    text              TEXT NOT NULL,
    source_session_id TEXT NOT NULL,
    source_task_id    TEXT,
    source_goal_id    TEXT NOT NULL,
    created_at        TEXT NOT NULL,
    expires_at        TEXT NOT NULL
);
CREATE INDEX idx_memories_repository ON memories (repository_id, id);

-- The model and effort columns on `goals` and `task_agents` are pins: the
-- orchestrator sizes each agent it staffs and writes the answer here, and the
-- row is what the launcher reads from there on. The model is required —
-- `<agent>:<model>`, and no agent default stands in for one. Only the effort
-- may be NULL, which means whatever the agent runs the model at. The user's
-- later choice overwrites them, while the task has not started.
CREATE TABLE goals (
    id                  TEXT PRIMARY KEY,
    title               TEXT NOT NULL,
    description         TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'planning'
                        CHECK (status IN ('planning', 'active', 'completed', 'cancelled')),
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    model               TEXT NOT NULL,
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
    -- NULL takes the daemon's configured default.
    permission_mode     TEXT CHECK (permission_mode IN ('auto', 'ask', 'learn')),
    worktree_path       TEXT,
    stalled             INTEGER NOT NULL DEFAULT 0,
    merge_commit        TEXT,
    -- The pull or merge request the author published, where it published one.
    pr_url              TEXT,
    -- The author the reviewers picked, on a task staffed with several. Its
    -- branch is what lands; the other authors' branches and worktrees go.
    -- NULL for a one-author task, and until the pick settles.
    picked_agent_id     TEXT REFERENCES task_agents (id),
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);
CREATE INDEX idx_tasks_goal ON tasks (goal_id);
CREATE INDEX idx_tasks_status ON tasks (status);

-- An ACP permission approval learned from one repository. A denial has no
-- row, so it is always asked again.
CREATE TABLE learned_permissions (
    repository_id TEXT NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    tool_name     TEXT NOT NULL,
    kind          TEXT NOT NULL,
    created_at    TEXT NOT NULL,
    PRIMARY KEY (repository_id, tool_name, kind)
);

-- The agents staffed on a task. An agent has no identity of its own: it is a
-- model of a registry agent, an effort, a brief and a set of skills, and its seat
-- says only where it sits — one of the authors that write the task, each on
-- its own branch, or one of the reviewers that vote on it.
--
-- `ordinal` is the order the orchestrator listed them in. Most tasks staff
-- one author; a task staffed with several runs them in parallel, each in its
-- own worktree, and the reviewers pick the one that lands (`task_picks`).
CREATE TABLE task_agents (
    id         TEXT PRIMARY KEY,
    task_id    TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    seat       TEXT NOT NULL CHECK (seat IN ('author', 'reviewer')),
    ordinal    INTEGER NOT NULL,
    model      TEXT NOT NULL,
    effort     TEXT,
    -- What this agent is told beyond the task itself, where the orchestrator
    -- has something to add. NULL = the task is the whole of it.
    brief      TEXT,
    UNIQUE (task_id, seat, ordinal)
);
CREATE INDEX idx_task_agents_task ON task_agents (task_id);

-- One reviewer's pick of the winning author, on a task staffed with several
-- authors. The primary key is what holds a reviewer to one pick per task:
-- a second one is refused by the schema, and the refusal names the reviewer.
CREATE TABLE task_picks (
    task_id           TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    reviewer_agent_id TEXT NOT NULL REFERENCES task_agents (id) ON DELETE CASCADE,
    author_agent_id   TEXT NOT NULL REFERENCES task_agents (id) ON DELETE CASCADE,
    created_at        TEXT NOT NULL,
    PRIMARY KEY (task_id, reviewer_agent_id)
);

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
-- `launch_id` names that run. A relaunch kills an agent process and starts
-- another under the same row, so for a moment two processes share one
-- ARIADNE_SESSION_ID: the one being torn down still has its exit to report,
-- and the report of it would otherwise land on the process that replaced it
-- and retire a session that is running. Every launch is given a fresh id, the
-- runtime reports each process's events under its own, and an event that
-- names another one is a dead process talking.
CREATE TABLE agent_sessions (
    id                  TEXT PRIMARY KEY,       -- == ARIADNE_SESSION_ID env of the agent
    goal_id             TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    task_id             TEXT REFERENCES tasks (id) ON DELETE CASCADE,  -- NULL = orchestrator
    seat                TEXT NOT NULL CHECK (seat IN ('orchestrator', 'author', 'reviewer')),
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
                                                    'disconnected', 'stalled')),
    attention_since     TEXT,
    model               TEXT NOT NULL,
    -- Copied off the pin the session's seat carries, beside its model.
    effort              TEXT,
    launched_at         TEXT,
    launch_id           TEXT                    -- == ARIADNE_LAUNCH_ID env of that run
);
CREATE INDEX idx_sessions_task ON agent_sessions (task_id);
CREATE INDEX idx_sessions_status ON agent_sessions (status);
CREATE INDEX idx_sessions_attention ON agent_sessions (attention_reason);

-- What each agent session has spent, as the transcripts under it report it.
--
-- A `source` is one transcript the totals were read from, and its three
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
-- One channel for everything the agents say between themselves: a message, an
-- author asking for a review, and a reviewer's verdict. A verdict used to be a
-- table of its own, which is why the only thing a reviewer could ever say was
-- approve or request changes.
--
-- One kind carries everything said outside a review, whether it asks or
-- answers. There is no `answer` kind and no reply: an answer is a row
-- addressed to whoever asked, so nothing threads and nothing points back.
--
-- A review is bounded by its own request rather than by a round number: the
-- verdicts that count are the ones sent after the last `review_request`, and
-- asking for a review again supersedes what came before it.
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
    kind          TEXT NOT NULL
                  CHECK (kind IN ('message', 'review_request',
                                  'approve', 'request_changes')),
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
    body          TEXT NOT NULL,
    -- When it was handed to the recipient's agent. NULL while it is still waiting.
    delivered_at  TEXT,
    created_at    TEXT NOT NULL
);
CREATE INDEX idx_messages_task ON messages (task_id, id);
CREATE INDEX idx_messages_goal ON messages (goal_id, id);
-- One verdict per reviewer per review request. Not an index: what a verdict
-- belongs to is "the review asked for last", which is a row of this same table
-- rather than a column, so the rule is read where a verdict is written
-- (`http::landing::send`).

CREATE TABLE agent_events (
    id         TEXT PRIMARY KEY,
    session_id TEXT REFERENCES agent_sessions (id) ON DELETE SET NULL,
    task_id    TEXT REFERENCES tasks (id) ON DELETE CASCADE,
    kind       TEXT NOT NULL,                   -- session_start | post_tool_use | stop | ...
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
