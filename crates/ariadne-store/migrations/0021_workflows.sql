-- A workflow: a linear kanban of columns that stages an author and reviewer
-- agent through a task, in place of the fixed author/reviewer/landing
-- pipeline. Stored the same way a skill is (0001): a NULL document while it
-- runs on the text Ariadne ships, so a reworded shipped workflow reaches
-- every database without a migration, and a reset drops the override rather
-- than copying a default in.
CREATE TABLE workflows (
    name       TEXT PRIMARY KEY,                -- kebab-case; named on the document's first line
    document   TEXT,                            -- NULL = the shipped default of `name`
    builtin    INTEGER NOT NULL DEFAULT 0 CHECK (builtin IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (builtin = 1 OR document IS NOT NULL)
);
