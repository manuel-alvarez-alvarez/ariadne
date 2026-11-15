-- Preserve occurrence identities at upgrade, including existing dismissals.
-- Each cause starts at the old shared revision, then advances independently.
INSERT INTO attention_source_versions (kind, source_id, version)
SELECT 'pull_request_cause', causes.prefix || pull_requests.id, COALESCE(versions.version, 0)
FROM pull_requests
CROSS JOIN (
    SELECT 'recovery:unknown:pull_request:' AS prefix
    UNION ALL SELECT 'recovery:unknown:pull_request:keeper:'
    UNION ALL SELECT 'pull_request:review_start:'
    UNION ALL SELECT 'pull_request:ready:'
) AS causes
LEFT JOIN attention_source_versions AS versions
    ON versions.kind = 'pull_request' AND versions.source_id = pull_requests.id;

-- Keep the legacy trigger and revisions for older callers and rollback.
CREATE TRIGGER attention_reviewer_recovery_changed AFTER UPDATE OF reviewer_given_up_at ON pull_requests
WHEN OLD.reviewer_given_up_at IS NOT NEW.reviewer_given_up_at
BEGIN
    INSERT INTO attention_source_versions (kind, source_id, version)
    VALUES ('pull_request_cause', 'recovery:unknown:pull_request:' || NEW.id, 1)
    ON CONFLICT(kind, source_id) DO UPDATE SET version = version + 1;
END;

CREATE TRIGGER attention_keeper_recovery_changed AFTER UPDATE OF keeper_given_up_at ON pull_requests
WHEN OLD.keeper_given_up_at IS NOT NEW.keeper_given_up_at
BEGIN
    INSERT INTO attention_source_versions (kind, source_id, version)
    VALUES ('pull_request_cause', 'recovery:unknown:pull_request:keeper:' || NEW.id, 1)
    ON CONFLICT(kind, source_id) DO UPDATE SET version = version + 1;
END;

CREATE TRIGGER attention_review_start_changed AFTER UPDATE OF review_asked, reviewer_given_up_at ON pull_requests
WHEN OLD.review_asked IS NOT NEW.review_asked OR OLD.reviewer_given_up_at IS NOT NEW.reviewer_given_up_at
BEGIN
    INSERT INTO attention_source_versions (kind, source_id, version)
    VALUES ('pull_request_cause', 'pull_request:review_start:' || NEW.id, 1)
    ON CONFLICT(kind, source_id) DO UPDATE SET version = version + 1;
END;

CREATE TRIGGER attention_readiness_changed AFTER UPDATE OF ready, ready_head_sha, babysit_asked ON pull_requests
WHEN OLD.ready IS NOT NEW.ready OR OLD.ready_head_sha IS NOT NEW.ready_head_sha
    OR (OLD.babysit_asked IS NOT NEW.babysit_asked AND NEW.origin_task_id IS NULL)
BEGIN
    INSERT INTO attention_source_versions (kind, source_id, version)
    VALUES ('pull_request_cause', 'pull_request:ready:' || NEW.id, 1)
    ON CONFLICT(kind, source_id) DO UPDATE SET version = version + 1;
END;
