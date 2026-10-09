-- Every goal runs on a workflow. The author, reviewer and landing pipeline
-- leaves the schema, and every row written on it is mapped onto the stepped
-- one. Run with foreign keys disabled before the transaction, as 0022 was:
-- the rebuilds below drop and rename tables that child rows reference.
--
-- The two shipped workflows are written here by name so that a database
-- which never held them (one that runs 0021 to 0023 in one open, before the
-- seed) still satisfies the references this migration adds. The seed that
-- follows every open leaves a row the database holds alone.
INSERT OR IGNORE INTO workflows (name, document, builtin, created_at, updated_at)
VALUES ('develop-review-merge', NULL, 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
       ('develop-review-pr', NULL, 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

-- A repository's default landing becomes its default workflow: a request
-- lands by `develop-review-pr`, everything else by `develop-review-merge`.
CREATE TABLE repositories_wf (
    id              TEXT PRIMARY KEY,
    path            TEXT NOT NULL,
    base_branch     TEXT NOT NULL,
    description     TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    permission_mode TEXT NOT NULL DEFAULT 'auto'
                    CHECK (permission_mode IN ('auto', 'ask', 'learn', 'ai')),
    -- The workflow a new goal runs on where its request names none.
    default_workflow TEXT NOT NULL DEFAULT 'develop-review-merge' REFERENCES workflows (name),
    UNIQUE (path, base_branch)
);
INSERT INTO repositories_wf (id, path, base_branch, description, created_at, updated_at,
                             permission_mode, default_workflow)
SELECT id, path, base_branch, description, created_at, updated_at, permission_mode,
       COALESCE(default_workflow,
                CASE default_landing WHEN 'pull_request' THEN 'develop-review-pr'
                                     ELSE 'develop-review-merge' END)
  FROM repositories;
DROP TABLE repositories;
ALTER TABLE repositories_wf RENAME TO repositories;

-- A goal's landing becomes its workflow the same way, and the column is
-- required from here on.
CREATE TABLE goals_wf (
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
    workflow     TEXT NOT NULL REFERENCES workflows (name)
);
INSERT INTO goals_wf (id, title, description, status, orchestrated, created_at, updated_at,
                      model, effort, issue_url, workflow)
SELECT id, title, description, status, orchestrated, created_at, updated_at, model, effort,
       issue_url,
       COALESCE(workflow,
                CASE landing WHEN 'pull_request' THEN 'develop-review-pr'
                             ELSE 'develop-review-merge' END)
  FROM goals;
DROP TABLE goals;
ALTER TABLE goals_wf RENAME TO goals;

-- A goal that ran on the old path has no columns yet: it gets the columns
-- of the shipped document its workflow names, as a goal created today
-- would. One statement, so every goal lacking steps is read before any
-- step is written.
INSERT INTO goal_steps (goal_id, ordinal, id, title, description, skills, rank, gate)
SELECT g.id, 0, 'develop', 'Develop', 'Build the task on its branch and commit it.',
       '["coding"]', 'balanced', 'committed'
  FROM goals g
 WHERE NOT EXISTS (SELECT 1 FROM goal_steps s WHERE s.goal_id = g.id)
UNION ALL
SELECT g.id, 1, 'review', 'Review',
       'Run the whole suite and judge the change against the task and the repository rules. Fail the step with the changes to make.',
       '["code-review"]', 'frontier', NULL
  FROM goals g
 WHERE NOT EXISTS (SELECT 1 FROM goal_steps s WHERE s.goal_id = g.id)
UNION ALL
SELECT g.id, 2,
       CASE g.workflow WHEN 'develop-review-pr' THEN 'pr' ELSE 'merge' END,
       CASE g.workflow WHEN 'develop-review-pr' THEN 'Pull request' ELSE 'Merge' END,
       CASE g.workflow
            WHEN 'develop-review-pr'
            THEN 'Push the branch, open the request and keep it until a human merges or closes it.'
            ELSE 'Rebase onto the base branch, run the whole suite, squash, fast-forward and push.'
       END,
       CASE g.workflow WHEN 'develop-review-pr' THEN '["pr-babysit"]' ELSE '["merge"]' END,
       CASE g.workflow WHEN 'develop-review-pr' THEN 'balanced' ELSE 'fast' END,
       CASE g.workflow WHEN 'develop-review-pr' THEN 'request_merged' ELSE 'merged' END
  FROM goals g
 WHERE NOT EXISTS (SELECT 1 FROM goal_steps s WHERE s.goal_id = g.id);

-- A goal owns no branch of its own any more.
CREATE TABLE goal_repositories_wf (
    goal_id       TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    repository_id TEXT NOT NULL REFERENCES repositories (id),
    PRIMARY KEY (goal_id, repository_id)
);
INSERT INTO goal_repositories_wf (goal_id, repository_id)
SELECT goal_id, repository_id FROM goal_repositories;
DROP TABLE goal_repositories;
ALTER TABLE goal_repositories_wf RENAME TO goal_repositories;
CREATE INDEX idx_goal_repositories_repository ON goal_repositories (repository_id);

-- The six statuses a task moves through now. A task that stood in a review
-- status, or with an author on it, on a goal of the old path is failed: the
-- agents that held it are gone, and a retry starts it on the first column.
-- A task already on a workflow keeps running. The picked author goes with
-- the pick.
CREATE TABLE tasks_wf (
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
INSERT INTO tasks_wf (id, goal_id, repo_id, title, description, status, branch, worktree_path,
                      stalled, merge_commit, pr_url, created_at, updated_at, step)
SELECT t.id, t.goal_id, t.repo_id, t.title, t.description,
       CASE WHEN t.status IN ('ready', 'in_progress', 'under_review', 'changes_requested',
                              'approved')
                 AND NOT EXISTS (SELECT 1 FROM task_agents a
                                  WHERE a.task_id = t.id AND a.seat = 'agent')
            THEN 'failed' ELSE t.status END,
       t.branch, t.worktree_path, t.stalled, t.merge_commit, t.pr_url, t.created_at,
       t.updated_at, t.step
  FROM tasks t;

-- One transition row per task failed above, by the daemon, dated now. Its id
-- is a ULID built here: the time half from the clock, so the row sorts after
-- every transition before it and before every one written later, and the
-- random half from SQLite.
INSERT INTO task_transitions (id, task_id, from_status, to_status, actor, reason, created_at,
                              from_step, to_step)
SELECT substr('0123456789abcdefghjkmnpqrstvwxyz', ((ms >> 45) & 31) + 1, 1)
    || substr('0123456789abcdefghjkmnpqrstvwxyz', ((ms >> 40) & 31) + 1, 1)
    || substr('0123456789abcdefghjkmnpqrstvwxyz', ((ms >> 35) & 31) + 1, 1)
    || substr('0123456789abcdefghjkmnpqrstvwxyz', ((ms >> 30) & 31) + 1, 1)
    || substr('0123456789abcdefghjkmnpqrstvwxyz', ((ms >> 25) & 31) + 1, 1)
    || substr('0123456789abcdefghjkmnpqrstvwxyz', ((ms >> 20) & 31) + 1, 1)
    || substr('0123456789abcdefghjkmnpqrstvwxyz', ((ms >> 15) & 31) + 1, 1)
    || substr('0123456789abcdefghjkmnpqrstvwxyz', ((ms >> 10) & 31) + 1, 1)
    || substr('0123456789abcdefghjkmnpqrstvwxyz', ((ms >> 5) & 31) + 1, 1)
    || substr('0123456789abcdefghjkmnpqrstvwxyz', (ms & 31) + 1, 1)
    || lower(hex(randomblob(8))),
       t.id, t.status, 'failed', 'daemon', 'replaced by workflows, retry it',
       strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), t.step, NULL
  FROM tasks t,
       (SELECT CAST(strftime('%s', 'now') AS INTEGER) * 1000 AS ms)
 WHERE t.status IN ('ready', 'in_progress', 'under_review', 'changes_requested', 'approved')
   AND NOT EXISTS (SELECT 1 FROM task_agents a WHERE a.task_id = t.id AND a.seat = 'agent');

DROP TABLE task_picks;
DROP TABLE tasks;
ALTER TABLE tasks_wf RENAME TO tasks;
CREATE INDEX idx_tasks_goal ON tasks (goal_id);
CREATE INDEX idx_tasks_status ON tasks (status);

-- Every agent of a task is the agent of one column. An author becomes the
-- agent of `develop` and a reviewer the agent of `review`, each kept on its
-- own row; the ordinal is the order the orchestrator listed them in, authors
-- first. A task of a `pull_request` goal gets no agent on `pr`: the
-- orchestrator staffs it before a retry.
CREATE TABLE task_agents_wf (
    id      TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    -- The column of the task's workflow this agent works.
    step    TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    model   TEXT NOT NULL,
    effort  TEXT,
    -- What this agent is told beyond the task itself, where the orchestrator
    -- has something to add. NULL = the task is the whole of it.
    brief   TEXT,
    UNIQUE (task_id, ordinal)
);
INSERT INTO task_agents_wf (id, task_id, step, ordinal, model, effort, brief)
SELECT id, task_id,
       CASE seat WHEN 'author' THEN 'develop'
                 WHEN 'reviewer' THEN 'review'
                 ELSE COALESCE(step, 'develop') END,
       ROW_NUMBER() OVER (PARTITION BY task_id ORDER BY seat = 'reviewer', ordinal, id) - 1,
       model, effort, brief
  FROM task_agents;
DROP TABLE task_agents;
ALTER TABLE task_agents_wf RENAME TO task_agents;
CREATE INDEX idx_task_agents_task ON task_agents (task_id);

-- Every author session, and every reviewer session of a task, becomes the
-- session of a column's agent. A reviewer session of a pull request (029)
-- sits on no task and keeps its seat.
CREATE TABLE agent_sessions_wf (
    id                  TEXT PRIMARY KEY,
    goal_id             TEXT REFERENCES goals (id) ON DELETE CASCADE,
    task_id             TEXT REFERENCES tasks (id) ON DELETE CASCADE,
    seat                TEXT CHECK (seat IN ('orchestrator', 'agent', 'reviewer')),
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
INSERT INTO agent_sessions_wf (id, goal_id, task_id, seat, task_agent_id, internal_session_id,
                               worktree_path, status, last_activity_at, created_at, ended_at,
                               attention_reason, attention_since, model, effort, context_used,
                               context_size, launched_at, launch_id, title, switched_from,
                               pull_request_id)
SELECT id, goal_id, task_id,
       CASE WHEN seat = 'author' THEN 'agent'
            WHEN seat = 'reviewer' AND task_id IS NOT NULL THEN 'agent'
            ELSE seat END,
       task_agent_id, internal_session_id, worktree_path, status, last_activity_at, created_at,
       ended_at, attention_reason, attention_since, model, effort, context_used, context_size,
       launched_at, launch_id, title, switched_from, pull_request_id
  FROM agent_sessions;
DROP TABLE agent_sessions;
ALTER TABLE agent_sessions_wf RENAME TO agent_sessions;
CREATE INDEX idx_sessions_task ON agent_sessions (task_id);
CREATE INDEX idx_sessions_status ON agent_sessions (status);
CREATE INDEX idx_sessions_attention ON agent_sessions (attention_reason);
CREATE UNIQUE INDEX idx_sessions_switched_from ON agent_sessions (switched_from);
CREATE INDEX agent_sessions_pull_request ON agent_sessions (pull_request_id);

-- One kind of message is left. A review request and the two verdicts become
-- messages that open with their old kind in brackets, so what was said stays
-- readable; the author and the reviewer that said it are agents.
CREATE TABLE messages_wf (
    id            TEXT PRIMARY KEY,
    goal_id       TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    task_id       TEXT REFERENCES tasks (id) ON DELETE CASCADE,
    kind          TEXT NOT NULL CHECK (kind IN ('message')),
    from_actor    TEXT NOT NULL
                  CHECK (from_actor IN ('orchestrator', 'agent', 'daemon', 'user')),
    from_agent_id TEXT REFERENCES task_agents (id) ON DELETE CASCADE,
    from_session  TEXT REFERENCES agent_sessions (id) ON DELETE SET NULL,
    to_actor      TEXT NOT NULL
                  CHECK (to_actor IN ('orchestrator', 'agent', 'daemon', 'user')),
    to_agent_id   TEXT REFERENCES task_agents (id) ON DELETE CASCADE,
    body          TEXT NOT NULL,
    delivered_at  TEXT,
    created_at    TEXT NOT NULL
);
INSERT INTO messages_wf (id, goal_id, task_id, kind, from_actor, from_agent_id, from_session,
                         to_actor, to_agent_id, body, delivered_at, created_at)
SELECT id, goal_id, task_id, 'message',
       CASE WHEN from_actor IN ('author', 'reviewer') THEN 'agent' ELSE from_actor END,
       from_agent_id, from_session,
       CASE WHEN to_actor IN ('author', 'reviewer') THEN 'agent' ELSE to_actor END,
       to_agent_id,
       CASE WHEN kind = 'message' THEN body ELSE '[' || kind || '] ' || body END,
       delivered_at, created_at
  FROM messages;
DROP TABLE messages;
ALTER TABLE messages_wf RENAME TO messages;
CREATE INDEX idx_messages_task ON messages (task_id, id);
CREATE INDEX idx_messages_goal ON messages (goal_id, id);

-- The actor of a transition is one of the four left. The statuses a row
-- moved between are history and stay as they were written.
CREATE TABLE task_transitions_wf (
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
INSERT INTO task_transitions_wf (id, task_id, from_status, to_status, actor, reason, created_at,
                                 from_step, to_step, step_briefed_at)
SELECT id, task_id, from_status, to_status,
       CASE WHEN actor IN ('author', 'reviewer') THEN 'agent' ELSE actor END,
       reason, created_at, from_step, to_step, step_briefed_at
  FROM task_transitions;
DROP TABLE task_transitions;
ALTER TABLE task_transitions_wf RENAME TO task_transitions;
CREATE INDEX idx_transitions_task ON task_transitions (task_id, id);
