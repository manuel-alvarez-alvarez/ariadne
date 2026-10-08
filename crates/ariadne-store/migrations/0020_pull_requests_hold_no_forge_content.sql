-- A pull request row holds Ariadne's own bookkeeping of a request it works
-- on, and nothing the forge holds (026): the title, the description, the
-- branches, the checks and the comments are read off the forge on every
-- fetch and kept in memory alone, so nothing stored can go stale.
--
-- A row stays only while Ariadne works on the request: the one a task
-- opened, one of the user's they asked Ariadne to review, or an open one
-- that asks for their review on a repository with a review pin. Every other
-- row goes, and so does an ended one whose work was taken down. The
-- sessions that ran on a row that goes are let go of it first, so their
-- history and spend stay.
CREATE TEMP TABLE pull_requests_kept AS
SELECT id FROM pull_requests p
 WHERE (p.state = 'open' OR p.cleaned_at IS NULL)
   AND (p.origin_task_id IS NOT NULL
        OR p.review_asked = 1
        OR (p.role = 'reviewer' AND p.review_requested = 1 AND p.state = 'open'
            AND EXISTS (SELECT 1 FROM forge_integrations f
                         WHERE f.repository_id = p.repository_id
                           AND f.enabled = 1 AND f.review_model IS NOT NULL)));
UPDATE agent_sessions SET pull_request_id = NULL
 WHERE pull_request_id IS NOT NULL
   AND pull_request_id NOT IN (SELECT id FROM pull_requests_kept);
DELETE FROM pull_requests WHERE id NOT IN (SELECT id FROM pull_requests_kept);
DROP TABLE pull_requests_kept;

-- A comment is the forge's: what stays of it is whether its session was
-- told of it, and whether an Ariadne review posted it (029).
CREATE TABLE pull_request_comment_marks (
    pull_request_id TEXT NOT NULL REFERENCES pull_requests(id) ON DELETE CASCADE,
    forge_id TEXT NOT NULL,
    told_at TEXT,
    from_review INTEGER NOT NULL DEFAULT 0 CHECK (from_review IN (0, 1)),
    PRIMARY KEY (pull_request_id, forge_id)
);
INSERT INTO pull_request_comment_marks (pull_request_id, forge_id, told_at, from_review)
SELECT pull_request_id, forge_id, told_at, from_review FROM pull_request_comments
 WHERE told_at IS NOT NULL OR from_review = 1;
DROP TABLE pull_request_comments;

ALTER TABLE pull_requests DROP COLUMN title;
ALTER TABLE pull_requests DROP COLUMN body;
ALTER TABLE pull_requests DROP COLUMN author_login;
ALTER TABLE pull_requests DROP COLUMN tracked_by;
ALTER TABLE pull_requests DROP COLUMN state;
ALTER TABLE pull_requests DROP COLUMN draft;
ALTER TABLE pull_requests DROP COLUMN head_branch;
ALTER TABLE pull_requests DROP COLUMN head_sha;
ALTER TABLE pull_requests DROP COLUMN head_repo;
ALTER TABLE pull_requests DROP COLUMN base_branch;
ALTER TABLE pull_requests DROP COLUMN checks;
ALTER TABLE pull_requests DROP COLUMN review_decision;
ALTER TABLE pull_requests DROP COLUMN unanswered_comments;
ALTER TABLE pull_requests DROP COLUMN opened_at;
ALTER TABLE pull_requests DROP COLUMN last_seen_at;
ALTER TABLE pull_requests DROP COLUMN failed_checks;
ALTER TABLE pull_requests DROP COLUMN behind_base;
ALTER TABLE pull_requests DROP COLUMN review_requested;
ALTER TABLE pull_requests DROP COLUMN merge_sha;
ALTER TABLE pull_requests DROP COLUMN cleaned_at;
