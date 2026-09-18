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
-- Resolution asks for the definitions of one name, over and over.
CREATE INDEX symbols_by_name ON symbols(name);

-- The identifiers of every symbol, split on camelCase, snake_case and the
-- segments of the qualified name, for `search_code`. The rowid is the
-- symbol's id, which is what joins a hit to its symbol and what a delete
-- finds a row by without reading the whole table.
CREATE VIRTUAL TABLE symbols_fts USING fts5(terms, tokenize = 'unicode61');

-- The names one blob names, as the parser found them: what the resolution
-- pass turns into edges. Kept per blob, so a blob that did not change is
-- resolved again without being parsed again.
CREATE TABLE mentions (
    blob        TEXT NOT NULL REFERENCES blobs(blob) ON DELETE CASCADE,
    -- calls | references | implements | extends
    kind        TEXT NOT NULL,
    name        TEXT NOT NULL,
    line        INTEGER NOT NULL,
    -- The definition the reference sits in. NULL at file scope.
    from_symbol INTEGER REFERENCES symbols(id) ON DELETE CASCADE
);
CREATE INDEX mentions_by_blob ON mentions(blob);
CREATE INDEX mentions_by_name ON mentions(name);

-- The import statements of one blob: `use` in Rust, `import` and `from …
-- import` in Python, `import` and `require` in TypeScript and JavaScript,
-- `using` in C#. A named import is an edge of its own; a whole-module import
-- (`name` NULL) only narrows where the other names of the file resolve.
CREATE TABLE imports (
    blob   TEXT NOT NULL REFERENCES blobs(blob) ON DELETE CASCADE,
    module TEXT NOT NULL,
    name   TEXT,
    line   INTEGER NOT NULL
);
CREATE INDEX imports_by_blob ON imports(blob);
CREATE INDEX imports_by_name ON imports(name);

-- Relations between symbols, within and across repositories, as the
-- resolution pass derived them. Keyed by the referencing blob at one ref of
-- one repository: re-deriving them is one delete by that key and one insert.
--
-- The ref is part of the key because the answer depends on it. Two refs share
-- the blob of an unchanged file, and each resolves its names against its own
-- tree: `a.rs` points at the `b` of the branch on the branch, and at the `b`
-- of the base branch on the base branch. One edge set per blob would hold
-- whichever ref resolved it last, and the other ref would then answer for a
-- definition it does not have.
CREATE TABLE edges (
    from_repository TEXT NOT NULL,
    git_ref         TEXT NOT NULL,
    from_blob       TEXT NOT NULL REFERENCES blobs(blob) ON DELETE CASCADE,
    kind            TEXT NOT NULL,
    -- The definition the reference sits in. NULL at file scope, which is
    -- where a file's own imports sit.
    from_symbol     INTEGER REFERENCES symbols(id) ON DELETE CASCADE,
    to_symbol       INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    to_repository   TEXT NOT NULL,
    from_line       INTEGER NOT NULL,
    -- exact | heuristic
    confidence      TEXT NOT NULL,
    -- How many definitions the name matched at the step that resolved it.
    candidates      INTEGER NOT NULL
);
-- Covering, in both directions, under the ref the walk reads: a walk over the
-- graph reads the index alone.
CREATE INDEX edges_from ON edges(from_repository, git_ref, kind, from_symbol, to_symbol, confidence);
CREATE INDEX edges_to ON edges(from_repository, git_ref, kind, to_symbol, from_symbol, confidence);
-- What deriving one blob's edges again deletes by, and what a dropped ref
-- deletes by.
CREATE INDEX edges_by_blob ON edges(from_repository, git_ref, from_blob);
