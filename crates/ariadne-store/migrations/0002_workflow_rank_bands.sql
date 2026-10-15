-- A workflow column's single optional `rank` becomes `ranks`: every
-- `ModelRank` planning may staff the column on, a JSON array that is never
-- empty where the document named any. There is no way to turn one
-- snapshotted rank into a capability band without guessing, so
-- `ariadne_store::Store::open` refuses a database whose `goal_steps` still
-- has the old column before this migration ever runs
-- (`pre_ranks_database`); the fix is the same as `pre_squash_database`'s:
-- delete the file and start again. SQLite's `ALTER TABLE` cannot add a
-- `NOT NULL` column with no default, so the table is rebuilt whole, the
-- way `0001_init.sql` first created it. `Store::open` already runs every
-- migration with foreign keys off and turns them back on once they are
-- done, so this file states no `PRAGMA` of its own.

CREATE TABLE goal_steps_new (
    goal_id     TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    ordinal     INTEGER NOT NULL,
    id          TEXT NOT NULL,
    title       TEXT NOT NULL,
    description TEXT NOT NULL,
    skills      TEXT NOT NULL CHECK (json_valid(skills)),
    ranks       TEXT NOT NULL CHECK (json_valid(ranks)),
    gate        TEXT,
    PRIMARY KEY (goal_id, ordinal),
    UNIQUE (goal_id, id)
);

INSERT INTO goal_steps_new (goal_id, ordinal, id, title, description, skills, ranks, gate)
SELECT goal_id, ordinal, id, title, description, skills, '[]', gate FROM goal_steps;

DROP TABLE goal_steps;
ALTER TABLE goal_steps_new RENAME TO goal_steps;
