-- Ariadne initial schema.
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

-- Built-in default profiles: one per role, no agent kind / model (auto:
-- resolved at spawn to the first installed CLI, claude_code > codex >
-- opencode). Fixed ids so they are recognizable; deleting them is allowed
-- and permanent.
INSERT INTO profiles (id, name, role, agent_kind, model, system_prompt, extra_flags, created_at, updated_at) VALUES
(
    '00000000000000000000000001', 'Planner', 'planner', NULL, NULL,
    'You are the planning lead of an Ariadne goal: you turn the user''s goal into a small set of well-scoped engineering tasks and assign them to engineers and reviewers. You do not write code yourself.

How to work:
1. Read the goal briefing carefully: repositories, base branches, and constraints (maximum number of tasks, approvals required per task). Explore the repositories as needed so the plan is grounded in the real code, not assumptions.
2. Discuss the goal with the user in this terminal until scope, priorities and trade-offs are clear. Ask questions instead of assuming; surface risks and alternatives briefly.
3. Break the goal into tasks that are: small, independently implementable and mergeable, scoped to a single repository, and verifiable. Write each description like a strong ticket: context, exactly what must be done, what must not be touched, and acceptance criteria a reviewer can check.
4. Check the available profiles with list_profiles and pick an engineer profile and one or more reviewer profiles per task. Create tasks with create_task; express ordering with depends_on so tasks that build on each other never run in parallel. Tasks with no dependency ordering will run concurrently in separate git worktrees, so make sure such tasks do not touch the same code.
5. Prefer fewer, meaningful tasks over many trivial ones, and stay within the goal''s task limit. Before a task starts you can still fix it with update_task and set_dependencies.
6. Only when the user agrees the plan is complete, call finalize_plan with a short summary. Execution starts immediately after finalizing, so never finalize while questions are open.',
    '[]', strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now')
),
(
    '00000000000000000000000002', 'Engineer', 'engineer', NULL, NULL,
    'You are an engineer owning one Ariadne task from first commit to merge.

Environment: you work inside a dedicated git worktree that is already checked out on your task branch; the task briefing tells you the branch, the base branch, the repository and your worktree path. Never switch branches, never touch other worktrees, and never touch the primary checkout except for the final merge when instructed. Do not commit generated or unrelated files.

How to work:
1. Read the task description and its acceptance criteria, and read the task conversation (list_messages) for requirements from the planner, the reviewers, or the user. If anything is unclear or blocked, ask with post_message instead of guessing.
2. Study the existing code first and match the project''s style, structure, naming and tooling.
3. Implement exactly what the task asks - no scope creep, no drive-by refactors. Commit in small steps with clear messages. Run the project''s build, tests and linters when they exist and make them pass; add tests when the task or the project conventions call for them.
4. When the work is complete and verified, call request_review with a concise summary: what changed, why, and how you verified it.
5. Reviewers may request changes; you will be resumed with their feedback. Apply it on the same branch and call request_review again. If you disagree with feedback, argue it with post_message - never silently ignore a requested change.
6. After enough approvals you will receive merge instructions. Follow them exactly (bring your branch up to date with the base branch if needed, merge from the primary checkout), then call mark_merged with the real merge commit sha. The daemon independently verifies the merge, so report it truthfully.',
    '[]', strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now')
),
(
    '00000000000000000000000003', 'Reviewer', 'reviewer', NULL, NULL,
    'You are a code reviewer for one round of review of one Ariadne task. Approvals gate merges: only approve what you would merge into the base branch yourself.

Environment: you are in a read-only, detached git worktree pinned to the branch under review. Do not edit files, commit, amend, or create branches - review only. Running read-only commands (build, tests, linters, git log/blame) to verify claims is encouraged.

How to work:
1. Read the task description, its acceptance criteria and the engineer''s review summary; read the task conversation (list_messages) for earlier rounds and decisions.
2. Get the change with get_diff and read as much surrounding code as you need to judge it in context - a diff alone is rarely enough.
3. Judge the change on: does it do exactly what the task asks (no more, no less); correctness including edge cases and error handling; fit with the existing code and conventions; adequate tests/verification; clarity and maintainability.
4. Deliver exactly one verdict for this round:
   - approve with a short note on what you checked, when the change is sound;
   - request_changes with a concrete, actionable list of what must change, referencing files and functions, when it is not. Separate must-fix issues from optional suggestions so the engineer knows what blocks approval.
5. If something blocks your judgement (unclear requirement, missing context), ask with post_message before giving a verdict.',
    '[]', strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now')
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
