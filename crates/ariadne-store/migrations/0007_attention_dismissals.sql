CREATE TABLE attention_dismissals (
    item_id TEXT PRIMARY KEY,
    occurrence TEXT NOT NULL
);

-- Source revisions change in the same transaction as the cause. A clear and
-- return between attention reads must not revive an old dismissal.
CREATE TABLE attention_source_versions (
    kind TEXT NOT NULL,
    source_id TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (kind, source_id)
);

CREATE TRIGGER attention_session_changed AFTER UPDATE OF attention_reason ON agent_sessions
WHEN OLD.attention_reason IS NOT NEW.attention_reason
BEGIN
    INSERT INTO attention_source_versions (kind, source_id, version)
    VALUES ('session', NEW.id, 1)
    ON CONFLICT(kind, source_id) DO UPDATE SET version = version + 1;
END;

CREATE TRIGGER attention_task_changed AFTER UPDATE OF status ON tasks
WHEN OLD.status IS NOT NEW.status
BEGIN
    INSERT INTO attention_source_versions (kind, source_id, version)
    VALUES ('task', NEW.id, 1)
    ON CONFLICT(kind, source_id) DO UPDATE SET version = version + 1;
END;

CREATE TRIGGER attention_goal_changed AFTER UPDATE OF status, orchestrator_given_up_at ON goals
WHEN OLD.status IS NOT NEW.status OR OLD.orchestrator_given_up_at IS NOT NEW.orchestrator_given_up_at
BEGIN
    INSERT INTO attention_source_versions (kind, source_id, version)
    VALUES ('goal', NEW.id, 1)
    ON CONFLICT(kind, source_id) DO UPDATE SET version = version + 1;
END;

CREATE TRIGGER attention_repository_changed AFTER UPDATE OF enabled, fetch_error, host, kind ON forge_integrations
WHEN OLD.enabled IS NOT NEW.enabled OR OLD.fetch_error IS NOT NEW.fetch_error OR OLD.host IS NOT NEW.host OR OLD.kind IS NOT NEW.kind
BEGIN
    INSERT INTO attention_source_versions (kind, source_id, version)
    VALUES ('repository', NEW.repository_id, 1)
    ON CONFLICT(kind, source_id) DO UPDATE SET version = version + 1;
END;

CREATE TRIGGER attention_pull_request_changed AFTER UPDATE OF review_asked, ready, ready_head_sha, reviewer_given_up_at, keeper_given_up_at, babysit_asked ON pull_requests
WHEN OLD.review_asked IS NOT NEW.review_asked OR OLD.ready IS NOT NEW.ready OR OLD.ready_head_sha IS NOT NEW.ready_head_sha OR OLD.reviewer_given_up_at IS NOT NEW.reviewer_given_up_at OR OLD.keeper_given_up_at IS NOT NEW.keeper_given_up_at OR OLD.babysit_asked IS NOT NEW.babysit_asked
BEGIN
    INSERT INTO attention_source_versions (kind, source_id, version)
    VALUES ('pull_request', NEW.id, 1)
    ON CONFLICT(kind, source_id) DO UPDATE SET version = version + 1;
END;

-- Forge evidence lives in the daemon cache. Persist its attention transitions
-- when a fetch installs evidence, including clears between client reads.
CREATE TABLE attention_observations (
    item_id TEXT PRIMARY KEY,
    state TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 0
);
