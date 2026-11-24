-- Source coordination has no execution or staffing rows.
CREATE TABLE source_task_references (
 id TEXT PRIMARY KEY,
 goal_id TEXT NOT NULL,
 repo_id TEXT NOT NULL,
 host TEXT NOT NULL,
 worker_id TEXT NOT NULL,
 request TEXT CHECK(request IS NULL OR json_valid(request)),
 cache TEXT NOT NULL CHECK(json_valid(cache)),
 stale INTEGER NOT NULL DEFAULT 1,
 cancel_requested INTEGER NOT NULL DEFAULT 0,
 retry_requested INTEGER NOT NULL DEFAULT 0,
 dependency_failure TEXT,
 authorization_revision INTEGER,
 progress_id TEXT,
 progress_at TEXT,
 created_at TEXT NOT NULL,
 updated_at TEXT NOT NULL
);
CREATE INDEX source_tasks_goal ON source_task_references(goal_id);
CREATE INDEX source_tasks_host ON source_task_references(host);
CREATE VIEW task_index AS SELECT * FROM tasks UNION ALL SELECT
 id, goal_id, repo_id, json_extract(cache,'$.title'), json_extract(cache,'$.description'),
 CASE WHEN dependency_failure IS NOT NULL THEN 'failed' ELSE json_extract(cache,'$.status') END, json_extract(cache,'$.branch'),
 NULL, COALESCE(json_extract(cache,'$.stalled'),0),
 json_extract(cache,'$.merge_commit'), json_extract(cache,'$.pr_url'),
 created_at, updated_at, json_extract(cache,'$.step'), host, goal_id, id, stale, NULL
 FROM source_task_references;
CREATE TABLE task_dependencies_new (
 task_id TEXT NOT NULL,
 depends_on_task_id TEXT NOT NULL,
 PRIMARY KEY(task_id, depends_on_task_id)
);
INSERT INTO task_dependencies_new SELECT * FROM task_dependencies;
DROP TABLE task_dependencies;
ALTER TABLE task_dependencies_new RENAME TO task_dependencies;
CREATE TRIGGER dependency_reference BEFORE INSERT ON task_dependencies
 WHEN NOT EXISTS(SELECT 1 FROM task_index WHERE id=NEW.task_id)
 OR NOT EXISTS(SELECT 1 FROM task_index WHERE id=NEW.depends_on_task_id)
 BEGIN SELECT RAISE(ABORT,'dependency needs an existing task'); END;
CREATE TRIGGER delete_local_dependencies AFTER DELETE ON tasks BEGIN
 DELETE FROM task_dependencies WHERE task_id=OLD.id OR depends_on_task_id=OLD.id;
END;
DROP TRIGGER message_goal_reference;
DROP TRIGGER delete_goal_execution;
CREATE TABLE messages_new (
    id            TEXT PRIMARY KEY,
    goal_id       TEXT NOT NULL,
    -- NULL for a message about the goal rather than about one task.
    task_id       TEXT,
    kind          TEXT NOT NULL CHECK (kind IN ('message')),
    from_actor    TEXT NOT NULL
                  CHECK (from_actor IN ('orchestrator', 'agent', 'daemon', 'user')),
    from_agent_id TEXT,
    -- The session that actually sent it, for the record; a message outlives it.
    from_session  TEXT REFERENCES agent_sessions (id) ON DELETE SET NULL,
    to_actor      TEXT NOT NULL
                  CHECK (to_actor IN ('orchestrator', 'agent', 'daemon', 'user')),
    to_agent_id   TEXT,
    body          TEXT NOT NULL,
    -- When it was handed to the recipient's agent. NULL while it is still waiting.
    delivered_at  TEXT,
    created_at    TEXT NOT NULL,
    relay_id TEXT,
    relayed_at TEXT
);

INSERT INTO messages_new SELECT * FROM messages;

DROP TABLE messages;

ALTER TABLE messages_new RENAME TO messages;

CREATE INDEX idx_messages_task ON messages (task_id, id);

CREATE INDEX idx_messages_goal ON messages (goal_id, id);

CREATE UNIQUE INDEX idx_messages_relay ON messages (task_id, relay_id);


CREATE TRIGGER message_goal_reference BEFORE INSERT ON messages
 WHEN NOT EXISTS (SELECT 1 FROM goals WHERE id=NEW.goal_id)
 AND NOT EXISTS (SELECT 1 FROM remote_task_contexts WHERE task_id=NEW.task_id AND source_goal_id=NEW.goal_id)
 BEGIN SELECT RAISE(ABORT,'message needs a local goal or saved remote context'); END;

CREATE TRIGGER delete_goal_execution AFTER DELETE ON goals BEGIN
    DELETE FROM messages WHERE goal_id = OLD.id AND
        (task_id IS NULL OR (task_id NOT IN (SELECT task_id FROM remote_task_contexts) AND task_id NOT IN (SELECT id FROM source_task_references)));
    DELETE FROM agent_sessions WHERE goal_id = OLD.id AND
        (task_id IS NULL OR (task_id NOT IN (SELECT task_id FROM remote_task_contexts) AND task_id NOT IN (SELECT id FROM source_task_references)));
    DELETE FROM tasks WHERE goal_id = OLD.id AND
        id NOT IN (SELECT task_id FROM remote_task_contexts);
END;


CREATE TRIGGER delete_source_dependencies AFTER DELETE ON source_task_references BEGIN
 DELETE FROM task_dependencies WHERE task_id=OLD.id OR depends_on_task_id=OLD.id;
 DELETE FROM messages WHERE task_id=OLD.id;
END;

-- Preserve local cascade behavior while source messages use worker agent IDs.
CREATE TRIGGER delete_task_messages AFTER DELETE ON tasks BEGIN
 DELETE FROM messages WHERE task_id=OLD.id;
END;
CREATE TRIGGER delete_agent_messages AFTER DELETE ON task_agents BEGIN
 DELETE FROM messages WHERE from_agent_id=OLD.id OR to_agent_id=OLD.id;
END;
CREATE TRIGGER message_task_reference BEFORE INSERT ON messages
 WHEN NEW.task_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM task_index WHERE id=NEW.task_id)
 BEGIN SELECT RAISE(ABORT,'message needs an existing task'); END;

CREATE TABLE source_task_controls (
 id TEXT PRIMARY KEY,
 task_id TEXT NOT NULL REFERENCES source_task_references(id) ON DELETE CASCADE,
 from_status TEXT NOT NULL,
 to_status TEXT NOT NULL,
 actor TEXT NOT NULL,
 reason TEXT,
 created_at TEXT NOT NULL,
 from_step TEXT,
 to_step TEXT
);
CREATE INDEX source_controls_task ON source_task_controls(task_id,id);
