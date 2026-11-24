-- Worker-owned dependency outcomes, fenced by the mutable draft revision.
CREATE TABLE remote_dependency_controls (
 task_id TEXT NOT NULL REFERENCES remote_task_contexts(task_id) ON DELETE CASCADE,
 operation_id TEXT NOT NULL,
 request_revision INTEGER NOT NULL,
 reason TEXT NOT NULL,
 retry_request_revision INTEGER,
 retry_revision INTEGER,
 PRIMARY KEY(task_id,operation_id)
);
ALTER TABLE source_task_references ADD COLUMN failure_operation_id TEXT;
ALTER TABLE source_task_references ADD COLUMN failure_revision INTEGER;
UPDATE source_task_references SET failure_operation_id=COALESCE(progress_id,lower(hex(randomblob(16)))),
 failure_revision=json_extract(cache,'$.execution_context.revision'),progress_id=NULL,progress_at=NULL WHERE dependency_failure IS NOT NULL;
DROP VIEW task_index;
CREATE VIEW task_index AS SELECT * FROM tasks UNION ALL SELECT
 id, goal_id, repo_id, json_extract(cache,'$.title'), json_extract(cache,'$.description'),
 json_extract(cache,'$.status'), json_extract(cache,'$.branch'),
 NULL, COALESCE(json_extract(cache,'$.stalled'),0),
 json_extract(cache,'$.merge_commit'), json_extract(cache,'$.pr_url'),
 created_at, updated_at, json_extract(cache,'$.step'), host, goal_id, id, stale, NULL
 FROM source_task_references;

DROP TABLE source_task_controls;
