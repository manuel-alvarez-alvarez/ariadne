DROP TRIGGER attention_pull_request_changed;
CREATE TRIGGER attention_pull_request_changed AFTER UPDATE OF review_asked, reviewer_given_up_at, keeper_given_up_at, babysit_asked ON pull_requests
WHEN OLD.review_asked IS NOT NEW.review_asked OR OLD.reviewer_given_up_at IS NOT NEW.reviewer_given_up_at OR OLD.keeper_given_up_at IS NOT NEW.keeper_given_up_at OR OLD.babysit_asked IS NOT NEW.babysit_asked
BEGIN
    INSERT INTO attention_source_versions (kind, source_id, version)
    VALUES ('pull_request', NEW.id, 1)
    ON CONFLICT(kind, source_id) DO UPDATE SET version = version + 1;
END;
DROP TRIGGER attention_readiness_changed;
CREATE TRIGGER attention_readiness_changed AFTER UPDATE OF babysit_asked ON pull_requests
WHEN (OLD.babysit_asked IS NOT NEW.babysit_asked AND NEW.origin_task_id IS NULL)
BEGIN
    INSERT INTO attention_source_versions (kind, source_id, version)
    VALUES ('pull_request_cause', 'pull_request:ready:' || NEW.id, 1)
    ON CONFLICT(kind, source_id) DO UPDATE SET version = version + 1;
END;
ALTER TABLE pull_requests DROP COLUMN ready;
ALTER TABLE pull_requests DROP COLUMN ready_confirmed_at;
ALTER TABLE pull_requests DROP COLUMN ready_head_sha;
CREATE TABLE pull_request_draft_marks (
 pull_request_id TEXT NOT NULL REFERENCES pull_requests(id) ON DELETE CASCADE,
 head_sha TEXT NOT NULL,
 PRIMARY KEY (pull_request_id, head_sha)
);
