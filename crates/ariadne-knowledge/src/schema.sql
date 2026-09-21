-- The knowledge store. One file, `knowledge.db`, beside the daemon's own
-- database, and disposable: `SCHEMA_VERSION` in `store.rs` is written to
-- `PRAGMA user_version`, and a file at any other version is deleted and
-- rebuilt from the repositories. Spec 022 documents every table.

-- Every registered repository the index has heard of, with its state and
-- the ref another repository is read against: what the daemon says the
-- base branch is, and the first ref indexed until it says.
CREATE TABLE repositories (
    id          TEXT PRIMARY KEY,
    -- idle | indexing | failed
    state       TEXT NOT NULL,
    error       TEXT,
    updated_at  TEXT NOT NULL,
    base_ref    TEXT
);

-- The refs indexed per repository, each at the commit it was last read at.
CREATE TABLE refs (
    repository_id TEXT NOT NULL REFERENCES repositories(id) ON DELETE CASCADE,
    git_ref       TEXT NOT NULL,
    -- The commit the files of the ref are at.
    commit_sha    TEXT NOT NULL,
    indexed_at    TEXT NOT NULL,
    -- The commit the edges of the ref are at: written once the resolution and
    -- link passes of a run succeeded. NULL, or behind `commit_sha`, after a
    -- run that failed between the two, and the next run derives every edge
    -- of the ref again.
    edges_commit  TEXT,
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
-- By blob first, then the ref: an edge names a blob at one ref of one
-- repository, and the join that reads the path back must not scan the ref.
CREATE INDEX files_by_blob ON files(blob, repository_id, git_ref);

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
CREATE INDEX mentions_by_from_symbol ON mentions(from_symbol);

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

-- What one blob offers or takes beyond its symbols: the package a manifest
-- defines and the ones it depends on, the routes a file registers or
-- requests, the environment variables it sets or reads. Kept per blob like
-- the mentions, and matched across repositories by the link pass.
CREATE TABLE interfaces (
    blob     TEXT NOT NULL REFERENCES blobs(blob) ON DELETE CASCADE,
    -- package | dependency | path_dependency | route | route_use | env_read | env_set
    kind     TEXT NOT NULL,
    -- The package, the route or the variable.
    name     TEXT NOT NULL,
    line     INTEGER NOT NULL,
    -- The definition it sits in, or the one a route decorator is on. NULL at
    -- file scope.
    symbol   INTEGER REFERENCES symbols(id) ON DELETE CASCADE,
    -- The handler names a route registration passes, space-separated. NULL
    -- on everything else.
    handlers TEXT,
    -- The HTTP method a route call names, uppercased. NULL where the call
    -- names none.
    method   TEXT
);
CREATE INDEX interfaces_by_blob ON interfaces(blob);
CREATE INDEX interfaces_by_name ON interfaces(kind, name);
CREATE INDEX interfaces_by_symbol ON interfaces(symbol);

-- Relations between two ends, within and across repositories: a reference
-- to the definition behind it, a manifest to the package it depends on, a
-- request to the route it calls, a set variable to where it is read. Each
-- end is a blob at a ref of a repository, a line, and the symbol there where
-- there is one — a manifest line or a `.env` line is no symbol.
--
-- A symbol edge is keyed by the referencing blob at one ref of one
-- repository: re-deriving it is one delete by that key and one insert. The
-- ref is part of the key because the answer depends on it. Two refs share
-- the blob of an unchanged file, and each resolves its names against its own
-- tree: `a.rs` points at the `b` of the branch on the branch, and at the `b`
-- of the base branch on the base branch. One edge set per blob would hold
-- whichever ref resolved it last, and the other ref would then answer for a
-- definition it does not have.
--
-- An interface edge, and a reference across repositories, is keyed by the
-- pair of refs it joins instead, and the pair is derived whole.
CREATE TABLE edges (
    from_repository TEXT NOT NULL,
    git_ref         TEXT NOT NULL,
    from_blob       TEXT NOT NULL REFERENCES blobs(blob) ON DELETE CASCADE,
    -- calls | references | implements | extends | imports |
    -- depends_on | calls_route | sets_env
    kind            TEXT NOT NULL,
    -- The definition the reference sits in. NULL at file scope, which is
    -- where a file's own imports and a manifest's lines sit.
    from_symbol     INTEGER REFERENCES symbols(id) ON DELETE CASCADE,
    from_line       INTEGER NOT NULL,
    to_repository   TEXT NOT NULL,
    to_ref          TEXT NOT NULL,
    to_blob         TEXT NOT NULL REFERENCES blobs(blob) ON DELETE CASCADE,
    -- The definition the edge points at. NULL where the end is a manifest
    -- line, a `.env` line or a route nothing resolved the handler of.
    to_symbol       INTEGER REFERENCES symbols(id) ON DELETE CASCADE,
    to_line         INTEGER NOT NULL,
    -- What an interface edge is about — the package, the route template, the
    -- variable — and the name a reference across repositories named. NULL on
    -- a symbol edge within one repository.
    name            TEXT,
    -- exact | heuristic
    confidence      TEXT NOT NULL,
    -- Which step answered: `file`, `directory`, `import` or `repository` for
    -- a symbol edge (rule 9), `name` for a foreign reference and for a
    -- variable or a dependency matched by name, `path` for a path
    -- dependency, `route` for a route the request fits.
    step            TEXT NOT NULL,
    -- How many definitions the name matched at the step that resolved it.
    candidates      INTEGER NOT NULL
);
-- Covering, in both directions, under the ref the walk reads: counting the
-- callers of a level reads the index alone, which is what keeps a hub cheap
-- to find. The callers of a definition are the edges into it at its own
-- repository and ref, whichever repository they come from; listing them
-- reads the row of each edge it answers with, for the step and the count.
CREATE INDEX edges_from ON edges(from_repository, git_ref, kind, from_symbol, to_symbol, confidence);
CREATE INDEX edges_to ON edges(to_repository, to_ref, kind, to_symbol, from_symbol, confidence);
-- What deriving one blob's edges again deletes by, and what a dropped ref
-- deletes by.
CREATE INDEX edges_by_blob ON edges(from_repository, git_ref, from_blob);
-- A parent delete reaches each edge end by these columns. The query indexes
-- above do not lead with them and cannot serve SQLite's cascade lookup.
CREATE INDEX edges_by_from_symbol ON edges(from_symbol);
CREATE INDEX edges_by_to_symbol ON edges(to_symbol);
CREATE INDEX edges_by_from_blob ON edges(from_blob);
CREATE INDEX edges_by_to_blob ON edges(to_blob);
