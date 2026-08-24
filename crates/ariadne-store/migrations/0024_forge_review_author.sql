-- no-transaction
-- What the people on a published request wrote has an author of its own.
--
-- The daemon polls a pull or merge request and writes what it finds there as
-- a round of requested changes, so that the engineer is sent back to answer
-- it. That round had to name a profile — every verdict did — so it borrowed
-- the task's integrator, and the engineer read the humans' comments under the
-- heading "From Integrator", over an agent that had not read them, let alone
-- written them.
--
-- A verdict is now from one of two kinds of author: a profile of the task —
-- a reviewer, or the integrator sending the change back itself — or a role
-- that is nobody's profile, of which the forge is the only one. Exactly one
-- of the two columns is set, which the CHECK says.
--
-- Rows already in the table keep their profile: an integrator's own send-back
-- and a relay written under its name are the same row today, and guessing
-- which was which would rewrite history rather than record it.
--
-- Both are CHECK constraints, and SQLite cannot alter one in place: the table
-- is rebuilt the documented way — foreign keys off, one explicit transaction,
-- keys back on — which is also why this file opts out of sqlx's own
-- transaction above (`PRAGMA foreign_keys` is a no-op inside one).

PRAGMA foreign_keys = OFF;

BEGIN;

CREATE TABLE reviews_new (
    id                  TEXT PRIMARY KEY,
    task_id             TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    round               INTEGER NOT NULL,
    reviewer_profile_id TEXT REFERENCES profiles (id),
    author_role         TEXT CHECK (author_role IN ('forge')),
    session_id          TEXT REFERENCES agent_sessions (id),
    verdict             TEXT NOT NULL CHECK (verdict IN ('approve', 'request_changes')),
    body                TEXT,
    created_at          TEXT NOT NULL,
    CHECK ((reviewer_profile_id IS NULL) <> (author_role IS NULL)),
    UNIQUE (task_id, round, reviewer_profile_id)
);

INSERT INTO reviews_new (id, task_id, round, reviewer_profile_id, session_id,
                         verdict, body, created_at)
SELECT id, task_id, round, reviewer_profile_id, session_id,
       verdict, body, created_at
FROM reviews;

DROP TABLE reviews;
ALTER TABLE reviews_new RENAME TO reviews;
CREATE INDEX idx_reviews_task ON reviews (task_id, round);

-- One verdict per author per round, for the authors that are not a profile:
-- NULLs are distinct in a SQLite unique index, so the UNIQUE above says
-- nothing about rows with no profile id, and a round would take a second
-- relay of the same comments.
CREATE UNIQUE INDEX idx_reviews_role_author ON reviews (task_id, round, author_role)
    WHERE reviewer_profile_id IS NULL;

COMMIT;

PRAGMA foreign_keys = ON;
