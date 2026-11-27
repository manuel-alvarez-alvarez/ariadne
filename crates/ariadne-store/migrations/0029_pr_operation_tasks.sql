-- A pull request review or babysit is one goal-less task (029, 032): the
-- task names its request by value here, as the goal did in `goals.pr_*`, so
-- the link outlives the request's own row. `open` is 1 while the operation
-- is Ariadne's work on the request, failed task included, until it
-- completes, is stopped or is replaced by a new ask; one open row exists per
-- request and operation.
CREATE TABLE task_pull_requests (
    task_id       TEXT PRIMARY KEY REFERENCES tasks (id) ON DELETE CASCADE,
    operation     TEXT NOT NULL CHECK (operation IN ('review', 'babysit')),
    repository_id TEXT NOT NULL,
    number        INTEGER NOT NULL,
    url           TEXT NOT NULL,
    open          INTEGER NOT NULL DEFAULT 1 CHECK (open IN (0, 1))
);

CREATE UNIQUE INDEX idx_task_pull_requests_open
    ON task_pull_requests (repository_id, number, operation)
    WHERE open = 1;

-- Each operation goal's task keeps the goal's request, column and workflow
-- name; the first task of an open goal is the open operation.
INSERT INTO task_pull_requests (task_id, operation, repository_id, number, url, open)
SELECT t.id, g.pr_operation, g.pr_repository_id, g.pr_number, COALESCE(g.pr_url, ''),
       g.status IN ('planning', 'active')
       AND t.id = (SELECT f.id FROM tasks f WHERE f.goal_id = g.id
                    ORDER BY f.created_at, f.id LIMIT 1)
  FROM tasks t JOIN goals g ON g.id = t.goal_id
 WHERE g.pr_operation IS NOT NULL
   AND g.pr_repository_id IS NOT NULL AND g.pr_number IS NOT NULL;

INSERT INTO remote_task_steps (task_id, ordinal, id, title, description, skills, ranks, gate)
SELECT t.id, s.ordinal, s.id, s.title, s.description, s.skills, s.ranks, s.gate
  FROM tasks t
  JOIN goals g ON g.id = t.goal_id
  JOIN goal_steps s ON s.goal_id = g.id
 WHERE g.pr_operation IS NOT NULL
   AND NOT EXISTS (SELECT 1 FROM remote_task_steps r WHERE r.task_id = t.id);

INSERT OR IGNORE INTO task_workflows (task_id, workflow)
SELECT t.id, g.workflow
  FROM tasks t JOIN goals g ON g.id = t.goal_id
 WHERE g.pr_operation IS NOT NULL;

-- The goals go; their tasks and sessions stay, with no goal.
UPDATE agent_sessions SET goal_id = NULL
 WHERE goal_id IN (SELECT id FROM goals WHERE pr_operation IS NOT NULL);
UPDATE tasks SET goal_id = NULL
 WHERE goal_id IN (SELECT id FROM goals WHERE pr_operation IS NOT NULL);
DELETE FROM goal_steps
 WHERE goal_id IN (SELECT id FROM goals WHERE pr_operation IS NOT NULL);
DELETE FROM goal_repositories
 WHERE goal_id IN (SELECT id FROM goals WHERE pr_operation IS NOT NULL);
DELETE FROM attention_source_versions
 WHERE kind = 'goal'
   AND source_id IN (SELECT id FROM goals WHERE pr_operation IS NOT NULL);
DELETE FROM goals WHERE pr_operation IS NOT NULL;

DROP INDEX idx_goals_pr_operation_active;
ALTER TABLE goals DROP COLUMN pr_url;
ALTER TABLE goals DROP COLUMN pr_number;
ALTER TABLE goals DROP COLUMN pr_repository_id;
ALTER TABLE goals DROP COLUMN pr_operation;
