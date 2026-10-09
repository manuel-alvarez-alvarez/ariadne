-- The forge a repository's remote is on, and whether Ariadne works with it
-- (025). One row per repository, and none where the checkout has no usable
-- remote. `host`, `owner` and `name` are stored lower-cased, so one forge
-- repository reads the same whichever URL spelled it.
CREATE TABLE forge_integrations (
    repository_id  TEXT PRIMARY KEY REFERENCES repositories (id) ON DELETE CASCADE,
    kind           TEXT NOT NULL CHECK (kind IN ('github', 'gitlab')),
    host           TEXT NOT NULL,
    owner          TEXT NOT NULL,
    name           TEXT NOT NULL,
    remote         TEXT NOT NULL,                 -- the remote's name, `origin`
    enabled        INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    login          TEXT,                          -- NULL until enabled
    babysit_model  TEXT,
    babysit_effort TEXT,
    review_model   TEXT,
    review_effort  TEXT,
    detected_at    TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);
-- The same checkout can be registered once per base branch (002), and one
-- forge repository is enabled on one of those rows at a time.
CREATE UNIQUE INDEX idx_forge_integrations_enabled
    ON forge_integrations (host, owner, name) WHERE enabled = 1;
