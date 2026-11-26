-- Empty source goal IDs denote goal-less references, never execution goals.
-- Expose these references with null goal IDs through the task index.
DROP VIEW task_index;
CREATE VIEW task_index AS SELECT * FROM tasks UNION ALL SELECT
 id, NULLIF(goal_id, ''), repo_id, json_extract(cache,'$.title'), json_extract(cache,'$.description'),
 CASE WHEN dependency_failure IS NOT NULL THEN 'failed' ELSE json_extract(cache,'$.status') END, json_extract(cache,'$.branch'),
 NULL, COALESCE(json_extract(cache,'$.stalled'),0),
 json_extract(cache,'$.merge_commit'), json_extract(cache,'$.pr_url'),
 created_at, updated_at, json_extract(cache,'$.step'), host, NULLIF(goal_id, ''), id, stale, NULL
 FROM source_task_references;
