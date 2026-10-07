-- Opening a request and finishing its task no longer waits on a human merge
-- inside the task: the daemon opens the request itself, and the task ends
-- once it is open and pushed. Nothing reads a readiness report any more.
ALTER TABLE tasks DROP COLUMN pr_ready;
