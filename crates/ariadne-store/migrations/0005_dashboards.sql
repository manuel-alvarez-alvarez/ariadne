CREATE TABLE dashboards (
    name TEXT PRIMARY KEY,
    document TEXT,
    builtin INTEGER NOT NULL DEFAULT 0 CHECK (builtin IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (builtin = 1 OR document IS NOT NULL)
);
