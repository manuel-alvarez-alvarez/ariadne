-- Ariadne's whole schema, and the only migration. Ariadne is pre-1.0: every
-- install is a first install, so there is nothing to carry forward and no
-- reason to keep the steps that got here. A database on an older chain is
-- refused rather than migrated (`pre_squash_database`); the fix is to delete
-- it and start again.
--
-- Schema only: the built-in skills and workflows are seeded from Rust
-- constants after the migrations run (`seed_builtin_skills`,
-- `seed_builtin_workflows`), so a default can change without a migration.
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

-- A workflow: a linear kanban of columns that stages an agent through a
-- task. Stored the same way a skill is: a NULL document while it runs on the
-- text Ariadne ships, so a reworded shipped workflow reaches every database
-- without a migration, and a reset drops the override rather than copying a
-- default in.
CREATE TABLE workflows (
    name       TEXT PRIMARY KEY,                -- kebab-case; named on the document's first line
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

-- The last accepted registry download. A refresh replaces this one row.
CREATE TABLE acp_registry_index (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    url        TEXT NOT NULL,
    document   TEXT NOT NULL,
    fetched_at TEXT NOT NULL
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

-- User-set ranks overlay discovery independently of disabled models.
-- An absent row means that the model is unranked.
CREATE TABLE model_ranks (
    id         TEXT PRIMARY KEY,
    rank       TEXT NOT NULL CHECK (rank IN ('frontier', 'balanced', 'fast', 'local')),
    updated_at TEXT NOT NULL
);

-- A checkout, registered once globally and named by id from there on, so that
-- editing it moves every goal that works in it.
--
-- A new goal whose request names no workflow runs on this repository's
-- default.
CREATE TABLE repositories (
    id              TEXT PRIMARY KEY,
    path            TEXT NOT NULL,               -- absolute repo path
    base_branch     TEXT NOT NULL,
    description     TEXT,                        -- NULL = none given
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    -- How the ACP permission requests of every session in this checkout are
    -- answered; `learn` keeps its approvals in `learned_permissions`.
    permission_mode TEXT NOT NULL DEFAULT 'auto'
                    CHECK (permission_mode IN ('auto', 'ask', 'learn', 'ai')),
    default_workflow TEXT NOT NULL DEFAULT 'develop-review-merge' REFERENCES workflows (name),
    -- The same checkout can be registered once per base branch.
    UNIQUE (path, base_branch)
);

-- The forge a repository's remote is on, and whether Ariadne works with it.
-- One row per repository, and none where the checkout has no usable remote.
-- `host`, `owner` and `name` are stored lower-cased, so one forge repository
-- reads the same whichever URL spelled it.
CREATE TABLE forge_integrations (
    repository_id            TEXT PRIMARY KEY REFERENCES repositories (id) ON DELETE CASCADE,
    kind                     TEXT NOT NULL CHECK (kind IN ('github', 'gitlab')),
    host                     TEXT NOT NULL,
    owner                    TEXT NOT NULL,
    name                     TEXT NOT NULL,
    remote                   TEXT NOT NULL,       -- the remote's name, `origin`
    enabled                  INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    login                    TEXT,                -- NULL until enabled
    review_model             TEXT,
    review_effort            TEXT,
    detected_at              TEXT NOT NULL,
    updated_at               TEXT NOT NULL,
    webhook_id               INTEGER,
    webhook_secret           TEXT,
    webhook_url              TEXT,
    webhook_state            TEXT NOT NULL DEFAULT 'polling'
                             CHECK (webhook_state IN ('live', 'polling', 'failed')),
    webhook_error            TEXT,
    webhook_last_delivery_at TEXT,
    -- Why the last fetch of this repository's requests failed, or NULL once
    -- one succeeds: polling has no hook to report on, so this is what says
    -- it works.
    fetch_error              TEXT
);
-- The same checkout can be registered once per base branch, and one forge
-- repository is enabled on one of those rows at a time.
CREATE UNIQUE INDEX idx_forge_integrations_enabled
    ON forge_integrations (host, owner, name) WHERE enabled = 1;

-- The one forge settings row: the tunnel switch, and the subdomain the
-- tunnel asks for, so its public URL survives a restart where the server
-- grants it again.
CREATE TABLE forge_settings (
    id               INTEGER PRIMARY KEY CHECK (id = 1),
    tunnel_enabled   INTEGER NOT NULL DEFAULT 1 CHECK (tunnel_enabled IN (0, 1)),
    -- NULL until the first tunnel picks one.
    tunnel_subdomain TEXT,
    updated_at       TEXT NOT NULL
);
INSERT INTO forge_settings (id, updated_at)
VALUES (1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

-- The model and effort columns on `goals` and `task_agents` are pins: the
-- orchestrator sizes each agent it staffs and writes the answer here, and the
-- row is what the launcher reads from there on. The model is required —
-- `<agent>:<model>`, and no agent default stands in for one. Only the effort
-- may be NULL, which means whatever the agent runs the model at. The user's
-- later choice overwrites them, while the task has not started.
CREATE TABLE goals (
    id           TEXT PRIMARY KEY,
    title        TEXT NOT NULL,
    description  TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'planning'
                 CHECK (status IN ('planning', 'active', 'completed', 'cancelled')),
    orchestrated INTEGER NOT NULL DEFAULT 1 CHECK (orchestrated IN (0, 1)),
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    model        TEXT NOT NULL,
    effort       TEXT,
    issue_url    TEXT,
    -- The workflow every task of the goal runs on. Chosen once, when the goal
    -- is created; its columns are snapshotted into `goal_steps`.
    workflow     TEXT NOT NULL REFERENCES workflows (name),
    -- The goal's failed and stalled task ids, each with the `updated_at`
    -- its failure carried at the moment the orchestrator's own
    -- `session/prompt` turn naming them actually returned (a JSON array
    -- of `[id, transition id]` pairs, the id of the `task_transitions` row
    -- that moved the task to `failed`) — written only by that turn's own
    -- completion (`acp::serve_with_input`, `Delivery::GoalAttention`),
    -- never by an ambient session status: an unrelated turn landing on
    -- the same session must not confirm a failure it never carried. The
    -- attention producer matches a task by both its id and this transition
    -- id, not by `Task.updated_at`, which an ordinary metadata edit on a
    -- failed task also moves; a later retry of the same task stamps a
    -- fresh transition row, so that confirmation does not answer for it.
    orchestrator_answered_failed_task_ids TEXT,
    -- When `scheduler::goals::orchestrator_could_not_start` gave up on this
    -- goal's orchestrator: the spawn-retry budget ran out, not merely a
    -- crash the liveness sweep is about to retry. Distinct from the
    -- `disconnected` flag `retire_disconnected` raises on every crash,
    -- which the scheduler may still resolve on its own; cleared the moment
    -- `keep_orchestrator` has a live orchestrator or a resume/spawn
    -- succeeds, so recovery taking ownership again takes this down with it.
    orchestrator_given_up_at TEXT,
    -- Whether that give-up was `scheduler::quiet::relaunch_wedged`'s own
    -- exhausted-relaunch decision for a session that started and then
    -- stopped answering, rather than `orchestrator_could_not_start`'s —
    -- one that never got off the ground at all. Read directly rather than
    -- inferred from the alarm session's own `stalled` flag: a pass can
    -- jump straight past the flag threshold to the relaunch one and give
    -- up without ever raising it, so the flag is not reliable evidence of
    -- which give-up this was.
    orchestrator_given_up_wedged INTEGER NOT NULL DEFAULT 0
);

-- Which repositories a goal works in, by reference.
CREATE TABLE goal_repositories (
    goal_id       TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    repository_id TEXT NOT NULL REFERENCES repositories (id),
    PRIMARY KEY (goal_id, repository_id)
);
-- Deleting a repository asks who still holds it, which reads this way round.
CREATE INDEX idx_goal_repositories_repository ON goal_repositories (repository_id);

-- A goal's workflow, snapshotted into its columns the moment the goal is
-- created: a later reword of the shipped workflow never moves a goal already
-- running on it.
CREATE TABLE goal_steps (
    goal_id     TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    ordinal     INTEGER NOT NULL,
    id          TEXT NOT NULL,
    title       TEXT NOT NULL,
    description TEXT NOT NULL,
    skills      TEXT NOT NULL CHECK (json_valid(skills)),
    rank        TEXT,
    gate        TEXT,
    PRIMARY KEY (goal_id, ordinal),
    UNIQUE (goal_id, id)
);

CREATE TABLE tasks (
    id            TEXT PRIMARY KEY,
    goal_id       TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
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
    step          TEXT
);
CREATE INDEX idx_tasks_goal ON tasks (goal_id);
CREATE INDEX idx_tasks_status ON tasks (status);

-- A pull request row holds Ariadne's own bookkeeping of a request it works
-- on, and nothing the forge holds: the title, the description, the
-- branches, the checks and the comments are read off the forge on every
-- fetch and kept in memory alone, so nothing stored can go stale.
--
-- A row stays only while Ariadne works on the request: the one a task
-- opened, one of the user's they asked Ariadne to review, or an open one
-- that asks for their review on a repository with a review pin.
CREATE TABLE pull_requests (
    id                    TEXT PRIMARY KEY,
    repository_id         TEXT NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    number                INTEGER NOT NULL CHECK (number > 0),
    url                   TEXT NOT NULL,
    -- NULL for one the user opened without Ariadne's help.
    origin_task_id        TEXT REFERENCES tasks (id) ON DELETE SET NULL,
    role                  TEXT NOT NULL CHECK (role IN ('author', 'reviewer')),
    ready                 INTEGER NOT NULL CHECK (ready IN (0, 1)),
    created_at            TEXT NOT NULL,
    updated_at            TEXT NOT NULL,
    -- What the request's session was last told, so each change is told
    -- once: the failed checks by name, whether told the head is behind its
    -- base, and the review decision, state and rolled-up check state.
    told_checks           TEXT NOT NULL DEFAULT '[]',
    told_behind_base      INTEGER NOT NULL DEFAULT 0 CHECK (told_behind_base IN (0, 1)),
    told_review_decision  TEXT,
    told_state            TEXT,
    told_check_state      TEXT,
    -- When the session was last handed news, which is when its prompt went out.
    news_told_at          TEXT,
    -- The head a reviewer session last posted a review on, and the head it
    -- was last told of, so a push is told once.
    reviewed_sha          TEXT,
    told_head_sha         TEXT,
    -- Whether a request of the user's own has been asked for an Ariadne
    -- review. Off on every row until asked.
    review_asked          INTEGER NOT NULL DEFAULT 0 CHECK (review_asked IN (0, 1)),
    -- The pin and extra skills the user picked for that review, where they
    -- asked for one: the review session runs on them rather than on the
    -- repository's review pin. NULL where nobody asked.
    review_model          TEXT,
    review_effort         TEXT,
    review_skills         TEXT NOT NULL DEFAULT '[]',
    -- The one summary comment an Ariadne review keeps on a request: its
    -- forge id, written when the review first posts it, and edited in place
    -- on every later round. NULL until the first review of the request.
    summary_comment_id    TEXT,
    -- When `scheduler::pull_requests::start_pull_request_session` gave up
    -- on this request's reviewer session: its spawn-retry budget ran out,
    -- not merely a crash the liveness sweep is about to retry. Cleared the
    -- moment a resume or spawn of that session next succeeds.
    reviewer_given_up_at  TEXT,
    -- Whether that give-up was `scheduler::quiet::relaunch_wedged`'s own
    -- exhausted-relaunch decision rather than `start_pull_request_session`'s
    -- own spawn-retry exhaustion — the same distinction, and for the same
    -- reason, as `goals.orchestrator_given_up_wedged`.
    reviewer_given_up_wedged INTEGER NOT NULL DEFAULT 0,
    UNIQUE (repository_id, number)
);

-- A comment is the forge's: what stays of it is whether its session was
-- told of it, and whether an Ariadne review posted it.
CREATE TABLE pull_request_comment_marks (
    pull_request_id TEXT NOT NULL REFERENCES pull_requests (id) ON DELETE CASCADE,
    forge_id        TEXT NOT NULL,
    told_at         TEXT,
    from_review     INTEGER NOT NULL DEFAULT 0 CHECK (from_review IN (0, 1)),
    PRIMARY KEY (pull_request_id, forge_id)
);

-- The AI permission model, the local model the `ai` permission mode answers
-- with. One row, because the settings are the daemon's and not a
-- repository's: a repository chooses the `ai` mode, and this says whether
-- there is a model to answer with.
--
-- The install state is here rather than read off the disk because an install
-- takes minutes and two gigabytes: what a client needs is what happened to
-- the last one, which outlives the task that ran it and the daemon that
-- started it. The files themselves stay under `<home>/ai-permissions`, and a
-- disabled model keeps them.
CREATE TABLE ai_permission_settings (
    id                   INTEGER PRIMARY KEY CHECK (id = 1),
    enabled              INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    -- Danger at or below this value is allowed, 0 to 1.
    allow_threshold      REAL NOT NULL DEFAULT 0.0201,
    -- Danger at or above this value is denied, 0 to 1.
    deny_threshold       REAL NOT NULL DEFAULT 0.6321,
    state                TEXT NOT NULL DEFAULT 'disabled'
                         CHECK (state IN ('disabled', 'installing', 'ready', 'failed')),
    installed_release    TEXT,                    -- the release tag on disk
    latest_release       TEXT,                     -- the tag the last download named
    weights_present      INTEGER NOT NULL DEFAULT 0 CHECK (weights_present IN (0, 1)),
    last_refresh_at      TEXT,                     -- when an install last ended well
    last_error           TEXT,                     -- why the last install failed
    updated_at           TEXT NOT NULL,
    -- The flavour and device the user chose. A NULL device is filled by the
    -- daemon at startup with the best device that runs the stored flavour.
    flavour              TEXT NOT NULL DEFAULT '4b' CHECK (flavour IN ('0.8b', '4b', '9b', '27b')),
    device               TEXT CHECK (device IN ('mlx', 'cuda', 'cpu')),
    -- Whether the user set the threshold pair by hand. A pair that is not
    -- hand-set follows the default of the chosen flavour, so the stored pair
    -- is read only where this is 1.
    thresholds_hand_set  INTEGER NOT NULL DEFAULT 0 CHECK (thresholds_hand_set IN (0, 1))
);
INSERT INTO ai_permission_settings (id, updated_at)
VALUES (1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

-- Training data for the permission model: one row per user choice and per
-- denial, allow or deny, keyed by the repository, the tool name, the level
-- and the normalized input of the ACP `toolCall`.
CREATE TABLE learned_permissions (
    id              TEXT PRIMARY KEY,
    repository_id   TEXT NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    tool_name       TEXT NOT NULL,
    key             TEXT NOT NULL,              -- the kept fields of `rawInput`, with placeholders
    level           TEXT NOT NULL CHECK (level IN ('once', 'command', 'family')),
    family          TEXT NOT NULL,              -- the command family, else the tool name
    risk_tags       TEXT NOT NULL,              -- the derived risk tags, a JSON array
    scope           TEXT NOT NULL DEFAULT 'repository' CHECK (scope IN ('repository', 'all')),
    tool_call       TEXT NOT NULL,              -- the ACP `toolCall`, `rawInput` with sorted keys
    options         TEXT NOT NULL,              -- the ACP `options`
    selected_option TEXT NOT NULL,              -- the option id of the final choice
    target          TEXT NOT NULL CHECK (target IN ('auto', 'ask', 'learn', 'ai')),
    output          TEXT,                       -- the model decision, when the model was called
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);
CREATE INDEX idx_learned_permissions_repository
ON learned_permissions (repository_id, created_at DESC);
CREATE UNIQUE INDEX idx_learned_permissions_key
ON learned_permissions (repository_id, tool_name, level, key);

-- The agent of one column of a task's workflow. An agent has no identity of
-- its own: it is a model of a registry agent, an effort, a brief and a set
-- of skills, and `step` names the workflow column it works.
--
-- `ordinal` is the order the orchestrator listed them in.
CREATE TABLE task_agents (
    id      TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    step    TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    model   TEXT NOT NULL,
    effort  TEXT,
    -- What this agent is told beyond the task itself, where the orchestrator
    -- has something to add. NULL = the task is the whole of it.
    brief   TEXT,
    UNIQUE (task_id, ordinal)
);
CREATE INDEX idx_task_agents_task ON task_agents (task_id);

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
--
-- `pull_request_id` is set for a request that gets a session of its own: one
-- of the user's they asked Ariadne to review, or an open one that asks for
-- their review on a repository with a review pin. NULL on a session of a
-- task, which carries `task_id` instead.
CREATE TABLE agent_sessions (
    id                  TEXT PRIMARY KEY,       -- == ARIADNE_SESSION_ID env of the agent
    goal_id             TEXT REFERENCES goals (id) ON DELETE CASCADE,
    task_id             TEXT REFERENCES tasks (id) ON DELETE CASCADE,  -- NULL = orchestrator or loose session
    seat                TEXT CHECK (seat IN ('orchestrator', 'agent', 'reviewer')),
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
    pull_request_id     TEXT REFERENCES pull_requests (id) ON DELETE CASCADE
);
CREATE INDEX idx_sessions_task ON agent_sessions (task_id);
CREATE INDEX idx_sessions_status ON agent_sessions (status);
CREATE INDEX idx_sessions_attention ON agent_sessions (attention_reason);
-- One successor per switched session: two switches of one session race to
-- this index, and the second is refused.
CREATE UNIQUE INDEX idx_sessions_switched_from ON agent_sessions (switched_from);
CREATE INDEX agent_sessions_pull_request ON agent_sessions (pull_request_id);

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
-- One channel for everything the agents say between themselves. The
-- orchestrator and an agent, each on their own task; `from_actor` and
-- `to_actor` name which side of a row is which.
--
-- Every message has exactly one recipient, so a request that goes to three
-- recipients is three rows: `delivered_at` is per recipient, and one row with
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
    created_at    TEXT NOT NULL
);
CREATE INDEX idx_messages_task ON messages (task_id, id);
CREATE INDEX idx_messages_goal ON messages (goal_id, id);

-- What an agent reported, as the ACP runtime saw it. The rows are the bulk of
-- a database that has run for months, so a payload is stored packed and every
-- row names the codec it was packed with (`store::events`).
--
-- A session takes its events with it: an event of a deleted goal is readable
-- by nobody, and `SET NULL` kept the orchestrator's forever, since an
-- orchestrator session has no task to cascade from either.
CREATE TABLE agent_events (
    id            TEXT PRIMARY KEY,
    session_id    TEXT REFERENCES agent_sessions (id) ON DELETE CASCADE,
    task_id       TEXT REFERENCES tasks (id) ON DELETE CASCADE,
    kind          TEXT NOT NULL,                -- session_start | post_tool_use | stop | ...
    payload       BLOB NOT NULL,                -- the JSON, under payload_codec
    payload_codec TEXT NOT NULL,                -- deflate | none
    created_at    TEXT NOT NULL
);
CREATE INDEX idx_events_task ON agent_events (task_id, id);
CREATE INDEX idx_events_session ON agent_events (session_id, id);

CREATE TABLE task_transitions (
    id              TEXT PRIMARY KEY,
    task_id         TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    from_status     TEXT NOT NULL,
    to_status       TEXT NOT NULL,
    actor           TEXT NOT NULL
                    CHECK (actor IN ('orchestrator', 'agent', 'daemon', 'user')),
    reason          TEXT,
    created_at      TEXT NOT NULL,
    from_step       TEXT,
    to_step         TEXT,
    step_briefed_at TEXT
);
CREATE INDEX idx_transitions_task ON task_transitions (task_id, id);

-- The stats ledger: one row per thing that happened which tells how the tool
-- and the models perform. Append-only, and the only table a stats read
-- touches, so a stat never scans `agent_events`. `kind` names the fact
-- (`session_ended`, ...), `data` is its JSON object, and `skills` the JSON
-- array of the skills the agent behind it loaded.
--
-- No foreign keys: a fact outlives the goal, the task and the session it is
-- about, and the ids are kept as they were when it was written.
CREATE TABLE stat_facts (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    repo_id    TEXT,
    goal_id    TEXT,
    task_id    TEXT,
    session_id TEXT,
    launch_id  TEXT,
    seat       TEXT,
    model      TEXT,
    effort     TEXT,
    skills     TEXT,
    data       TEXT NOT NULL
);
CREATE INDEX idx_stat_facts_kind ON stat_facts (kind, created_at);

-- The two workflows Ariadne ships: a request lands by `develop-review-pr`,
-- everything else by `develop-review-merge`.
INSERT INTO workflows (name, document, builtin, created_at, updated_at)
VALUES ('develop-review-merge', NULL, 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
       ('develop-review-pr', NULL, 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
