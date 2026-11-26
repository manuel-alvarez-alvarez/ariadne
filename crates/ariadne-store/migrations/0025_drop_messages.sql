-- The agents post to each other's live sessions now (039), and no agent
-- message is stored. The `messages` table goes, with its indexes and the
-- triggers on it. Two triggers on other tables delete from it, so each is
-- built again without that statement. The two that only deleted messages go.
DROP TRIGGER delete_goal_execution;
DROP TRIGGER delete_source_dependencies;
DROP TRIGGER delete_task_messages;
DROP TRIGGER delete_agent_messages;
DROP TRIGGER message_goal_reference;
DROP TRIGGER message_task_reference;
DROP INDEX IF EXISTS idx_messages_task;
DROP INDEX IF EXISTS idx_messages_goal;
DROP INDEX IF EXISTS idx_messages_relay;
DROP TABLE messages;

CREATE TRIGGER delete_goal_execution AFTER DELETE ON goals BEGIN
    DELETE FROM agent_sessions WHERE goal_id = OLD.id AND
        (task_id IS NULL OR (task_id NOT IN (SELECT task_id FROM remote_task_contexts) AND task_id NOT IN (SELECT id FROM source_task_references)));
    DELETE FROM tasks WHERE goal_id = OLD.id AND
        id NOT IN (SELECT task_id FROM remote_task_contexts);
END;

CREATE TRIGGER delete_source_dependencies AFTER DELETE ON source_task_references BEGIN
 DELETE FROM task_dependencies WHERE task_id=OLD.id OR depends_on_task_id=OLD.id;
END;
