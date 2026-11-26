DROP VIEW task_index;
DROP TRIGGER dependency_reference;
DROP TRIGGER delete_goal_execution;
-- Goal-free task execution. Store::open disables foreign keys during rebuilds.

CREATE TABLE tasks_new (
    id            TEXT PRIMARY KEY,
    goal_id       TEXT,
    repo_id       TEXT NOT NULL REFERENCES repositories (id),
    title         TEXT NOT NULL,
    description   TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'pending'
                  CHECK (status IN ('pending', 'ready', 'in_progress', 'finished',
                                    'cancelled', 'failed')),
    branch        TEXT NOT NULL,
    worktree_path TEXT,
    stalled       INTEGER NOT NULL DEFAULT 0,
    merge_commit  TEXT,
    -- The pull or merge request the task's `pr` column opened, where it did.
    pr_url        TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    -- The column the task is in while it is `in_progress`.
    step          TEXT,
    host TEXT,
    far_goal_id TEXT,
    far_task_id TEXT,
    host_unreachable INTEGER NOT NULL DEFAULT 0,
    origin_task_id TEXT
);

INSERT INTO tasks_new SELECT * FROM tasks;

DROP TABLE tasks;

ALTER TABLE tasks_new RENAME TO tasks;

CREATE INDEX idx_tasks_goal ON tasks (goal_id);

CREATE INDEX idx_tasks_status ON tasks (status);

CREATE TRIGGER attention_task_changed AFTER UPDATE OF status ON tasks
WHEN OLD.status IS NOT NEW.status
BEGIN
    INSERT INTO attention_source_versions (kind, source_id, version)
    VALUES ('task', NEW.id, 1)
    ON CONFLICT(kind, source_id) DO UPDATE SET version = version + 1;
END;

CREATE INDEX idx_tasks_host ON tasks (host) WHERE host IS NOT NULL;

CREATE UNIQUE INDEX idx_tasks_origin ON tasks (origin_task_id)
    WHERE origin_task_id IS NOT NULL;

CREATE VIEW task_index AS SELECT * FROM tasks UNION ALL SELECT
 id, goal_id, repo_id, json_extract(cache,'$.title'), json_extract(cache,'$.description'),
 CASE WHEN dependency_failure IS NOT NULL THEN 'failed' ELSE json_extract(cache,'$.status') END, json_extract(cache,'$.branch'),
 NULL, COALESCE(json_extract(cache,'$.stalled'),0),
 json_extract(cache,'$.merge_commit'), json_extract(cache,'$.pr_url'),
 created_at, updated_at, json_extract(cache,'$.step'), host, goal_id, id, stale, NULL
 FROM source_task_references;
CREATE TRIGGER delete_local_dependencies AFTER DELETE ON tasks BEGIN
 DELETE FROM task_dependencies WHERE task_id=OLD.id OR depends_on_task_id=OLD.id;
END;
CREATE TRIGGER task_goal_reference BEFORE INSERT ON tasks
WHEN NEW.goal_id IS NOT NULL
 AND NOT EXISTS (SELECT 1 FROM goals WHERE id = NEW.goal_id)
 AND NOT EXISTS (SELECT 1 FROM remote_task_contexts WHERE task_id = NEW.id AND source_goal_id = NEW.goal_id)
BEGIN SELECT RAISE(ABORT, 'task needs a local goal or saved remote context'); END;
CREATE TRIGGER delete_goal_execution AFTER DELETE ON goals BEGIN
    DELETE FROM agent_sessions WHERE goal_id = OLD.id AND
        (task_id IS NULL OR (task_id NOT IN (SELECT task_id FROM remote_task_contexts) AND task_id NOT IN (SELECT id FROM source_task_references)));
    DELETE FROM tasks WHERE goal_id = OLD.id AND
        id NOT IN (SELECT task_id FROM remote_task_contexts);
END;

CREATE TRIGGER dependency_reference BEFORE INSERT ON task_dependencies
 WHEN NOT EXISTS(SELECT 1 FROM task_index WHERE id=NEW.task_id)
 OR NOT EXISTS(SELECT 1 FROM task_index WHERE id=NEW.depends_on_task_id)
 BEGIN SELECT RAISE(ABORT,'dependency needs an existing task'); END;
CREATE TABLE remote_task_steps_new (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    id TEXT NOT NULL,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    skills TEXT NOT NULL CHECK (json_valid(skills)),
    ranks TEXT NOT NULL CHECK (json_valid(ranks)),
    gate TEXT,
    PRIMARY KEY (task_id, ordinal),
    UNIQUE (task_id, id)
);
INSERT INTO remote_task_steps_new SELECT * FROM remote_task_steps;
DROP TABLE remote_task_steps;
ALTER TABLE remote_task_steps_new RENAME TO remote_task_steps;
CREATE TABLE task_workflows (
 task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
 workflow TEXT NOT NULL
);
