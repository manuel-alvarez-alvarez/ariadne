-- The knowledge store. One file, `knowledge.db`, beside the daemon's own
-- database, and disposable: `SCHEMA_VERSION` in `store.rs` is written to
-- `PRAGMA user_version`, and a file at any other version is deleted and
-- rebuilt from the repositories. Spec 022 documents every table.

-- Every registered repository the index has heard of, with its state.
CREATE TABLE repositories (
    id          TEXT PRIMARY KEY,
    -- idle | indexing | failed
    state       TEXT NOT NULL,
    error       TEXT,
    updated_at  TEXT NOT NULL
);

-- The refs indexed per repository, each at the commit it was last read at.
CREATE TABLE refs (
    repository_id TEXT NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    git_ref       TEXT NOT NULL,
    commit_sha    TEXT NOT NULL,
    indexed_at    TEXT NOT NULL,
    PRIMARY KEY (repository_id, git_ref)
);

-- The files git tracks at a ref, each with the blob it held there. A blob is
-- parsed once and shared: two refs, or two repositories, holding the same
-- file content point at the same symbols.
CREATE TABLE files (
    repository_id TEXT NOT NULL,
    git_ref       TEXT NOT NULL,
    path          TEXT NOT NULL,
    blob          TEXT NOT NULL,
    language      TEXT NOT NULL,
    PRIMARY KEY (repository_id, git_ref, path),
    FOREIGN KEY (repository_id, git_ref) REFERENCES refs(repository_id, git_ref) ON DELETE CASCADE
);
CREATE INDEX files_by_blob ON files(blob);

-- The blobs parsed so far, by git object hash.
CREATE TABLE blobs (
    blob       TEXT PRIMARY KEY,
    language   TEXT NOT NULL,
    parsed_at  TEXT NOT NULL
);

-- The definitions of one blob. An id is never given twice: `edges` and the
-- FTS rows below point at symbols by id, and a reused id would point them at
-- a stranger.
CREATE TABLE symbols (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    blob           TEXT NOT NULL REFERENCES blobs(blob) ON DELETE CASCADE,
    kind           TEXT NOT NULL,
    name           TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    start_line     INTEGER NOT NULL,
    end_line       INTEGER NOT NULL,
    signature      TEXT NOT NULL,
    doc            TEXT,
    is_test        INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX symbols_by_blob ON symbols(blob);

-- The identifiers of every symbol, split on camelCase, snake_case and the
-- segments of the qualified name, for `search_code`. The rowid is the
-- symbol's id, which is what joins a hit to its symbol and what a delete
-- finds a row by without reading the whole table.
CREATE VIRTUAL TABLE symbols_fts USING fts5(terms, tokenize = 'unicode61');

-- Relations between symbols, within and across repositories. Empty here:
-- the task that reads references fills it.
CREATE TABLE edges (
    kind            TEXT NOT NULL,
    from_symbol     INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    to_symbol       INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    from_repository TEXT NOT NULL,
    to_repository   TEXT NOT NULL,
    confidence      REAL NOT NULL
);
CREATE INDEX edges_from ON edges(from_symbol);
CREATE INDEX edges_to ON edges(to_symbol);
