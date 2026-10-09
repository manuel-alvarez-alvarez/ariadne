-- A pull request of the user gets a session of its own, fed by the daemon
-- (026). Every change adds a column or a table beside the ones that exist,
-- so an older row keeps every value it has, and a session of a task keeps a
-- NULL request.
ALTER TABLE agent_sessions
    ADD COLUMN pull_request_id TEXT REFERENCES pull_requests(id) ON DELETE CASCADE;
CREATE INDEX agent_sessions_pull_request ON agent_sessions (pull_request_id);

-- What the detail fetch reads off the forge beside the list fetch's fields.
ALTER TABLE pull_requests ADD COLUMN failed_checks TEXT NOT NULL DEFAULT '[]';
ALTER TABLE pull_requests
    ADD COLUMN behind_base INTEGER NOT NULL DEFAULT 0 CHECK (behind_base IN (0, 1));

-- What the request's session was last told, so each change is told once.
-- The failed checks it was told by name, whether it was told the head is
-- behind its base, and the review decision, state and rolled-up check state
-- it was last told. A NULL decision, state or check state is the baseline a
-- fresh row starts from: `none`, `open` and `none`.
ALTER TABLE pull_requests ADD COLUMN told_checks TEXT NOT NULL DEFAULT '[]';
ALTER TABLE pull_requests
    ADD COLUMN told_behind_base INTEGER NOT NULL DEFAULT 0 CHECK (told_behind_base IN (0, 1));
ALTER TABLE pull_requests ADD COLUMN told_review_decision TEXT;
ALTER TABLE pull_requests ADD COLUMN told_state TEXT;
ALTER TABLE pull_requests ADD COLUMN told_check_state TEXT;
-- When the session was last handed news, which is when its prompt went out.
ALTER TABLE pull_requests ADD COLUMN news_told_at TEXT;

-- When the daemon took the work of an ended request down: its session, its
-- worktree and the branches it owes. NULL on an ended row is cleanup still
-- owed, which outlives a restart. A row already ended before this release
-- owes nothing, since no session of this release ever ran for it.
ALTER TABLE pull_requests ADD COLUMN cleaned_at TEXT;
UPDATE pull_requests SET cleaned_at = updated_at WHERE state <> 'open';

CREATE TABLE pull_request_comments (
    id TEXT PRIMARY KEY,
    pull_request_id TEXT NOT NULL REFERENCES pull_requests(id) ON DELETE CASCADE,
    forge_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('review_comment', 'issue_comment', 'review')),
    author_login TEXT NOT NULL,
    author_is_bot INTEGER NOT NULL CHECK (author_is_bot IN (0, 1)),
    body TEXT NOT NULL,
    path TEXT,
    line INTEGER,
    in_reply_to TEXT,
    created_at TEXT NOT NULL,
    fetched_at TEXT NOT NULL,
    answered INTEGER NOT NULL DEFAULT 0 CHECK (answered IN (0, 1)),
    resolved INTEGER NOT NULL DEFAULT 0 CHECK (resolved IN (0, 1)),
    told_at TEXT,
    UNIQUE (pull_request_id, forge_id)
);
CREATE INDEX pull_request_comments_thread
    ON pull_request_comments (pull_request_id, thread_id);
