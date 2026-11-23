-- A far task runs on another host (037). `host` names that host, or is NULL
-- for a task on this daemon. `far_goal_id` and `far_task_id` are the linked
-- goal and the task on that host, once the task is sent there.
-- `host_unreachable` is 1 while the host is not `ready`.
-- On the host, `origin_task_id` is the far task's id on the goal's daemon:
-- a linked goal holds one task for each, so a repeated create finds it.
ALTER TABLE tasks ADD COLUMN host TEXT;
ALTER TABLE tasks ADD COLUMN far_goal_id TEXT;
ALTER TABLE tasks ADD COLUMN far_task_id TEXT;
ALTER TABLE tasks ADD COLUMN host_unreachable INTEGER NOT NULL DEFAULT 0;
CREATE INDEX idx_tasks_host ON tasks (host) WHERE host IS NOT NULL;
-- One active linked goal for each origin goal.
CREATE UNIQUE INDEX idx_goals_open_origin ON goals (origin_goal_id)
    WHERE origin_goal_id IS NOT NULL AND status = 'active';
ALTER TABLE tasks ADD COLUMN origin_task_id TEXT;
CREATE UNIQUE INDEX idx_tasks_origin ON tasks (goal_id, origin_task_id)
    WHERE origin_task_id IS NOT NULL;
