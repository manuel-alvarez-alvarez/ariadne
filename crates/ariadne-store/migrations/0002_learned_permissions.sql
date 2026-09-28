ALTER TABLE learned_permissions RENAME TO learned_permissions_old;

CREATE TABLE learned_permissions (
    id TEXT PRIMARY KEY,
    repository_id TEXT NOT NULL REFERENCES repositories (id) ON DELETE CASCADE,
    tool_name TEXT NOT NULL,
    kind TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('console', 'manual')),
    tool_call TEXT,
    options TEXT,
    selected_option TEXT,
    session_id TEXT,
    task_id TEXT,
    label TEXT CHECK (label IN ('allow', 'ask', 'deny')),
    danger REAL,
    allow_threshold REAL,
    deny_threshold REAL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (repository_id, tool_name, kind)
);

INSERT INTO learned_permissions (id, repository_id, tool_name, kind, source, created_at, updated_at)
SELECT '0' || substr(lower(hex(randomblob(13))), 1, 25),
       repository_id, tool_name, kind, 'console', created_at, created_at
FROM learned_permissions_old;

DROP TABLE learned_permissions_old;

CREATE INDEX idx_learned_permissions_repository
ON learned_permissions (repository_id, created_at DESC);
