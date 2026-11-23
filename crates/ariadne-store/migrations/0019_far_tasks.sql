-- A far task runs on another host (037). `host` names that host, or is NULL
-- for a task on this daemon. `far_goal_id` and `far_task_id` are the linked
-- goal and the task on that host, once the task is sent there.
-- `host_unreachable` is 1 while the host is not `ready`.
-- On the host, `origin_task_id` is the far task's id on the goal's daemon.
-- The host holds one task for each, in any goal, so a repeated create finds
-- it. The column is new, so no row of an older database can break the index.
-- Goals get no index: an older database may hold two active linked goals of
-- one origin goal, and an upgrade keeps both.
ALTER TABLE tasks ADD COLUMN host TEXT;
ALTER TABLE tasks ADD COLUMN far_goal_id TEXT;
ALTER TABLE tasks ADD COLUMN far_task_id TEXT;
ALTER TABLE tasks ADD COLUMN host_unreachable INTEGER NOT NULL DEFAULT 0;
CREATE INDEX idx_tasks_host ON tasks (host) WHERE host IS NOT NULL;
ALTER TABLE tasks ADD COLUMN origin_task_id TEXT;
CREATE UNIQUE INDEX idx_tasks_origin ON tasks (origin_task_id)
    WHERE origin_task_id IS NOT NULL;
