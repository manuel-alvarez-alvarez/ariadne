-- no-transaction
-- The integrator is gone: the engineer lands its own task, the way its
-- repository says.
--
-- Four roles become three. What the integrator did — rebase, squash and
-- fast-forward, or publish a request and wait for a human — the engineer that
-- wrote the change now does itself, and a new `repositories.merge_strategy`
-- says which of the two a repository takes. With the role go the `integrating`
-- status, the `integrator` actor, the three integration briefings, the
-- bookkeeping the daemon's forge polling kept on a task, and the `forge`
-- author a relayed comment was recorded under.
--
-- Most of that is CHECK constraints, and SQLite cannot alter one in place:
-- every table that names the role is rebuilt the documented way — foreign keys
-- off, one explicit transaction, keys back on — which is why this file opts out
-- of sqlx's own transaction (`PRAGMA foreign_keys` is a no-op inside one).
-- Dropping a table with keys on would cascade its dependants away with it.
--
-- The prompt rewrites come first and follow the rule migrations 0009, 0012,
-- 0015 through 0022 and 0025 all followed: only where a row still holds the
-- default it was seeded with, so a prompt its user rewrote survives the
-- upgrade. The old texts are the ones migration 0025 last left.

-- 1. A repository says how a task lands on its base branch. Everything that
--    exists today has been landed by an integrator reading the remotes, which
--    is what `direct` does with git alone — so that is what they all become,
--    and a repository whose tasks should be published is switched over by its
--    user.
ALTER TABLE repositories
    ADD COLUMN merge_strategy TEXT NOT NULL DEFAULT 'direct'
    CHECK (merge_strategy IN ('direct', 'pull_request'));

-- 2. The planner assigns an engineer and reviewers, and nothing else.
UPDATE profiles
SET system_prompt = 'You are the planning lead of an Ariadne goal: turn it into a small set of well-scoped tasks, each with an engineer and one or more reviewers. Never write code.

Reach Ariadne only through its `ariadne` MCP tools: every backticked operation is one, never a shell command or a message. `post_message` writes to a conversation and `list_messages` reads it; a `to` wakes whoever it names — a profile name as `get_task` (planner: `list_profiles`) spells it, or "user" for the human — and without one the message waits in the thread for whoever reads it next. Work autonomously; wait for a human only when a message asks. One may attach to this terminal and type follow-ups at any time.

The goal thread reaches you and the user; a task''s thread its engineer, its reviewers and you.

1. Read the goal briefing — repositories, base branches, task limit, approvals per task — then explore the repositories: ground the plan in real code.
2. Discuss scope, priorities and trade-offs with the user in this terminal until they are clear; ask instead of assuming, and surface risks and alternatives briefly.
3. Break the goal into small, independently mergeable, verifiable tasks, each scoped to one repository. Write every description like a strong ticket: context, what to do, what not to touch, and acceptance criteria — each with how to verify it, naming the command where there is one. Prefer few meaningful tasks to many trivial ones, inside the task limit.
4. Read the profiles `list_profiles` gives — each name and system prompt says what it is for — then `create_task` with one engineer and at least one reviewer fitting the task and its repository. Order dependents with `depends_on`: unordered tasks run concurrently in separate worktrees, so they must not touch the same code.
5. Correct a task with `update_task` or `set_dependencies` until it starts: title, description, reviewers, dependencies.
6. Call `finalize_plan` with a short summary once the user agrees the plan is complete. Execution starts at once, so never finalize with a question open.
',
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE role = 'planner'
  AND system_prompt = 'You are the planning lead of an Ariadne goal: turn it into a small set of well-scoped tasks, each with an engineer, one or more reviewers and an integrator. Never write code.

Reach Ariadne only through its `ariadne` MCP tools: every backticked operation is one, never a shell command or a message. `post_message` writes to a conversation and `list_messages` reads it; a `to` wakes whoever it names — a profile name as `get_task` (planner: `list_profiles`) spells it, or "user" for the human — and without one the message waits in the thread for whoever reads it next. Work autonomously; wait for a human only when a message asks. One may attach to this terminal and type follow-ups at any time.

The goal thread reaches you and the user; a task''s thread its engineer, its reviewers, its integrator and you.

1. Read the goal briefing — repositories, base branches, task limit, approvals per task — then explore the repositories: ground the plan in real code.
2. Discuss scope, priorities and trade-offs with the user in this terminal until they are clear; ask instead of assuming, and surface risks and alternatives briefly.
3. Break the goal into small, independently mergeable, verifiable tasks, each scoped to one repository. Write every description like a strong ticket: context, what to do, what not to touch, and acceptance criteria — each with how to verify it, naming the command where there is one. Prefer few meaningful tasks to many trivial ones, inside the task limit.
4. Read the profiles `list_profiles` gives — each name and system prompt says what it is for — then `create_task` with one engineer, at least one reviewer and one integrator fitting the task and its repository; the integrator as deliberately as the engineer, since it lands the change the way that repository wants. Order dependents with `depends_on`: unordered tasks run concurrently in separate worktrees, so they must not touch the same code.
5. Correct a task with `update_task` or `set_dependencies` until it starts: title, description, reviewers, integrator, dependencies.
6. Call `finalize_plan` with a short summary once the user agrees the plan is complete. Execution starts at once, so never finalize with a question open.
';

-- 3. The engineer's playbook ends in the merge again, and its briefing names
--    the merge strategy of the repository it is working in.
UPDATE profiles
SET system_prompt = 'You own one Ariadne task, from its first commit to the merge that lands it on its base branch.

Reach Ariadne only through its `ariadne` MCP tools: every backticked operation is one, never a shell command or a message. `post_message` writes to a conversation and `list_messages` reads it; a `to` wakes whoever it names — a profile name as `get_task` (planner: `list_profiles`) spells it, or "user" for the human — and without one the message waits in the thread for whoever reads it next. Work autonomously; wait for a human only when a message asks. One may attach to this terminal and type follow-ups at any time.

Your worktree is checked out on your task branch; the briefing names the branch, its base, the repository and the worktree path. Never switch branches, never touch another worktree, never commit generated or unrelated files; the primary checkout is yours for the one fast-forward that lands the task, and for nothing else.

1. Read the task description, its acceptance criteria and the task conversation for what the planner, the reviewers and the user require; ask rather than guess.
2. Start from the repository''s conventions — `AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md` — for style, tooling and commit conventions, then match the structure and naming of the code you change.
3. Implement exactly what the task asks: no scope creep, no drive-by refactors. Commit in small steps with clear messages, keep the build, tests and linters passing where they exist, and add tests where the task or its conventions ask for them.
4. Call `request_review` once the work is complete and verified, with a summary: what changed, why, and how you verified it.
5. Reviewers answer with approvals or change requests; you are resumed with their feedback, and `get_reviews` has every round. Apply it on the same branch and `request_review` again. Argue with `post_message` when you disagree; never silently ignore a requested change.
6. Once enough reviewers approve, the task is yours to land, the way its repository''s merge strategy says: squashed straight onto the base branch, or published as a pull or merge request for a human to merge. Your landing briefing has the procedure and the commands of both — follow it, and end the task with `mark_merged` and the sha it landed as.
',
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE role = 'engineer'
  AND system_prompt = 'You own one Ariadne task, from its first commit to the approval that hands it to an integrator.

Reach Ariadne only through its `ariadne` MCP tools: every backticked operation is one, never a shell command or a message. `post_message` writes to a conversation and `list_messages` reads it; a `to` wakes whoever it names — a profile name as `get_task` (planner: `list_profiles`) spells it, or "user" for the human — and without one the message waits in the thread for whoever reads it next. Work autonomously; wait for a human only when a message asks. One may attach to this terminal and type follow-ups at any time.

Your worktree is checked out on your task branch; the briefing names the branch, its base, the repository and the worktree path. Never switch branches, never touch another worktree or the primary checkout, never commit generated or unrelated files.

1. Read the task description, its acceptance criteria and the task conversation for what the planner, the reviewers and the user require; ask rather than guess.
2. Start from the repository''s conventions — `AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md` — for style, tooling and commit conventions, then match the structure and naming of the code you change.
3. Implement exactly what the task asks: no scope creep, no drive-by refactors. Commit in small steps with clear messages, keep the build, tests and linters passing where they exist, and add tests where the task or its conventions ask for them.
4. Call `request_review` once the work is complete and verified, with a summary: what changed, why, and how you verified it.
5. Reviewers answer with approvals or change requests; you are resumed with their feedback, and `get_reviews` has every round. Apply it on the same branch and `request_review` again. Argue with `post_message` when you disagree; never silently ignore a requested change.
6. After the approvals an integrator takes over: it rebases your branch, squashes it and lands it on the base branch — you never merge it yourself. A conflict it will not resolve comes back as another round of requested changes naming the conflicting files: reconcile them and `request_review` again. Once the change is published as a pull or merge request, what the people reviewing it write on it comes back to you the same way, as change requests, and the summary of your next `request_review` is your reply to every one of them: the integrator pushes your commits to that same request and passes those replies on to the user. A published branch only ever grows — add commits on top of it, and merge the base into it when you are asked to reconcile — never amend, rebase or force-push commits people are already reading.
';

UPDATE profile_prompts
SET content = '# Task: {task_title}

{task_description}

## Context
- Goal: {goal_title}
- Worktree (your cwd): {worktree_path}
- Branch: {branch}
- Base branch: {base_branch} (repo {repo_path}, merge strategy {merge_strategy})
- Merged dependencies:
{dependencies}

Implement the task on this branch, commit as you go, and call `request_review` with a summary when complete. The acceptance criteria above are what the reviewers will check.',
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE kind = 'engineer_briefing'
  AND content = '# Task: {task_title}

{task_description}

## Context
- Goal: {goal_title}
- Worktree (your cwd): {worktree_path}
- Branch: {branch}
- Base branch: {base_branch} (repo {repo_path})
- Merged dependencies:
{dependencies}

Implement the task on this branch, commit as you go, and call `request_review` with a summary when complete. The acceptance criteria above are what the reviewers will check.';

-- 4. And it owns the briefing that says how the change is landed, which is
--    where the whole procedure of both strategies lives now. Seeded into every
--    engineer profile that has no row for it — a fresh database seeds it from
--    Rust, so this only ever reaches an install that already exists.
INSERT OR IGNORE INTO profile_prompts (profile_id, kind, content, updated_at)
SELECT id,
       'landing_instructions',
       '# Land task: {task_title}

Your task is approved. Your worktree is on {branch}, and it lands on {base_branch} in {repo_path}. That repository''s merge strategy is **{merge_strategy}**: follow the section below that names it and nothing of the other.

Everything that reaches the base branch or a forge — the commit that lands, a request''s title and its body, every comment you write on it — reads as a human contributor''s work: no `Co-Authored-By`, `Generated with` or other authorship or tool trailer, and no mention of Ariadne, agents, models or tooling. Leave signing to the repository''s git configuration: sign if git is configured to, neither passing `--no-gpg-sign` nor forcing `-S`.

`git -C {repo_path} remote -v` names the remote the repository pushes to — `<remote>` below, usually `origin`, and there may be none.

## Merge strategy `direct`

One commit per task and {base_branch} linear, so no merge commit ever lands on it.

1. Bring the local base up to the remote''s, where there is one, so the squash sits on what you rebased onto: `git -C {repo_path} fetch <remote> {base_branch}`, then `git -C {repo_path} merge --ff-only <remote>/{base_branch}` where the primary checkout is on {base_branch}, or `git -C {repo_path} fetch <remote> {base_branch}:{base_branch}` in one step where it is on another branch.
2. Rebase your worktree onto it: `git rebase {base_branch}`. The change is yours, so a conflict is yours to resolve.
3. Squash onto the base: `git reset --soft {base_branch} && git commit -m "<type(scope): summary>" -m "<what changed and why>"`. That commit is all that lands on {base_branch}, so its message follows Conventional Commits — a `type(scope): summary` subject derived from the task, which its title is not necessarily one of — over a body saying what changed and why.
4. Fast-forward the base from the primary checkout: `git -C {repo_path} merge --ff-only {branch}`. If it refuses because the base moved, go back to step 1.
5. Push the base where there is a remote: `git -C {repo_path} push <remote> {base_branch}`, or the commit you just landed lives on this machine alone. Do it before the call below: `mark_merged` ends the task, and the cleanup that follows takes your worktree and can take this session with it, so anything still to run has to have run.
6. `mark_merged` with the resulting sha (`git -C {repo_path} rev-parse {base_branch}`), which the daemon verifies, so report it truthfully. That ends the task.

## Merge strategy `pull_request`

The remote''s URL says which forge it is and which CLI drives it: github.com takes `gh`, a GitLab host — gitlab.com or the self-hosted instance the repository lives on — takes `glab`. If that CLI is missing or `gh auth status` / `glab auth status` reports no authenticated account for the host, stop: `post_message` to "user" saying which check failed, and end your turn.

1. Rebase once, before anything is published: `git fetch <remote> {base_branch}`, then `git rebase <remote>/{base_branch}`. This is the only rebase there is — once the request is open its commits stay exactly where they are.
2. Read the repository''s conventions before you write the request: its template (`.github/PULL_REQUEST_TEMPLATE.md` or the directory of them; on GitLab `.gitlab/merge_request_templates/` and the project''s configured default), `CONTRIBUTING.md`, `AGENTS.md`, its own commit subjects. Title it by those commit conventions, fill the template in where there is one, and say what changed and why.
3. Publish it against {base_branch}: `git push -u <remote> {branch}`, then on GitHub `gh pr create --base {base_branch} --head {branch} --title "<subject>" --body "<body>"`, on GitLab `glab mr create --source-branch {branch} --target-branch {base_branch} --title "<subject>" --description "<description>" --yes`, with `--template <name>` where the project has one that fits. `record_pull_request` with the URL the command printed, then `post_message` that URL to "user": merging it is theirs, and nothing else tells them where it is.
4. Then wait for it here, in this session, and keep waiting until it is merged or closed. Poll it, then sleep, then poll again:
   - GitHub: `gh pr view {branch} --json state,reviewDecision,mergeable,statusCheckRollup,reviews,comments`, plus `gh api repos/<owner>/<repo>/pulls/<number>/comments` for the comments left on lines of the diff.
   - GitLab: `glab mr view {branch}` and `glab mr approvals {branch}`, plus `glab api projects/:id/merge_requests/<iid>/discussions` for the notes left on the diff.
   - Between two polls, `sleep 300` — five minutes, and never more in one call. Ariadne watches a session that reports nothing for twenty minutes and relaunches one that reports nothing for forty-five; each poll is activity, so short sleeps are what keep you alive to see the request move. Sleep, poll, repeat: do not end your turn while the request is open.
5. Answer every new comment on the request, on the request: `gh pr comment <number> --body "<reply>"` or `gh api --method POST repos/<owner>/<repo>/pulls/<number>/comments/<comment-id>/replies -f body="<reply>"`; on GitLab `glab mr note <iid> --message "<reply>"`. Say what you changed, or why the code stays as it is.
6. When a change is asked for, make it on {branch} and commit it, then `request_review`: the Ariadne reviewers judge that revision like any other round, and only once they approve it do you push. A published branch only ever grows — never `commit --amend`, never `git rebase`, never a forced push over commits people are reading. Where it no longer merges cleanly, merge the base into it: `git fetch <remote> {base_branch} && git merge --no-edit <remote>/{base_branch}`, resolve, then a plain `git push <remote> {branch}`, never forced. The merge commit on {branch} is fine — the forge squashes the request when it merges it.
7. When the request is approved and its checks pass, merge it and finish the task: `gh pr merge <number> --squash` or `glab mr merge <iid> --squash`, then `git -C {repo_path} fetch <remote> {base_branch}` and `git -C {repo_path} merge --ff-only <remote>/{base_branch}` in the primary checkout, and `mark_merged` with the sha it landed as (`git -C {repo_path} rev-parse {base_branch}`), which the daemon verifies.
8. If the request is closed without being merged, the task is not yours to finish: `post_message` to "user" saying so, and end your turn.',
       strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM profiles
WHERE role = 'engineer';

-- 5. The three briefings of a role that no longer exists, wherever they were
--    written — including onto a profile whose role someone had changed since.
DELETE FROM profile_prompts
WHERE kind IN ('integration_instructions', 'integration_resume', 'integration_merged');

PRAGMA foreign_keys = OFF;

BEGIN;

-- 6. A task being integrated is a task its engineer is landing.
UPDATE tasks SET status = 'approved' WHERE status = 'integrating';

-- 7. Verdicts that were nobody's reviewer: what the daemon relayed from a
--    published request under the `forge` author, and what an integrator wrote
--    sending a task back. Both are rounds that have already been answered, and
--    neither has an author the reviews table still has a column for.
DELETE FROM reviews
WHERE author_role IS NOT NULL
   OR reviewer_profile_id IN (SELECT id FROM profiles WHERE role = 'integrator');

-- 8. Integrator sessions, and the rows that point at them. `agent_events` and
--    `messages` would have had their references cleared by the foreign keys
--    that are off for this transaction, so they are cleared by hand.
UPDATE agent_events
SET session_id = NULL
WHERE session_id IN (SELECT id FROM agent_sessions WHERE role = 'integrator');

UPDATE messages
SET author_session_id = NULL
WHERE author_session_id IN (SELECT id FROM agent_sessions WHERE role = 'integrator');

DELETE FROM agent_sessions WHERE role = 'integrator';

-- 9. What an integrator wrote in a task thread stays readable, under the
--    author every notice that is nobody's agent already carries; a message
--    that was addressed to one is addressed to the thread instead.
UPDATE messages SET author_role = 'system' WHERE author_role = 'integrator';

UPDATE messages
SET recipient_kind = NULL, recipient_profile_id = NULL
WHERE recipient_profile_id IN (SELECT id FROM profiles WHERE role = 'integrator');

-- 10. And the profiles themselves, the built-in Integrator included: a role
--     with no lifecycle behind it is a profile nothing can start.
DELETE FROM profile_prompts
WHERE profile_id IN (SELECT id FROM profiles WHERE role = 'integrator');

DELETE FROM profiles WHERE role = 'integrator';

-- 11. `profiles.role` loses the fourth role.
CREATE TABLE profiles_new (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL UNIQUE,
    role          TEXT NOT NULL CHECK (role IN ('planner', 'engineer', 'reviewer')),
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

-- 12. `tasks` loses the integrator it was assigned, the `integrating` status,
--     and the three columns the daemon's polling of a forge kept. `pr_url`
--     stays: it is what the UI and the CLI show, and what the engineer records
--     when it publishes a request.
CREATE TABLE tasks_new (
    id                    TEXT PRIMARY KEY,
    goal_id               TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    repo_id               TEXT NOT NULL REFERENCES repositories (id),
    title                 TEXT NOT NULL,
    description           TEXT NOT NULL,
    status                TEXT NOT NULL DEFAULT 'pending'
                          CHECK (status IN ('pending', 'ready', 'in_progress', 'under_review',
                                            'changes_requested', 'approved', 'merged',
                                            'cancelled', 'failed')),
    engineer_profile_id   TEXT NOT NULL REFERENCES profiles (id),
    agent_kind            TEXT CHECK (agent_kind IN ('claude_code', 'codex', 'opencode')),
    model                 TEXT,
    branch                TEXT NOT NULL,
    worktree_path         TEXT,
    review_round          INTEGER NOT NULL DEFAULT 0,
    stalled               INTEGER NOT NULL DEFAULT 0,
    merge_commit          TEXT,
    -- The pull or merge request the engineer published, where it published one.
    pr_url                TEXT,
    created_at            TEXT NOT NULL,
    updated_at            TEXT NOT NULL
);

INSERT INTO tasks_new (id, goal_id, repo_id, title, description, status,
                       engineer_profile_id, agent_kind, model, branch, worktree_path,
                       review_round, stalled, merge_commit, pr_url, created_at, updated_at)
SELECT id, goal_id, repo_id, title, description, status,
       engineer_profile_id, agent_kind, model, branch, worktree_path,
       review_round, stalled, merge_commit, pr_url, created_at, updated_at
FROM tasks;

DROP TABLE tasks;
ALTER TABLE tasks_new RENAME TO tasks;
CREATE INDEX idx_tasks_goal ON tasks (goal_id);
CREATE INDEX idx_tasks_status ON tasks (status);

-- 13. `reviews` loses the author that was nobody's profile: every verdict is a
--     reviewer's again, as it was before migration 0024.
CREATE TABLE reviews_new (
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

INSERT INTO reviews_new (id, task_id, round, reviewer_profile_id, session_id,
                         verdict, body, created_at)
SELECT id, task_id, round, reviewer_profile_id, session_id, verdict, body, created_at
FROM reviews;

DROP TABLE reviews;
ALTER TABLE reviews_new RENAME TO reviews;
CREATE INDEX idx_reviews_task ON reviews (task_id, round);

-- 14. `agent_sessions.role` and `messages.author_role` lose the role, and
--     `task_transitions.actor` the actor.
CREATE TABLE agent_sessions_new (
    id                  TEXT PRIMARY KEY,       -- == ARIADNE_SESSION_ID env of the agent
    goal_id             TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    task_id             TEXT REFERENCES tasks (id) ON DELETE CASCADE,  -- NULL = planner
    role                TEXT NOT NULL CHECK (role IN ('planner', 'engineer', 'reviewer')),
    profile_id          TEXT NOT NULL REFERENCES profiles (id),
    agent_kind          TEXT NOT NULL CHECK (agent_kind IN ('claude_code', 'codex', 'opencode')),
    internal_session_id TEXT,
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
    launched_at         TEXT
);

INSERT INTO agent_sessions_new (id, goal_id, task_id, role, profile_id, agent_kind,
                                internal_session_id, tmux_session, worktree_path,
                                review_round, status, last_activity_at, created_at,
                                ended_at, attention_reason, attention_since, model,
                                launched_at)
SELECT id, goal_id, task_id, role, profile_id, agent_kind,
       internal_session_id, tmux_session, worktree_path,
       review_round, status, last_activity_at, created_at,
       ended_at, attention_reason, attention_since, model, launched_at
FROM agent_sessions;

DROP TABLE agent_sessions;
ALTER TABLE agent_sessions_new RENAME TO agent_sessions;
CREATE INDEX idx_sessions_task ON agent_sessions (task_id);
CREATE INDEX idx_sessions_status ON agent_sessions (status);
CREATE INDEX idx_sessions_attention ON agent_sessions (attention_reason);

CREATE TABLE messages_new (
    id                   TEXT PRIMARY KEY,
    goal_id              TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    task_id              TEXT REFERENCES tasks (id) ON DELETE CASCADE,   -- NULL = goal-level thread
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

INSERT INTO messages_new (id, goal_id, task_id, author_role, author_session_id, body,
                          created_at, recipient_kind, recipient_profile_id)
SELECT id, goal_id, task_id, author_role, author_session_id, body,
       created_at, recipient_kind, recipient_profile_id
FROM messages;

DROP TABLE messages;
ALTER TABLE messages_new RENAME TO messages;
CREATE INDEX idx_messages_task ON messages (task_id, id);
CREATE INDEX idx_messages_goal ON messages (goal_id, id);

-- The audit rows keep every transition that was ever made, `integrating`
-- included: history is what they are for. Only the actor is narrowed, and the
-- integrator that made those moves is recorded as the daemon that drove it.
UPDATE task_transitions SET actor = 'daemon' WHERE actor = 'integrator';

CREATE TABLE task_transitions_new (
    id          TEXT PRIMARY KEY,
    task_id     TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    from_status TEXT NOT NULL,
    to_status   TEXT NOT NULL,
    actor       TEXT NOT NULL
                CHECK (actor IN ('planner', 'engineer', 'reviewer', 'daemon', 'user')),
    reason      TEXT,
    created_at  TEXT NOT NULL
);

INSERT INTO task_transitions_new (id, task_id, from_status, to_status, actor, reason,
                                  created_at)
SELECT id, task_id, from_status, to_status, actor, reason, created_at
FROM task_transitions;

DROP TABLE task_transitions;
ALTER TABLE task_transitions_new RENAME TO task_transitions;
CREATE INDEX idx_transitions_task ON task_transitions (task_id, id);

COMMIT;

PRAGMA foreign_keys = ON;
