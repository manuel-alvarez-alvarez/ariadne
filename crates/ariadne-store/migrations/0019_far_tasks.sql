-- A far task runs on another host (037). `host` names that host, or is NULL
-- for a task on this daemon. `far_goal_id` and `far_task_id` are the linked
-- goal and the task on that host, once the task is sent there.
-- `host_unreachable` is 1 while the host is not `ready`.
ALTER TABLE tasks ADD COLUMN host TEXT;
ALTER TABLE tasks ADD COLUMN far_goal_id TEXT;
ALTER TABLE tasks ADD COLUMN far_task_id TEXT;
ALTER TABLE tasks ADD COLUMN host_unreachable INTEGER NOT NULL DEFAULT 0;
CREATE INDEX idx_tasks_host ON tasks (host) WHERE host IS NOT NULL;
