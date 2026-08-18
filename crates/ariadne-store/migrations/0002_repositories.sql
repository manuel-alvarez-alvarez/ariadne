-- Repositories as a first-class global entity: a repo is registered once and
-- named by id, instead of being spelled out again in every goal that uses it.
-- Goals keep their own `goal_repos` rows for now; rewiring them comes later.

CREATE TABLE repositories (
    id          TEXT PRIMARY KEY,
    path        TEXT NOT NULL,                  -- absolute repo path
    base_branch TEXT NOT NULL,
    description TEXT,                           -- NULL = none given
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    -- The same checkout can be registered once per base branch.
    UNIQUE (path, base_branch)
);
