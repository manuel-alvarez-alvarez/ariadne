-- A keeping agent escalates a disputed review thread to a human (026): the
-- mark of the comment it escalates records when and why. The
-- marks still go with their row (`ON DELETE CASCADE` of 0001).
ALTER TABLE pull_request_comment_marks ADD COLUMN escalated_at TEXT;
ALTER TABLE pull_request_comment_marks ADD COLUMN escalation_reason TEXT;

-- Each escalated thread is its own attention cause (031): a write of
-- `escalated_at` moves only that thread's revision, so a dismissal of one
-- thread never hides another, and a later escalation of a dismissed
-- thread brings it back.
CREATE TRIGGER attention_escalation_raised AFTER INSERT ON pull_request_comment_marks
WHEN NEW.escalated_at IS NOT NULL
BEGIN
    INSERT INTO attention_source_versions (kind, source_id, version)
    VALUES ('pull_request_cause',
            'pull_request:escalated:' || NEW.pull_request_id || ':' || NEW.forge_id, 1)
    ON CONFLICT(kind, source_id) DO UPDATE SET version = version + 1;
END;

CREATE TRIGGER attention_escalation_changed AFTER UPDATE OF escalated_at ON pull_request_comment_marks
BEGIN
    INSERT INTO attention_source_versions (kind, source_id, version)
    VALUES ('pull_request_cause',
            'pull_request:escalated:' || NEW.pull_request_id || ':' || NEW.forge_id, 1)
    ON CONFLICT(kind, source_id) DO UPDATE SET version = version + 1;
END;
