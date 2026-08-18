-- no-transaction
-- Goals and tasks reference the global `repositories` table instead of the
-- per-goal copy `goal_repos` was: a checkout is registered once and read live
-- from there, so editing a repository moves every goal that uses it.
-- `goal_repos` becomes the join table `goal_repositories`, and `tasks.repo_id`
-- is remapped onto `repositories`.
--
-- Retargeting that foreign key means rebuilding `tasks`, and rebuilding means
-- dropping it — which, with foreign keys on, would cascade its messages,
-- reviews, sessions and reviewers away with it. So this runs the SQLite
-- table-rebuild dance: keys off, one explicit transaction, keys back on. That
-- is also why the file opts out of sqlx's own transaction above: `PRAGMA
-- foreign_keys` is a no-op inside one.

PRAGMA foreign_keys = OFF;

BEGIN;

-- 1. Every (path, base_branch) some goal used, registered globally if it is
--    not registered already. Ids are random hex rather than ULIDs: nothing
--    outside this file mints them, and all that is asked of them is that they
--    are unique TEXT.
INSERT INTO repositories (id, path, base_branch, description, created_at, updated_at)
SELECT lower(hex(randomblob(16))),
       old.path,
       old.base_branch,
       NULL,
       strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
       strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM (SELECT DISTINCT path, base_branch FROM goal_repos) AS old
WHERE NOT EXISTS (
    SELECT 1 FROM repositories r
     WHERE r.path = old.path AND r.base_branch = old.base_branch
);

-- 2. Which repositories a goal works in, by reference.
CREATE TABLE goal_repositories (
    goal_id       TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    repository_id TEXT NOT NULL REFERENCES repositories (id),
    PRIMARY KEY (goal_id, repository_id)
);
-- Deleting a repository asks who still holds it, which reads this way round.
CREATE INDEX idx_goal_repositories_repository ON goal_repositories (repository_id);

-- Two goal_repos rows of one goal could spell out the same checkout twice;
-- the join table holds one row per pair.
INSERT OR IGNORE INTO goal_repositories (goal_id, repository_id)
SELECT gr.goal_id, r.id
FROM goal_repos gr
JOIN repositories r ON r.path = gr.path AND r.base_branch = gr.base_branch;

-- 3. `tasks.repo_id` names a repository now. The rows are copied across with
--    each old `goal_repos` id swapped for the repository it became; a task
--    whose repo cannot be resolved fails the copy on NOT NULL rather than
--    quietly losing its checkout.
CREATE TABLE tasks_new (
    id                  TEXT PRIMARY KEY,
    goal_id             TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    repo_id             TEXT NOT NULL REFERENCES repositories (id),
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

INSERT INTO tasks_new (id, goal_id, repo_id, title, description, status,
                       engineer_profile_id, branch, worktree_path, review_round,
                       stalled, merge_commit, created_at, updated_at)
SELECT t.id,
       t.goal_id,
       (SELECT r.id
          FROM goal_repos gr
          JOIN repositories r ON r.path = gr.path AND r.base_branch = gr.base_branch
         WHERE gr.id = t.repo_id),
       t.title, t.description, t.status, t.engineer_profile_id, t.branch,
       t.worktree_path, t.review_round, t.stalled, t.merge_commit,
       t.created_at, t.updated_at
FROM tasks t;

DROP TABLE tasks;
ALTER TABLE tasks_new RENAME TO tasks;
CREATE INDEX idx_tasks_goal ON tasks (goal_id);
CREATE INDEX idx_tasks_status ON tasks (status);

DROP TABLE goal_repos;

COMMIT;

PRAGMA foreign_keys = ON;
