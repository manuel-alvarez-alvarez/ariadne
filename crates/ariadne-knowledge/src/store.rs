//! The SQLite file the index lives in, and the questions asked of it.
//!
//! `knowledge.db` sits beside the daemon's database and is disposable: the
//! schema in `schema.sql` is stamped into `PRAGMA user_version`, and a file
//! at any other version is deleted and rebuilt rather than migrated. Symbols
//! are keyed by blob, so a file whose content has not changed is parsed once
//! and shared by every ref and every repository that holds it.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Pool, QueryBuilder, Sqlite};

use crate::interfaces::Interface;
use crate::map::{self, MapFile, RepoMap};
use crate::parser::{Import, Reference, Symbol};
use crate::resolve::{
    Candidate, Edge, INTERFACE_KINDS, InterfaceRow, MIN_FOREIGN_NAME, Mention, Names,
};

/// The schema this build writes. Bump it with every change to `schema.sql`:
/// a store at another version is thrown away and indexed again.
pub const SCHEMA_VERSION: i64 = 5;

/// The edge kinds a walk of the callers follows: a call, and a request of
/// a route the definition handles.
const CALL_KINDS: &[&str] = &["calls", "calls_route"];

/// The directed edge kinds a path between definitions follows.
const PATH_KINDS: &[&str] = &[
    "calls",
    "calls_route",
    "references",
    "implements",
    "extends",
    "imports",
];

/// The edge kinds a map ranks over: the symbol edges of one name resolved to
/// one definition (022, rule 9), and no interface edge.
///
/// The link pass derives the interface edges of a ref against itself (rule
/// 31), so a repository whose crates depend on each other by path holds
/// `depends_on` edges between its own manifests, and a `.env` file holds
/// `sets_env` edges to every file that reads the variable. Both ends of
/// those are lines rather than definitions, and counting them would rank a
/// manifest by how many crates depend on it, beside the files the call graph
/// ranks. A map is of the code.
const SYMBOL_KINDS: &[&str] = &["calls", "references", "implements", "extends", "imports"];

const SCHEMA: &str = include_str!("schema.sql");

/// How many bound values one statement carries at most. SQLite's default
/// ceiling is far higher, and chunking at this size keeps every statement
/// short whatever the repository.
const CHUNK: usize = 500;

#[derive(Clone)]
pub struct KnowledgeStore {
    /// Single-connection pool: every write serializes here.
    write: Pool<Sqlite>,
    read: Pool<Sqlite>,
}

/// Where a repository's index stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Indexing,
    Failed,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Indexing => "indexing",
            State::Failed => "failed",
        }
    }

    fn parse(text: &str) -> State {
        match text {
            "indexing" => State::Indexing,
            "failed" => State::Failed,
            _ => State::Idle,
        }
    }
}

/// What the index holds for one repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub state: State,
    pub error: Option<String>,
    pub refs: Vec<RefStatus>,
    /// Distinct paths indexed across the repository's refs.
    pub files: i64,
    /// The symbols of the distinct blobs those paths hold.
    pub symbols: i64,
    pub languages: Vec<LanguageCount>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefStatus {
    pub git_ref: String,
    pub commit: String,
    pub indexed_at: String,
    pub files: i64,
    pub symbols: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageCount {
    pub language: String,
    pub files: i64,
}

/// One `search_code` question.
#[derive(Clone, Debug, Default)]
pub struct SearchQuery {
    pub q: String,
    /// The `(repository_id, git_ref)` pairs to search, one ref per
    /// repository.
    pub scopes: Vec<(String, String)>,
    pub kind: Option<String>,
    /// A substring of the path.
    pub path: Option<String>,
    pub limit: i64,
}

/// One search answer.
#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct Hit {
    pub repository_id: String,
    pub git_ref: String,
    pub path: String,
    pub line: i64,
    pub kind: String,
    pub name: String,
    pub signature: String,
}

/// One definition of an outline.
#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct OutlineEntry {
    pub kind: String,
    pub name: String,
    pub start_line: i64,
    pub end_line: i64,
    pub signature: String,
}

/// One tracked file at a ref, as the indexer writes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileRow {
    pub path: String,
    pub blob: String,
    pub language: &'static str,
}

/// What an index run does to the file rows of a ref.
#[derive(Clone, Debug, Default)]
pub(crate) struct FileChanges {
    /// Drop every row of the ref first: a first index, or one that could
    /// not read the diff since the last.
    pub replace_all: bool,
    pub removed: Vec<String>,
    pub upserted: Vec<FileRow>,
}

/// One blob's definitions and references, ready to store.
#[derive(Clone, Debug)]
pub(crate) struct ParsedBlob {
    pub blob: String,
    pub language: &'static str,
    pub symbols: Vec<Symbol>,
    pub references: Vec<Reference>,
    pub imports: Vec<Import>,
    pub interfaces: Vec<Interface>,
}

/// One definition of a name, as `GET /v1/knowledge/symbol` answers it.
#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct Definition {
    pub id: i64,
    pub repository_id: String,
    pub git_ref: String,
    pub path: String,
    /// The git object the file held: what the definition's own text is read
    /// from.
    pub blob: String,
    pub start_line: i64,
    pub end_line: i64,
    pub kind: String,
    pub name: String,
    pub signature: String,
    pub doc: Option<String>,
}

/// One end of an edge, as an answer names it.
#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct Related {
    pub repository_id: String,
    pub path: String,
    pub line: i64,
    pub name: String,
    /// `exact` or `heuristic`.
    pub confidence: String,
}

/// What one definition is joined to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SymbolContext {
    pub callers: Vec<Related>,
    pub callees: Vec<Related>,
    pub implementations: Vec<Related>,
    /// What names it without calling it: a type annotation, a constructed
    /// class, an import, and every reference from another repository.
    pub references: Vec<Related>,
    pub tests: Vec<Related>,
    /// How many entries each list held past [`CONTEXT_LIMIT`], 0 where the
    /// list is whole.
    pub more: ContextMore,
}

/// How many entries each list of a [`SymbolContext`] held back past
/// [`CONTEXT_LIMIT`], 0 where the list is whole.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextMore {
    pub callers: i64,
    pub callees: i64,
    pub implementations: i64,
    pub references: i64,
    pub tests: i64,
}

/// One end of an interaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InteractionEnd {
    pub repository_id: String,
    pub path: String,
    pub line: i64,
    /// The definition at that end, or what the edge is about where the end
    /// is no definition: the package, the route, the variable.
    pub symbol: String,
}

/// One edge between two repositories, as `GET /v1/knowledge/interactions`
/// lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interaction {
    /// `depends_on`, `references`, `calls_route` or `sets_env`.
    pub kind: String,
    pub from: InteractionEnd,
    pub to: InteractionEnd,
    pub confidence: String,
}

/// The order the interaction kinds are listed in.
pub const INTERACTION_KINDS: [&str; 4] = ["depends_on", "references", "calls_route", "sets_env"];

/// One caller of a changed symbol, and how far from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImpactCaller {
    /// 1 is a direct caller.
    pub depth: i64,
    pub repository_id: String,
    pub path: String,
    pub line: i64,
    pub name: String,
    pub confidence: String,
}

/// One definition on a shortest directed path. The edge fields name the
/// edge into this definition, and are empty on the first hop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathHop {
    pub repository_id: String,
    pub path: String,
    pub line: i64,
    pub kind: String,
    pub name: String,
    pub edge_kind: Option<String>,
    pub confidence: Option<String>,
}

/// How many callers a symbol may have and still be walked past. A symbol
/// over it is a utility everything touches, and its callers say nothing
/// about what one change reaches.
pub const MAX_FANOUT: i64 = 200;

/// How many entries each list of a symbol's context holds.
pub const CONTEXT_LIMIT: i64 = 20;

/// How many files a map ranks and names at most, whatever its budget. The
/// rendering cuts at the budget; this is what the queries under it carry.
const MAP_FILES: usize = 100;

/// How many definitions one file of a map names at most: the ones most of
/// the repository points at.
const MAP_SYMBOLS: usize = 10;

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

impl KnowledgeStore {
    /// Open the store at `path`, creating it, or recreating it where the file
    /// there was written at another schema version.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if path.exists() && version_of(path).await != Some(SCHEMA_VERSION) {
            tracing::info!(path = %path.display(), "the knowledge store is at another schema version: rebuilding it");
            remove(path)?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true);
        let write = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await
            .with_context(|| format!("opening {}", path.display()))?;
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(&write)
            .await?;
        if version == 0 {
            sqlx::raw_sql(SCHEMA).execute(&write).await?;
            // Safe: the one value interpolated is this build's own constant.
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "PRAGMA user_version = {SCHEMA_VERSION}"
            )))
            .execute(&write)
            .await?;
        }
        let read = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options.read_only(true))
            .await?;
        Ok(Self { write, read })
    }

    // -- status ---------------------------------------------------------------

    pub async fn status(&self, repository_id: &str) -> Result<Status> {
        let row: Option<(String, Option<String>)> =
            sqlx::query_as("SELECT state, error FROM repositories WHERE id = ?")
                .bind(repository_id)
                .fetch_optional(&self.read)
                .await?;
        let (state, error) = match row {
            Some((state, error)) => (State::parse(&state), error),
            None => (State::Idle, None),
        };
        let refs: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT git_ref, commit_sha, indexed_at FROM refs WHERE repository_id = ? ORDER BY git_ref",
        )
        .bind(repository_id)
        .fetch_all(&self.read)
        .await?;
        let mut statuses = Vec::with_capacity(refs.len());
        for (git_ref, commit, indexed_at) in refs {
            let (files, symbols) = self.counts(repository_id, &git_ref).await?;
            statuses.push(RefStatus {
                git_ref,
                commit,
                indexed_at,
                files,
                symbols,
            });
        }
        let files: i64 =
            sqlx::query_scalar("SELECT COUNT(DISTINCT path) FROM files WHERE repository_id = ?")
                .bind(repository_id)
                .fetch_one(&self.read)
                .await?;
        let symbols: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM symbols
             WHERE blob IN (SELECT DISTINCT blob FROM files WHERE repository_id = ?)",
        )
        .bind(repository_id)
        .fetch_one(&self.read)
        .await?;
        let languages: Vec<(String, i64)> = sqlx::query_as(
            "SELECT language, COUNT(DISTINCT path) AS files FROM files
             WHERE repository_id = ? GROUP BY language ORDER BY files DESC, language",
        )
        .bind(repository_id)
        .fetch_all(&self.read)
        .await?;
        Ok(Status {
            state,
            error,
            refs: statuses,
            files,
            symbols,
            languages: languages
                .into_iter()
                .map(|(language, files)| LanguageCount { language, files })
                .collect(),
        })
    }

    /// Record where a repository's indexing stands.
    pub async fn set_state(
        &self,
        repository_id: &str,
        state: State,
        error: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO repositories (id, state, error, updated_at) VALUES (?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET state = excluded.state, error = excluded.error,
                                           updated_at = excluded.updated_at",
        )
        .bind(repository_id)
        .bind(state.as_str())
        .bind(error)
        .bind(now())
        .execute(&self.write)
        .await?;
        Ok(())
    }

    /// Record the ref another repository is read against: the base branch,
    /// as the daemon knows it. Until it is recorded, the first ref indexed
    /// stands in.
    pub async fn set_base_ref(&self, repository_id: &str, git_ref: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO repositories (id, state, error, updated_at, base_ref) VALUES (?, 'idle', NULL, ?, ?)
             ON CONFLICT(id) DO UPDATE SET base_ref = excluded.base_ref",
        )
        .bind(repository_id)
        .bind(now())
        .bind(git_ref)
        .execute(&self.write)
        .await?;
        Ok(())
    }

    /// Every repository whose base ref is indexed, with that ref: what a
    /// ref of another repository is linked against.
    pub(crate) async fn base_refs(&self) -> Result<Vec<(String, String)>> {
        Ok(sqlx::query_as(
            "SELECT r.id, r.base_ref FROM repositories r
             JOIN refs ON refs.repository_id = r.id AND refs.git_ref = r.base_ref
             WHERE r.base_ref IS NOT NULL ORDER BY r.id",
        )
        .fetch_all(&self.read)
        .await?)
    }

    /// The commit a ref was last indexed at, if ever.
    pub async fn ref_commit(&self, repository_id: &str, git_ref: &str) -> Result<Option<String>> {
        Ok(sqlx::query_scalar(
            "SELECT commit_sha FROM refs WHERE repository_id = ? AND git_ref = ?",
        )
        .bind(repository_id)
        .bind(git_ref)
        .fetch_optional(&self.read)
        .await?)
    }

    /// The refs indexed for a repository.
    pub async fn ref_names(&self, repository_id: &str) -> Result<Vec<String>> {
        Ok(
            sqlx::query_scalar("SELECT git_ref FROM refs WHERE repository_id = ? ORDER BY git_ref")
                .bind(repository_id)
                .fetch_all(&self.read)
                .await?,
        )
    }

    /// Files and symbols indexed at one ref.
    pub async fn counts(&self, repository_id: &str, git_ref: &str) -> Result<(i64, i64)> {
        let files: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM files WHERE repository_id = ? AND git_ref = ?",
        )
        .bind(repository_id)
        .bind(git_ref)
        .fetch_one(&self.read)
        .await?;
        let symbols: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM symbols s JOIN files f ON f.blob = s.blob
             WHERE f.repository_id = ? AND f.git_ref = ?",
        )
        .bind(repository_id)
        .bind(git_ref)
        .fetch_one(&self.read)
        .await?;
        Ok((files, symbols))
    }

    /// Forget a repository: its state, its refs and its files, and the blobs
    /// no file holds any more.
    pub async fn drop_repository(&self, repository_id: &str) -> Result<()> {
        let mut tx = self.write.begin().await?;
        sqlx::query("DELETE FROM repositories WHERE id = ?")
            .bind(repository_id)
            .execute(&mut *tx)
            .await?;
        // Edges are keyed by the refs they join, which no cascade reaches: a
        // blob another repository still holds keeps its own rows.
        sqlx::query("DELETE FROM edges WHERE from_repository = ? OR to_repository = ?")
            .bind(repository_id)
            .bind(repository_id)
            .execute(&mut *tx)
            .await?;
        prune_orphan_blobs(&mut tx).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Forget one ref of a repository: its files, and the blobs no file
    /// holds any more. What a landed or deleted task branch leaves behind.
    pub async fn drop_ref(&self, repository_id: &str, git_ref: &str) -> Result<()> {
        let mut tx = self.write.begin().await?;
        sqlx::query("DELETE FROM refs WHERE repository_id = ? AND git_ref = ?")
            .bind(repository_id)
            .bind(git_ref)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "DELETE FROM edges WHERE (from_repository = ?1 AND git_ref = ?2)
                                  OR (to_repository = ?1 AND to_ref = ?2)",
        )
        .bind(repository_id)
        .bind(git_ref)
        .execute(&mut *tx)
        .await?;
        prune_orphan_blobs(&mut tx).await?;
        tx.commit().await?;
        Ok(())
    }

    // -- queries --------------------------------------------------------------

    /// The symbols whose identifiers match the query, best first: an exact
    /// name, then a name the query is a prefix of, then by FTS rank.
    pub async fn search(&self, query: &SearchQuery) -> Result<Vec<Hit>> {
        let Some(matched) = fts_match(&query.q) else {
            return Ok(Vec::new());
        };
        if query.scopes.is_empty() {
            return Ok(Vec::new());
        }
        let needle = query.q.trim().to_lowercase();
        let mut sql = QueryBuilder::<Sqlite>::new(
            "SELECT f.repository_id, f.git_ref, f.path, s.start_line AS line, s.kind, s.name, s.signature
             FROM symbols_fts
             JOIN symbols s ON s.id = symbols_fts.rowid
             JOIN files f ON f.blob = s.blob
             WHERE symbols_fts MATCH ",
        );
        sql.push_bind(matched);
        sql.push(" AND (");
        for (at, (repository_id, git_ref)) in query.scopes.iter().enumerate() {
            if at > 0 {
                sql.push(" OR ");
            }
            sql.push("(f.repository_id = ")
                .push_bind(repository_id)
                .push(" AND f.git_ref = ")
                .push_bind(git_ref)
                .push(")");
        }
        sql.push(")");
        if let Some(kind) = &query.kind {
            sql.push(" AND s.kind = ").push_bind(kind);
        }
        if let Some(path) = &query.path {
            sql.push(" AND instr(f.path, ")
                .push_bind(path)
                .push(") > 0");
        }
        sql.push(" ORDER BY (lower(s.name) = ")
            .push_bind(needle.clone())
            .push(") DESC, (substr(lower(s.name), 1, ")
            .push_bind(needle.chars().count() as i64)
            .push(") = ")
            .push_bind(needle)
            .push(") DESC, bm25(symbols_fts), f.path, s.start_line LIMIT ")
            .push_bind(query.limit.max(1));
        Ok(sql.build_query_as::<Hit>().fetch_all(&self.read).await?)
    }

    /// The definitions of one file at one ref, in line order. `None` where the
    /// file is not indexed there.
    pub async fn outline(
        &self,
        repository_id: &str,
        git_ref: &str,
        path: &str,
    ) -> Result<Option<Vec<OutlineEntry>>> {
        let indexed: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM files WHERE repository_id = ? AND git_ref = ? AND path = ?",
        )
        .bind(repository_id)
        .bind(git_ref)
        .bind(path)
        .fetch_optional(&self.read)
        .await?;
        if indexed.is_none() {
            return Ok(None);
        }
        Ok(Some(
            sqlx::query_as::<_, OutlineEntry>(
                "SELECT s.kind, s.name, s.start_line, s.end_line, s.signature
                 FROM files f JOIN symbols s ON s.blob = f.blob
                 WHERE f.repository_id = ? AND f.git_ref = ? AND f.path = ?
                 ORDER BY s.start_line, s.end_line DESC",
            )
            .bind(repository_id)
            .bind(git_ref)
            .bind(path)
            .fetch_all(&self.read)
            .await?,
        ))
    }

    /// Every definition of `name`, over the `(repository_id, git_ref)` pairs
    /// given, in path order.
    pub async fn definitions(
        &self,
        name: &str,
        scopes: &[(String, String)],
    ) -> Result<Vec<Definition>> {
        if scopes.is_empty() {
            return Ok(Vec::new());
        }
        let mut sql = QueryBuilder::<Sqlite>::new(
            "SELECT s.id, f.repository_id, f.git_ref, f.path, s.blob, s.start_line, s.end_line,
                    s.kind, s.name, s.signature, s.doc
             FROM symbols s JOIN files f ON f.blob = s.blob
             WHERE s.name = ",
        );
        sql.push_bind(name).push(" AND (");
        for (at, (repository_id, git_ref)) in scopes.iter().enumerate() {
            if at > 0 {
                sql.push(" OR ");
            }
            sql.push("(f.repository_id = ")
                .push_bind(repository_id)
                .push(" AND f.git_ref = ")
                .push_bind(git_ref)
                .push(")");
        }
        sql.push(") ORDER BY f.repository_id, f.path, s.start_line");
        Ok(sql
            .build_query_as::<Definition>()
            .fetch_all(&self.read)
            .await?)
    }

    /// What one definition is joined to at one ref: who calls it, what it
    /// calls, what implements it, what names it, and the tests that reach
    /// it. The other repositories' ends are in every list, after the
    /// definition's own.
    ///
    /// A test reaches it over at most two edges — the test itself, or a
    /// helper the test calls — which is what makes a test of a function
    /// found without the test naming it.
    pub async fn context(
        &self,
        symbol_id: i64,
        repository_id: &str,
        git_ref: &str,
        limit: i64,
    ) -> Result<SymbolContext> {
        let at =
            |query: RelatedQuery| self.related(query, symbol_id, repository_id, git_ref, limit);
        // Who calls it: the far end of a call into it.
        let (callers, callers_more) = at(RelatedQuery {
            listed: End::From,
            kinds: CALL_KINDS,
            ..Default::default()
        })
        .await?;
        // What it calls: the far end of a call out of it.
        let (callees, callees_more) = at(RelatedQuery {
            listed: End::To,
            kinds: CALL_KINDS,
            ..Default::default()
        })
        .await?;
        // What implements it, or what it is a base of.
        let (implementations, implementations_more) = at(RelatedQuery {
            listed: End::From,
            kinds: &["implements", "extends"],
            ..Default::default()
        })
        .await?;
        // What names it: a type annotation, an import, a foreign reference.
        let (references, references_more) = at(RelatedQuery {
            listed: End::From,
            kinds: &["references", "imports"],
            ..Default::default()
        })
        .await?;
        // The tests at most two edges away: a test that calls it, and a
        // test that calls something that calls it.
        let (tests, tests_more) = at(RelatedQuery {
            listed: End::From,
            kinds: CALL_KINDS,
            two_hops: true,
            tests_only: true,
        })
        .await?;
        Ok(SymbolContext {
            callers,
            callees,
            implementations,
            references,
            tests,
            more: ContextMore {
                callers: callers_more,
                callees: callees_more,
                implementations: implementations_more,
                references: references_more,
                tests: tests_more,
            },
        })
    }

    /// One side of the edges of a symbol, as the answer names them, and how
    /// many more matched past `limit`. The symbol sits at `repository_id`
    /// and `git_ref`; the listed end is the other one, wherever it is, and
    /// its file is read at its own repository and ref. The symbol's own
    /// repository is listed first.
    async fn related(
        &self,
        query: RelatedQuery,
        symbol_id: i64,
        repository_id: &str,
        git_ref: &str,
        limit: i64,
    ) -> Result<(Vec<Related>, i64)> {
        let (listed, listed_repository, listed_ref, other, other_repository, other_ref) =
            match query.listed {
                End::From => (
                    "from_symbol",
                    "from_repository",
                    "git_ref",
                    "to_symbol",
                    "to_repository",
                    "to_ref",
                ),
                End::To => (
                    "to_symbol",
                    "to_repository",
                    "to_ref",
                    "from_symbol",
                    "from_repository",
                    "git_ref",
                ),
            };
        let where_clause = |sql: &mut QueryBuilder<Sqlite>| {
            sql.push(listed)
                .push(" JOIN files f ON f.blob = s.blob AND f.repository_id = e.")
                .push(listed_repository)
                .push(" AND f.git_ref = e.")
                .push(listed_ref)
                .push(" WHERE e.")
                .push(other_repository)
                .push(" = ")
                .push_bind(repository_id)
                .push(" AND e.")
                .push(other_ref)
                .push(" = ")
                .push_bind(git_ref)
                .push(" AND ");
            push_kinds(sql, "e.", query.kinds);
            sql.push(" AND e.").push(other);
            match query.two_hops {
                false => {
                    sql.push(" = ").push_bind(symbol_id);
                }
                true => {
                    sql.push(" IN (SELECT ")
                        .push_bind(symbol_id)
                        .push(" UNION SELECT from_symbol FROM edges WHERE to_repository = ")
                        .push_bind(repository_id)
                        .push(" AND to_ref = ")
                        .push_bind(git_ref)
                        .push(" AND ");
                    push_kinds(sql, "", query.kinds);
                    sql.push(" AND from_symbol IS NOT NULL AND to_symbol = ")
                        .push_bind(symbol_id)
                        .push(")");
                }
            }
            if query.tests_only {
                sql.push(" AND s.is_test = 1");
            }
            sql.push(" AND s.id <> ").push_bind(symbol_id);
        };

        let mut count_sql = QueryBuilder::<Sqlite>::new(
            "SELECT COUNT(*) FROM (SELECT DISTINCT f.repository_id, f.path, s.start_line AS line, s.name, e.confidence
             FROM edges e JOIN symbols s ON s.id = e.",
        );
        where_clause(&mut count_sql);
        count_sql.push(")");
        let total: i64 = count_sql.build_query_scalar().fetch_one(&self.read).await?;

        let limit = limit.max(1);
        let mut sql = QueryBuilder::<Sqlite>::new(
            "SELECT DISTINCT f.repository_id, f.path, s.start_line AS line, s.name, e.confidence
             FROM edges e JOIN symbols s ON s.id = e.",
        );
        where_clause(&mut sql);
        sql.push(" ORDER BY (f.repository_id <> ")
            .push_bind(repository_id)
            .push("), f.repository_id, f.path, line LIMIT ")
            .push_bind(limit);
        let rows = sql
            .build_query_as::<Related>()
            .fetch_all(&self.read)
            .await?;
        Ok((rows, (total - limit).max(0)))
    }

    /// The callers of `symbol_id`, walked out to `depth`, with the symbols
    /// the walk did not go past. A call into the definition from another
    /// repository is followed like any other, and the walk goes on in that
    /// repository at the ref the edge names.
    ///
    /// One level is two statements per repository and ref it reaches: how
    /// many callers each symbol of the level has, and then the callers of
    /// the ones under [`MAX_FANOUT`]. Both read the `edges(kind, to_symbol,
    /// …)` index and nothing else, so the walk costs the callers it answers
    /// with and no more.
    pub async fn impact(
        &self,
        symbol_id: i64,
        repository_id: &str,
        git_ref: &str,
        depth: i64,
    ) -> Result<(Vec<ImpactCaller>, Vec<String>)> {
        let mut callers: Vec<ImpactCaller> = Vec::new();
        let mut stopped: Vec<String> = Vec::new();
        let mut seen: HashSet<i64> = HashSet::from([symbol_id]);
        // The symbols of the level, under the repository and ref each sits
        // at: the edges into a symbol are read at its own ref.
        let mut level: Vec<(i64, String, String)> =
            vec![(symbol_id, repository_id.to_string(), git_ref.to_string())];
        for at in 1..=depth.max(1) {
            if level.is_empty() {
                break;
            }
            let mut scopes: Vec<((String, String), Vec<i64>)> = Vec::new();
            for (id, repository, git_ref) in level {
                let scope = (repository, git_ref);
                match scopes.iter_mut().find(|(s, _)| *s == scope) {
                    Some((_, ids)) => ids.push(id),
                    None => scopes.push((scope, vec![id])),
                }
            }
            let mut next = Vec::new();
            for ((repository, git_ref), ids) in scopes {
                // How many callers each symbol of the level has, in one
                // statement: the index answers it without reading a row of
                // the table.
                let mut counts: HashMap<i64, i64> = HashMap::new();
                for chunk in ids.chunks(CHUNK) {
                    let mut sql = QueryBuilder::<Sqlite>::new(
                        "SELECT to_symbol, COUNT(*) FROM edges WHERE to_repository = ",
                    );
                    sql.push_bind(&repository)
                        .push(" AND to_ref = ")
                        .push_bind(&git_ref)
                        .push(" AND ");
                    push_kinds(&mut sql, "", CALL_KINDS);
                    sql.push(" AND to_symbol IN (");
                    let mut values = sql.separated(", ");
                    for id in chunk {
                        values.push_bind(id);
                    }
                    sql.push(") GROUP BY to_symbol");
                    let rows: Vec<(i64, i64)> = sql.build_query_as().fetch_all(&self.read).await?;
                    counts.extend(rows);
                }
                let mut wanted: Vec<i64> = Vec::new();
                for id in &ids {
                    match counts.get(id).copied().unwrap_or(0) > MAX_FANOUT {
                        true => stopped.push(self.name_of(*id).await?),
                        false => wanted.push(*id),
                    }
                }
                for chunk in wanted.chunks(CHUNK) {
                    let mut sql = QueryBuilder::<Sqlite>::new(
                        "SELECT DISTINCT s.id, f.repository_id, f.git_ref, f.path, s.start_line,
                                s.name, e.confidence
                         FROM edges e
                         JOIN symbols s ON s.id = e.from_symbol
                         JOIN files f ON f.blob = s.blob AND f.repository_id = e.from_repository
                                     AND f.git_ref = e.git_ref
                         WHERE e.to_repository = ",
                    );
                    sql.push_bind(&repository)
                        .push(" AND e.to_ref = ")
                        .push_bind(&git_ref)
                        .push(" AND ");
                    push_kinds(&mut sql, "e.", CALL_KINDS);
                    sql.push(" AND e.to_symbol IN (");
                    let mut values = sql.separated(", ");
                    for id in chunk {
                        values.push_bind(id);
                    }
                    sql.push(")");
                    let rows: Vec<(i64, String, String, String, i64, String, String)> =
                        sql.build_query_as().fetch_all(&self.read).await?;
                    for (id, repository, git_ref, path, line, name, confidence) in rows {
                        if !seen.insert(id) {
                            continue;
                        }
                        next.push((id, repository.clone(), git_ref));
                        callers.push(ImpactCaller {
                            depth: at,
                            repository_id: repository,
                            path,
                            line,
                            name,
                            confidence,
                        });
                    }
                }
            }
            level = next;
        }
        callers.sort_by(|a, b| {
            a.depth
                .cmp(&b.depth)
                .then_with(|| {
                    (a.repository_id != repository_id).cmp(&(b.repository_id != repository_id))
                })
                .then_with(|| a.repository_id.cmp(&b.repository_id))
                .then_with(|| a.path.cmp(&b.path))
                .then(a.line.cmp(&b.line))
        });
        stopped.sort();
        stopped.dedup();
        Ok((callers, stopped))
    }

    /// The shortest directed path from any definition in `starts` to any
    /// definition in `ends`, up to `depth` edges. The search grows from both
    /// sides, and reads each reached repository at the ref its edge names.
    pub async fn path(
        &self,
        starts: &[Definition],
        ends: &[Definition],
        depth: i64,
    ) -> Result<Vec<PathHop>> {
        if starts.is_empty() || ends.is_empty() {
            return Ok(Vec::new());
        }
        let mut forward: HashMap<PathKey, PathVisit> = starts
            .iter()
            .map(|definition| {
                let node = PathNode::from(definition);
                (node.key.clone(), PathVisit { node, link: None })
            })
            .collect();
        let mut backward: HashMap<PathKey, PathVisit> = ends
            .iter()
            .map(|definition| {
                let node = PathNode::from(definition);
                (node.key.clone(), PathVisit { node, link: None })
            })
            .collect();
        let mut forward_level: Vec<PathNode> =
            forward.values().map(|visit| visit.node.clone()).collect();
        let mut backward_level: Vec<PathNode> =
            backward.values().map(|visit| visit.node.clone()).collect();
        forward_level.sort_by(path_node_order);
        backward_level.sort_by(path_node_order);

        if let Some(meeting) = forward_level
            .iter()
            .find(|node| backward.contains_key(&node.key))
        {
            return Ok(join_path(&meeting.key, &forward, &backward));
        }

        let max_depth = depth.max(0);
        let mut forward_depth = 0;
        let mut backward_depth = 0;
        while forward_depth + backward_depth < max_depth {
            if forward_level.is_empty() && backward_level.is_empty() {
                break;
            }
            let grow_forward = !forward_level.is_empty()
                && (backward_level.is_empty() || forward_level.len() <= backward_level.len());
            let mut meeting = None;
            if grow_forward {
                forward_depth += 1;
                let mut next = Vec::new();
                for neighbor in self.path_neighbors(&forward_level, true).await? {
                    if forward.contains_key(&neighbor.far.key) {
                        continue;
                    }
                    let key = neighbor.far.key.clone();
                    forward.insert(
                        key.clone(),
                        PathVisit {
                            node: neighbor.far.clone(),
                            link: Some((neighbor.near, neighbor.edge)),
                        },
                    );
                    next.push(neighbor.far);
                    if meeting.is_none() && backward.contains_key(&key) {
                        meeting = Some(key);
                    }
                }
                forward_level = next;
            } else {
                backward_depth += 1;
                let mut next = Vec::new();
                for neighbor in self.path_neighbors(&backward_level, false).await? {
                    if backward.contains_key(&neighbor.far.key) {
                        continue;
                    }
                    let key = neighbor.far.key.clone();
                    backward.insert(
                        key.clone(),
                        PathVisit {
                            node: neighbor.far.clone(),
                            link: Some((neighbor.near, neighbor.edge)),
                        },
                    );
                    next.push(neighbor.far);
                    if meeting.is_none() && forward.contains_key(&key) {
                        meeting = Some(key);
                    }
                }
                backward_level = next;
            }
            if let Some(meeting) = meeting {
                return Ok(join_path(&meeting, &forward, &backward));
            }
        }
        Ok(Vec::new())
    }

    /// One level beside `level`. Forward reads edges out of the level;
    /// backward reads edges into it. Each query stays on an edge index.
    async fn path_neighbors(&self, level: &[PathNode], forward: bool) -> Result<Vec<PathNeighbor>> {
        let mut scopes: Vec<((String, String), Vec<i64>)> = Vec::new();
        for node in level {
            let scope = (node.key.repository_id.clone(), node.key.git_ref.clone());
            match scopes.iter_mut().find(|(found, _)| *found == scope) {
                Some((_, ids)) => ids.push(node.key.symbol_id),
                None => scopes.push((scope, vec![node.key.symbol_id])),
            }
        }
        let mut neighbors = Vec::new();
        for ((repository, git_ref), ids) in scopes {
            for chunk in ids.chunks(CHUNK) {
                let mut sql = match forward {
                    true => QueryBuilder::<Sqlite>::new(
                        "SELECT e.from_symbol AS near_symbol, s.id AS far_symbol,
                                f.repository_id AS far_repository, f.git_ref AS far_ref,
                                f.path, s.start_line AS line, s.kind, s.name,
                                e.kind AS edge_kind, e.confidence
                         FROM edges e
                         JOIN symbols s ON s.id = e.to_symbol
                         JOIN files f ON f.blob = s.blob AND f.repository_id = e.to_repository
                                     AND f.git_ref = e.to_ref
                         WHERE e.from_repository = ",
                    ),
                    false => QueryBuilder::<Sqlite>::new(
                        "SELECT e.to_symbol AS near_symbol, s.id AS far_symbol,
                                f.repository_id AS far_repository, f.git_ref AS far_ref,
                                f.path, s.start_line AS line, s.kind, s.name,
                                e.kind AS edge_kind, e.confidence
                         FROM edges e
                         JOIN symbols s ON s.id = e.from_symbol
                         JOIN files f ON f.blob = s.blob AND f.repository_id = e.from_repository
                                     AND f.git_ref = e.git_ref
                         WHERE e.to_repository = ",
                    ),
                };
                sql.push_bind(&repository)
                    .push(if forward {
                        " AND e.git_ref = "
                    } else {
                        " AND e.to_ref = "
                    })
                    .push_bind(&git_ref)
                    .push(" AND ");
                push_kinds(&mut sql, "e.", PATH_KINDS);
                sql.push(if forward {
                    " AND e.from_symbol IN ("
                } else {
                    " AND e.to_symbol IN ("
                });
                let mut values = sql.separated(", ");
                for id in chunk {
                    values.push_bind(id);
                }
                sql.push(") ORDER BY near_symbol, edge_kind, far_repository, path, line");
                let rows: Vec<PathEdgeRow> = sql.build_query_as().fetch_all(&self.read).await?;
                neighbors.extend(rows.into_iter().map(|row| PathNeighbor {
                    near: PathKey {
                        symbol_id: row.near_symbol,
                        repository_id: repository.clone(),
                        git_ref: git_ref.clone(),
                    },
                    far: PathNode {
                        key: PathKey {
                            symbol_id: row.far_symbol,
                            repository_id: row.far_repository,
                            git_ref: row.far_ref,
                        },
                        path: row.path,
                        line: row.line,
                        kind: row.kind,
                        name: row.name,
                    },
                    edge: PathEdge {
                        kind: row.edge_kind,
                        confidence: row.confidence,
                    },
                }));
            }
        }
        Ok(neighbors)
    }

    /// The edges between one ref of a repository and every other repository,
    /// in both directions, each end at its own path: what
    /// `GET /v1/knowledge/interactions` lists. Grouped by kind in the order
    /// of [`INTERACTION_KINDS`], then by the from end.
    pub async fn interactions(
        &self,
        repository_id: &str,
        git_ref: &str,
    ) -> Result<Vec<Interaction>> {
        let rows: Vec<InteractionRow> = sqlx::query_as(
            "SELECT DISTINCT e.kind,
                    e.from_repository, ff.path AS from_path, e.from_line,
                    COALESCE(sf.name, e.name, '') AS from_symbol,
                    e.to_repository, ft.path AS to_path, e.to_line,
                    COALESCE(st.name, e.name, '') AS to_symbol,
                    e.confidence
             FROM edges e
             JOIN files ff ON ff.blob = e.from_blob AND ff.repository_id = e.from_repository
                          AND ff.git_ref = e.git_ref
             JOIN files ft ON ft.blob = e.to_blob AND ft.repository_id = e.to_repository
                          AND ft.git_ref = e.to_ref
             LEFT JOIN symbols sf ON sf.id = e.from_symbol
             LEFT JOIN symbols st ON st.id = e.to_symbol
             WHERE e.from_repository <> e.to_repository
               AND ((e.from_repository = ?1 AND e.git_ref = ?2)
                    OR (e.to_repository = ?1 AND e.to_ref = ?2))
             ORDER BY e.from_repository, ff.path, e.from_line, e.to_repository, ft.path, e.to_line",
        )
        .bind(repository_id)
        .bind(git_ref)
        .fetch_all(&self.read)
        .await?;
        let mut found: Vec<Interaction> = rows
            .into_iter()
            .map(|row| Interaction {
                kind: row.kind,
                from: InteractionEnd {
                    repository_id: row.from_repository,
                    path: row.from_path,
                    line: row.from_line,
                    symbol: row.from_symbol,
                },
                to: InteractionEnd {
                    repository_id: row.to_repository,
                    path: row.to_path,
                    line: row.to_line,
                    symbol: row.to_symbol,
                },
                confidence: row.confidence,
            })
            .collect();
        let rank = |kind: &str| {
            INTERACTION_KINDS
                .iter()
                .position(|k| *k == kind)
                .unwrap_or(INTERACTION_KINDS.len())
        };
        found.sort_by_key(|interaction| rank(&interaction.kind));
        Ok(found)
    }

    /// The map of one ref: its files ranked by PageRank over the references
    /// between them, each with the definitions most of the ref points at,
    /// rendered as plain text under `budget` tokens.
    ///
    /// `toward` restarts the walk at one file, so a map asked for around a
    /// path names that file's neighbors first. A file that defines nothing
    /// is left out: a heading with no definition under it says nothing.
    pub async fn map(
        &self,
        repository_id: &str,
        git_ref: &str,
        toward: Option<&str>,
        budget: usize,
    ) -> Result<RepoMap> {
        let paths: Vec<String> = sqlx::query_scalar(
            "SELECT path FROM files WHERE repository_id = ? AND git_ref = ? ORDER BY path",
        )
        .bind(repository_id)
        .bind(git_ref)
        .fetch_all(&self.read)
        .await?;
        if paths.is_empty() {
            return Ok(RepoMap::default());
        }
        let at: HashMap<&str, usize> = paths
            .iter()
            .enumerate()
            .map(|(at, path)| (path.as_str(), at))
            .collect();
        // One row per pair of files, however many references join them. The
        // map is of one ref of one repository, so an edge to another one is
        // no edge of this graph, and of the symbol edges alone.
        let mut sql = QueryBuilder::<Sqlite>::new(
            "SELECT ff.path, ft.path, COUNT(*)
             FROM edges e
             JOIN files ff ON ff.blob = e.from_blob AND ff.repository_id = e.from_repository
                          AND ff.git_ref = e.git_ref
             JOIN files ft ON ft.blob = e.to_blob AND ft.repository_id = e.to_repository
                          AND ft.git_ref = e.to_ref
             WHERE e.from_repository = ",
        );
        sql.push_bind(repository_id)
            .push(" AND e.git_ref = ")
            .push_bind(git_ref)
            .push(" AND e.to_repository = ")
            .push_bind(repository_id)
            .push(" AND e.to_ref = ")
            .push_bind(git_ref)
            .push(" AND ff.path <> ft.path AND ");
        push_kinds(&mut sql, "e.", SYMBOL_KINDS);
        sql.push(" GROUP BY ff.path, ft.path");
        let joined: Vec<(String, String, i64)> = sql.build_query_as().fetch_all(&self.read).await?;
        let edges: Vec<(usize, usize, f64)> = joined
            .iter()
            .filter_map(|(from, to, count)| {
                Some(map::both_ways(
                    *at.get(from.as_str())?,
                    *at.get(to.as_str())?,
                    *count as f64,
                ))
            })
            .flatten()
            .collect();
        let rank = map::page_rank(
            paths.len(),
            &edges,
            toward.and_then(|path| at.get(path).copied()),
        );
        let mut ranked: Vec<usize> = (0..paths.len()).collect();
        ranked.sort_by(|a, b| {
            rank[*b]
                .partial_cmp(&rank[*a])
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| paths[*a].cmp(&paths[*b]))
        });
        ranked.truncate(MAP_FILES);
        let wanted: Vec<&str> = ranked.iter().map(|at| paths[*at].as_str()).collect();
        let mut named = self.map_symbols(repository_id, git_ref, &wanted).await?;
        let files: Vec<MapFile> = ranked
            .into_iter()
            .filter_map(|at| {
                let symbols = named.remove(paths[at].as_str())?;
                Some(MapFile {
                    path: paths[at].clone(),
                    rank: rank[at],
                    symbols,
                })
            })
            .collect();
        Ok(map::render(&files, budget))
    }

    /// The definitions a map names per file: the ones most of the ref points
    /// at over [`SYMBOL_KINDS`], in line order, [`MAP_SYMBOLS`] at most. A
    /// file that defines nothing is absent.
    async fn map_symbols(
        &self,
        repository_id: &str,
        git_ref: &str,
        paths: &[&str],
    ) -> Result<HashMap<String, Vec<OutlineEntry>>> {
        let mut found: HashMap<String, Vec<(i64, OutlineEntry)>> = HashMap::new();
        for chunk in paths.chunks(CHUNK) {
            let mut sql = QueryBuilder::<Sqlite>::new(
                "SELECT f.path, s.kind, s.name, s.start_line, s.end_line, s.signature,
                        (SELECT COUNT(*) FROM edges e
                          WHERE e.to_repository = f.repository_id AND e.to_ref = f.git_ref
                            AND e.to_symbol = s.id AND ",
            );
            push_kinds(&mut sql, "e.", SYMBOL_KINDS);
            sql.push(
                ") AS uses
                 FROM files f JOIN symbols s ON s.blob = f.blob
                 WHERE f.repository_id = ",
            );
            sql.push_bind(repository_id)
                .push(" AND f.git_ref = ")
                .push_bind(git_ref)
                .push(" AND f.path IN (");
            let mut values = sql.separated(", ");
            for path in chunk {
                values.push_bind(*path);
            }
            sql.push(")");
            let rows: Vec<(String, String, String, i64, i64, String, i64)> =
                sql.build_query_as().fetch_all(&self.read).await?;
            for (path, kind, name, start_line, end_line, signature, uses) in rows {
                found.entry(path).or_default().push((
                    uses,
                    OutlineEntry {
                        kind,
                        name,
                        start_line,
                        end_line,
                        signature,
                    },
                ));
            }
        }
        Ok(found
            .into_iter()
            .map(|(path, mut symbols)| {
                symbols.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.start_line.cmp(&b.1.start_line)));
                symbols.truncate(MAP_SYMBOLS);
                symbols.sort_by_key(|(_, symbol)| symbol.start_line);
                (
                    path,
                    symbols.into_iter().map(|(_, symbol)| symbol).collect(),
                )
            })
            .collect())
    }

    /// The name of one symbol, or `?` for one that is gone.
    async fn name_of(&self, symbol_id: i64) -> Result<String> {
        Ok(
            sqlx::query_scalar::<_, String>("SELECT name FROM symbols WHERE id = ?")
                .bind(symbol_id)
                .fetch_optional(&self.read)
                .await?
                .unwrap_or_else(|| "?".to_string()),
        )
    }

    /// The definitions of one ref whose lines overlap what a diff changed.
    pub async fn symbols_in_lines(
        &self,
        repository_id: &str,
        git_ref: &str,
        changed: &[(String, Vec<(u32, u32)>)],
    ) -> Result<Vec<(i64, Related)>> {
        let mut found = Vec::new();
        for (path, ranges) in changed {
            if ranges.is_empty() {
                continue;
            }
            let mut sql = QueryBuilder::<Sqlite>::new(
                "SELECT s.id, f.repository_id, f.path, s.start_line AS line, s.name
                 FROM files f JOIN symbols s ON s.blob = f.blob
                 WHERE f.repository_id = ",
            );
            sql.push_bind(repository_id)
                .push(" AND f.git_ref = ")
                .push_bind(git_ref)
                .push(" AND f.path = ")
                .push_bind(path)
                .push(" AND (");
            for (at, (first, last)) in ranges.iter().enumerate() {
                if at > 0 {
                    sql.push(" OR ");
                }
                sql.push("(s.start_line <= ")
                    .push_bind(*last as i64)
                    .push(" AND s.end_line >= ")
                    .push_bind(*first as i64)
                    .push(")");
            }
            sql.push(") ORDER BY s.start_line");
            let rows: Vec<(i64, String, String, i64, String)> =
                sql.build_query_as().fetch_all(&self.read).await?;
            for (id, repository, path, line, name) in rows {
                found.push((
                    id,
                    Related {
                        repository_id: repository,
                        path,
                        line,
                        name,
                        confidence: "exact".into(),
                    },
                ));
            }
        }
        Ok(found)
    }

    // -- writes of the indexer ------------------------------------------------

    /// Which of `blobs` have been parsed already.
    pub(crate) async fn known_blobs(&self, blobs: &[String]) -> Result<HashSet<String>> {
        let mut known = HashSet::new();
        for chunk in blobs.chunks(CHUNK) {
            let mut sql = QueryBuilder::<Sqlite>::new("SELECT blob FROM blobs WHERE blob IN (");
            let mut values = sql.separated(", ");
            for blob in chunk {
                values.push_bind(blob);
            }
            sql.push(")");
            let found: Vec<String> = sql.build_query_scalar().fetch_all(&self.read).await?;
            known.extend(found);
        }
        Ok(known)
    }

    /// Store the definitions of blobs parsed for the first time.
    ///
    /// Symbols are given their ids here and written many rows to a
    /// statement: a repository is thousands of them, and one statement per
    /// symbol was most of what indexing one cost.
    pub(crate) async fn commit_blobs(&self, parsed: &[ParsedBlob]) -> Result<()> {
        let mut tx = self.write.begin().await?;
        let stamp = now();
        // Past the highest id ever given, not the highest still there: a
        // dropped symbol's id is never handed to a new one.
        let mut next_id: i64 = sqlx::query_scalar(
            "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name = 'symbols'), 0) + 1",
        )
        .fetch_one(&mut *tx)
        .await?;
        let mut rows: Vec<(i64, &ParsedBlob, &Symbol)> = Vec::new();
        // The blobs this run is the first to store, and the id the first
        // symbol of each one was given: a reference names the definition it
        // sits in by its place in the blob, which is that id plus its index.
        let mut fresh: Vec<&ParsedBlob> = Vec::new();
        let mut first_id: HashMap<&str, i64> = HashMap::new();
        for blob in parsed {
            let inserted = sqlx::query(
                "INSERT OR IGNORE INTO blobs (blob, language, parsed_at) VALUES (?, ?, ?)",
            )
            .bind(&blob.blob)
            .bind(blob.language)
            .bind(&stamp)
            .execute(&mut *tx)
            .await?;
            if inserted.rows_affected() == 0 {
                // Parsed by an earlier run: its symbols are already here.
                continue;
            }
            fresh.push(blob);
            if !blob.symbols.is_empty() {
                first_id.insert(blob.blob.as_str(), next_id);
            }
            for symbol in &blob.symbols {
                rows.push((next_id, blob, symbol));
                next_id += 1;
            }
        }
        // Ten values a row, so a chunk stays well under SQLite's bind limit.
        for chunk in rows.chunks(CHUNK / 10) {
            let mut sql = QueryBuilder::<Sqlite>::new(
                "INSERT INTO symbols (id, blob, kind, name, qualified_name, start_line, end_line,
                                      signature, doc, is_test) ",
            );
            sql.push_values(chunk, |mut row, (id, blob, symbol)| {
                row.push_bind(*id)
                    .push_bind(&blob.blob)
                    .push_bind(&symbol.kind)
                    .push_bind(&symbol.name)
                    .push_bind(&symbol.qualified_name)
                    .push_bind(symbol.start_line as i64)
                    .push_bind(symbol.end_line as i64)
                    .push_bind(&symbol.signature)
                    .push_bind(&symbol.doc)
                    .push_bind(symbol.is_test as i64);
            });
            sql.build().execute(&mut *tx).await?;
        }
        for chunk in rows.chunks(CHUNK / 2) {
            let mut sql = QueryBuilder::<Sqlite>::new("INSERT INTO symbols_fts (rowid, terms) ");
            sql.push_values(chunk, |mut row, (id, _, symbol)| {
                row.push_bind(*id).push_bind(terms_of(symbol));
            });
            sql.build().execute(&mut *tx).await?;
        }
        // The names each blob names, with the id of the definition each one
        // sits in: what the resolution pass turns into edges, now and for
        // every later run over this blob.
        let mentions: Vec<(&ParsedBlob, &Reference)> = fresh
            .iter()
            .flat_map(|blob| blob.references.iter().map(move |r| (*blob, r)))
            .collect();
        for chunk in mentions.chunks(CHUNK / 5) {
            let mut sql = QueryBuilder::<Sqlite>::new(
                "INSERT INTO mentions (blob, kind, name, line, from_symbol) ",
            );
            sql.push_values(chunk, |mut row, (blob, reference)| {
                row.push_bind(&blob.blob)
                    .push_bind(reference.kind.as_str())
                    .push_bind(&reference.name)
                    .push_bind(reference.line as i64)
                    .push_bind(
                        reference
                            .from
                            .and_then(|at| Some(first_id.get(blob.blob.as_str())? + at as i64)),
                    );
            });
            sql.build().execute(&mut *tx).await?;
        }
        let imports: Vec<(&ParsedBlob, &Import)> = fresh
            .iter()
            .flat_map(|blob| blob.imports.iter().map(move |i| (*blob, i)))
            .collect();
        for chunk in imports.chunks(CHUNK / 4) {
            let mut sql =
                QueryBuilder::<Sqlite>::new("INSERT INTO imports (blob, module, name, line) ");
            sql.push_values(chunk, |mut row, (blob, import)| {
                row.push_bind(&blob.blob)
                    .push_bind(&import.module)
                    .push_bind(&import.name)
                    .push_bind(import.line as i64);
            });
            sql.build().execute(&mut *tx).await?;
        }
        // What each blob offers or takes: the link pass matches these across
        // repositories, now and for every later run.
        let interfaces: Vec<(&ParsedBlob, &Interface)> = fresh
            .iter()
            .flat_map(|blob| blob.interfaces.iter().map(move |i| (*blob, i)))
            .collect();
        for chunk in interfaces.chunks(CHUNK / 6) {
            let mut sql = QueryBuilder::<Sqlite>::new(
                "INSERT INTO interfaces (blob, kind, name, line, symbol, handlers) ",
            );
            sql.push_values(chunk, |mut row, (blob, interface)| {
                row.push_bind(&blob.blob)
                    .push_bind(interface.kind.as_str())
                    .push_bind(&interface.name)
                    .push_bind(interface.line as i64)
                    .push_bind(
                        interface
                            .symbol
                            .and_then(|at| Some(first_id.get(blob.blob.as_str())? + at as i64)),
                    )
                    .push_bind(match interface.handlers.is_empty() {
                        true => None,
                        false => Some(interface.handlers.join(" ")),
                    });
            });
            sql.build().execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    // -- the resolution pass --------------------------------------------------

    /// The names the blobs at `paths` define, at one ref.
    pub(crate) async fn names_at(
        &self,
        repository_id: &str,
        git_ref: &str,
        paths: &[String],
    ) -> Result<HashSet<String>> {
        let mut names = HashSet::new();
        for chunk in paths.chunks(CHUNK) {
            let mut sql = QueryBuilder::<Sqlite>::new(
                "SELECT DISTINCT s.name FROM files f JOIN symbols s ON s.blob = f.blob
                 WHERE f.repository_id = ",
            );
            sql.push_bind(repository_id)
                .push(" AND f.git_ref = ")
                .push_bind(git_ref)
                .push(" AND f.path IN (");
            let mut values = sql.separated(", ");
            for path in chunk {
                values.push_bind(path);
            }
            sql.push(")");
            let found: Vec<String> = sql.build_query_scalar().fetch_all(&self.read).await?;
            names.extend(found);
        }
        Ok(names)
    }

    /// The blobs of one ref that name any of `names`, in a mention or in an
    /// import: the blobs whose edges a change to those definitions moves.
    pub(crate) async fn blobs_naming(
        &self,
        repository_id: &str,
        git_ref: &str,
        names: &[String],
    ) -> Result<HashSet<String>> {
        let mut blobs = HashSet::new();
        for chunk in names.chunks(CHUNK) {
            let mut sql = QueryBuilder::<Sqlite>::new(
                "SELECT DISTINCT f.blob FROM files f
                 WHERE f.repository_id = ",
            );
            sql.push_bind(repository_id)
                .push(" AND f.git_ref = ")
                .push_bind(git_ref)
                .push(
                    " AND (EXISTS (SELECT 1 FROM mentions m WHERE m.blob = f.blob AND m.name IN (",
                );
            let mut values = sql.separated(", ");
            for name in chunk {
                values.push_bind(name);
            }
            sql.push(")) OR EXISTS (SELECT 1 FROM imports i WHERE i.blob = f.blob AND i.name IN (");
            let mut values = sql.separated(", ");
            for name in chunk {
                values.push_bind(name);
            }
            sql.push(")))");
            let found: Vec<String> = sql.build_query_scalar().fetch_all(&self.read).await?;
            blobs.extend(found);
        }
        Ok(blobs)
    }

    /// What each of `blobs` names, and the path it sits at in this ref.
    pub(crate) async fn names_of(
        &self,
        repository_id: &str,
        git_ref: &str,
        blobs: &[String],
    ) -> Result<HashMap<String, Names>> {
        let mut named: HashMap<String, Names> = HashMap::new();
        for chunk in blobs.chunks(CHUNK) {
            // Only a blob this ref holds: its path is what step two of the
            // resolution order compares directories with.
            let mut sql =
                QueryBuilder::<Sqlite>::new("SELECT blob, path FROM files WHERE repository_id = ");
            sql.push_bind(repository_id)
                .push(" AND git_ref = ")
                .push_bind(git_ref)
                .push(" AND");
            let paths: Vec<(String, String)> = in_blobs(sql, chunk)
                .build_query_as()
                .fetch_all(&self.read)
                .await?;
            for (blob, path) in paths {
                named.entry(blob).or_default().path = path;
            }
            let sql = QueryBuilder::<Sqlite>::new(
                "SELECT blob, kind, name, line, from_symbol FROM mentions WHERE",
            );
            let mentions: Vec<(String, String, String, i64, Option<i64>)> = in_blobs(sql, chunk)
                .build_query_as()
                .fetch_all(&self.read)
                .await?;
            for (blob, kind, name, line, from_symbol) in mentions {
                if let Some(names) = named.get_mut(&blob) {
                    names.mentions.push(Mention {
                        kind,
                        name,
                        line,
                        from_symbol,
                    });
                }
            }
            let sql =
                QueryBuilder::<Sqlite>::new("SELECT blob, module, name, line FROM imports WHERE");
            let imports: Vec<(String, String, Option<String>, i64)> = in_blobs(sql, chunk)
                .build_query_as()
                .fetch_all(&self.read)
                .await?;
            for (blob, module, name, line) in imports {
                if let Some(names) = named.get_mut(&blob) {
                    names.imports.push(Import {
                        module,
                        name,
                        line: line as u32,
                    });
                }
            }
        }
        Ok(named)
    }

    /// Every definition of each of `names`, at one ref.
    pub(crate) async fn candidates(
        &self,
        repository_id: &str,
        git_ref: &str,
        names: &[String],
    ) -> Result<HashMap<String, Vec<Candidate>>> {
        let mut candidates: HashMap<String, Vec<Candidate>> = HashMap::new();
        for chunk in names.chunks(CHUNK) {
            let mut sql = QueryBuilder::<Sqlite>::new(
                "SELECT s.name, s.id, s.blob, f.path, f.language, s.qualified_name, s.start_line
                 FROM symbols s JOIN files f ON f.blob = s.blob
                 WHERE f.repository_id = ",
            );
            sql.push_bind(repository_id)
                .push(" AND f.git_ref = ")
                .push_bind(git_ref)
                .push(" AND s.name IN (");
            let mut values = sql.separated(", ");
            for name in chunk {
                values.push_bind(name);
            }
            sql.push(")");
            let found: Vec<(String, i64, String, String, String, String, i64)> =
                sql.build_query_as().fetch_all(&self.read).await?;
            for (name, id, blob, path, language, qualified_name, start_line) in found {
                candidates.entry(name).or_default().push(Candidate {
                    id,
                    blob,
                    path,
                    language,
                    qualified_name,
                    start_line,
                });
            }
        }
        Ok(candidates)
    }

    /// Replace the edges of `blobs` with `edges`, in one transaction: a
    /// blob's edges are all derived together or not at all. Both ends are
    /// this repository at this ref.
    pub(crate) async fn commit_edges(
        &self,
        repository_id: &str,
        git_ref: &str,
        blobs: &[String],
        edges: &[Edge],
    ) -> Result<()> {
        let mut tx = self.write.begin().await?;
        for chunk in blobs.chunks(CHUNK) {
            let mut sql = QueryBuilder::<Sqlite>::new("DELETE FROM edges WHERE from_repository = ");
            sql.push_bind(repository_id)
                .push(" AND git_ref = ")
                .push_bind(git_ref)
                .push(" AND from_blob IN (");
            let mut values = sql.separated(", ");
            for blob in chunk {
                values.push_bind(blob);
            }
            sql.push(")");
            sql.build().execute(&mut *tx).await?;
        }
        insert_edges(
            &mut tx,
            repository_id,
            git_ref,
            repository_id,
            git_ref,
            edges,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    // -- the link pass --------------------------------------------------------

    /// Every interface of one ref, with the path each sits at.
    pub(crate) async fn interfaces_at(
        &self,
        repository_id: &str,
        git_ref: &str,
    ) -> Result<Vec<InterfaceRow>> {
        Ok(sqlx::query_as::<_, InterfaceRow>(
            "SELECT f.blob, f.path, i.kind, i.name, i.line, i.symbol, i.handlers
             FROM files f JOIN interfaces i ON i.blob = f.blob
             WHERE f.repository_id = ? AND f.git_ref = ?
             ORDER BY f.path, i.line",
        )
        .bind(repository_id)
        .bind(git_ref)
        .fetch_all(&self.read)
        .await?)
    }

    /// Derive the edges between one ref and every other repository at its
    /// base ref, in both directions, and the interface edges within the ref
    /// itself: the whole set for each pair, replaced as one.
    ///
    /// Answers how many edges the pass wrote.
    pub(crate) async fn link(&self, repository_id: &str, git_ref: &str) -> Result<usize> {
        let own = self.interfaces_at(repository_id, git_ref).await?;
        let others: Vec<(String, String, Vec<InterfaceRow>)> = {
            let mut others = Vec::new();
            for (other, base) in self.base_refs().await? {
                if other == repository_id {
                    continue;
                }
                let rows = self.interfaces_at(&other, &base).await?;
                others.push((other, base, rows));
            }
            others
        };
        let scope = |repository: &str, git_ref: &str| (repository.to_string(), git_ref.to_string());
        let mut pairs: Vec<((String, String), (String, String))> =
            vec![(scope(repository_id, git_ref), scope(repository_id, git_ref))];
        for (other, base, _) in &others {
            pairs.push((scope(repository_id, git_ref), scope(other, base)));
            pairs.push((scope(other, base), scope(repository_id, git_ref)));
        }
        let rows_of = |repository: &str, at_ref: &str| -> &[InterfaceRow] {
            if repository == repository_id && at_ref == git_ref {
                return &own;
            }
            others
                .iter()
                .find(|(other, base, _)| other == repository && base == at_ref)
                .map_or(&[][..], |(_, _, rows)| rows)
        };
        let mut written = 0;
        // The interface edges first: whether one repository depends on
        // another is what the foreign references are then narrowed by.
        for ((from_repository, from_ref), (to_repository, to_ref)) in &pairs {
            let from = rows_of(from_repository, from_ref);
            let to = rows_of(to_repository, to_ref);
            let handler_names: Vec<String> = to
                .iter()
                .filter(|row| row.kind == "route")
                .flat_map(|row| {
                    row.handlers
                        .as_deref()
                        .unwrap_or("")
                        .split_whitespace()
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .collect();
            let handlers = match handler_names.is_empty() {
                true => HashMap::new(),
                false => {
                    self.candidates(to_repository, to_ref, &handler_names)
                        .await?
                }
            };
            let edges = crate::resolve::interface_edges(from, to, &handlers);
            written += edges.len();
            self.commit_links(
                (from_repository, from_ref),
                (to_repository, to_ref),
                &INTERFACE_KINDS,
                &edges,
            )
            .await?;
        }
        // Then the names one repository uses and another defines.
        let mut unresolved: HashMap<(String, String), Vec<String>> = HashMap::new();
        for ((from_repository, from_ref), (to_repository, to_ref)) in &pairs {
            if from_repository == to_repository {
                continue;
            }
            let key = scope(from_repository, from_ref);
            if !unresolved.contains_key(&key) {
                let names = self.unresolved_names(from_repository, from_ref).await?;
                unresolved.insert(key.clone(), names);
            }
            let names = &unresolved[&key];
            let mut defined = self.candidates(to_repository, to_ref, names).await?;
            // A repository the manifest depends on is preferred: where the
            // name is also defined in one this repository depends on, and
            // this one is not, the name means that one's definition.
            let linked = self.linked_repositories(from_repository, from_ref).await?;
            if !linked.contains(to_repository) && !linked.is_empty() {
                let names: Vec<String> = defined.keys().cloned().collect();
                let elsewhere = self.defining_repositories(&names).await?;
                defined.retain(|name, _| {
                    !elsewhere
                        .get(name)
                        .is_some_and(|repositories| repositories.iter().any(|r| linked.contains(r)))
                });
            }
            let names: Vec<String> = defined.keys().cloned().collect();
            let mentions = self
                .mentions_named(from_repository, from_ref, &names)
                .await?;
            let edges = crate::resolve::foreign_edges(&mentions, &defined);
            written += edges.len();
            self.commit_links(
                (from_repository, from_ref),
                (to_repository, to_ref),
                &["references"],
                &edges,
            )
            .await?;
        }
        Ok(written)
    }

    /// Replace the edges of `kinds` from one ref to another with `edges`.
    async fn commit_links(
        &self,
        from: (&str, &str),
        to: (&str, &str),
        kinds: &[&str],
        edges: &[Edge],
    ) -> Result<()> {
        let mut tx = self.write.begin().await?;
        let mut sql = QueryBuilder::<Sqlite>::new("DELETE FROM edges WHERE from_repository = ");
        sql.push_bind(from.0)
            .push(" AND git_ref = ")
            .push_bind(from.1)
            .push(" AND to_repository = ")
            .push_bind(to.0)
            .push(" AND to_ref = ")
            .push_bind(to.1)
            .push(" AND ");
        push_kinds(&mut sql, "", kinds);
        sql.build().execute(&mut *tx).await?;
        insert_edges(&mut tx, from.0, from.1, to.0, to.1, edges).await?;
        tx.commit().await?;
        Ok(())
    }

    /// The names one ref mentions or imports and defines nowhere, at least
    /// [`MIN_FOREIGN_NAME`] long: what may be defined in another repository.
    async fn unresolved_names(&self, repository_id: &str, git_ref: &str) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT DISTINCT m.name FROM files f JOIN mentions m ON m.blob = f.blob
             WHERE f.repository_id = ?1 AND f.git_ref = ?2 AND length(m.name) >= ?3
               AND NOT EXISTS (SELECT 1 FROM symbols s JOIN files g ON g.blob = s.blob
                               WHERE s.name = m.name AND g.repository_id = ?1 AND g.git_ref = ?2)
             UNION
             SELECT DISTINCT i.name FROM files f JOIN imports i ON i.blob = f.blob
             WHERE f.repository_id = ?1 AND f.git_ref = ?2 AND i.name IS NOT NULL
               AND length(i.name) >= ?3
               AND NOT EXISTS (SELECT 1 FROM symbols s JOIN files g ON g.blob = s.blob
                               WHERE s.name = i.name AND g.repository_id = ?1 AND g.git_ref = ?2)",
        )
        .bind(repository_id)
        .bind(git_ref)
        .bind(MIN_FOREIGN_NAME as i64)
        .fetch_all(&self.read)
        .await?)
    }

    /// Every mention and named import of `names` at one ref, keyed by name,
    /// each with the blob it sits in.
    async fn mentions_named(
        &self,
        repository_id: &str,
        git_ref: &str,
        names: &[String],
    ) -> Result<HashMap<String, Vec<(String, Mention)>>> {
        let mut found: HashMap<String, Vec<(String, Mention)>> = HashMap::new();
        for chunk in names.chunks(CHUNK / 2) {
            let mut sql = QueryBuilder::<Sqlite>::new(
                "SELECT DISTINCT m.blob, m.kind, m.name, m.line, m.from_symbol
                 FROM files f JOIN mentions m ON m.blob = f.blob
                 WHERE f.repository_id = ",
            );
            sql.push_bind(repository_id)
                .push(" AND f.git_ref = ")
                .push_bind(git_ref)
                .push(" AND m.name IN (");
            let mut values = sql.separated(", ");
            for name in chunk {
                values.push_bind(name);
            }
            sql.push(
                ") UNION SELECT DISTINCT i.blob, 'imports', i.name, i.line, NULL
                       FROM files f JOIN imports i ON i.blob = f.blob WHERE f.repository_id = ",
            )
            .push_bind(repository_id)
            .push(" AND f.git_ref = ")
            .push_bind(git_ref)
            .push(" AND i.name IN (");
            let mut values = sql.separated(", ");
            for name in chunk {
                values.push_bind(name);
            }
            sql.push(")");
            let rows: Vec<(String, String, String, i64, Option<i64>)> =
                sql.build_query_as().fetch_all(&self.read).await?;
            for (blob, kind, name, line, from_symbol) in rows {
                found.entry(name.clone()).or_default().push((
                    blob,
                    Mention {
                        kind,
                        name,
                        line,
                        from_symbol,
                    },
                ));
            }
        }
        Ok(found)
    }

    /// The repositories one ref depends on, by its manifests.
    async fn linked_repositories(
        &self,
        repository_id: &str,
        git_ref: &str,
    ) -> Result<HashSet<String>> {
        let found: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT to_repository FROM edges
             WHERE from_repository = ?1 AND git_ref = ?2 AND kind = 'depends_on'
               AND to_repository <> ?1",
        )
        .bind(repository_id)
        .bind(git_ref)
        .fetch_all(&self.read)
        .await?;
        Ok(found.into_iter().collect())
    }

    /// The repositories that define each of `names` at their base ref.
    async fn defining_repositories(
        &self,
        names: &[String],
    ) -> Result<HashMap<String, HashSet<String>>> {
        let mut found: HashMap<String, HashSet<String>> = HashMap::new();
        for chunk in names.chunks(CHUNK) {
            let mut sql = QueryBuilder::<Sqlite>::new(
                "SELECT DISTINCT s.name, f.repository_id
                 FROM symbols s JOIN files f ON f.blob = s.blob
                 JOIN repositories r ON r.id = f.repository_id AND r.base_ref = f.git_ref
                 WHERE s.name IN (",
            );
            let mut values = sql.separated(", ");
            for name in chunk {
                values.push_bind(name);
            }
            sql.push(")");
            let rows: Vec<(String, String)> = sql.build_query_as().fetch_all(&self.read).await?;
            for (name, repository) in rows {
                found.entry(name).or_default().insert(repository);
            }
        }
        Ok(found)
    }

    /// Record the files of a ref at a commit, once their blobs are stored.
    pub(crate) async fn commit_files(
        &self,
        repository_id: &str,
        git_ref: &str,
        commit: &str,
        changes: &FileChanges,
    ) -> Result<()> {
        let mut tx = self.write.begin().await?;
        let stamp = now();
        // The first ref indexed is the base ref until the daemon says which
        // one is.
        sqlx::query(
            "INSERT INTO repositories (id, state, error, updated_at, base_ref) VALUES (?1, 'idle', NULL, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET base_ref = COALESCE(base_ref, excluded.base_ref)",
        )
        .bind(repository_id)
        .bind(&stamp)
        .bind(git_ref)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO refs (repository_id, git_ref, commit_sha, indexed_at) VALUES (?, ?, ?, ?)
             ON CONFLICT(repository_id, git_ref) DO UPDATE SET commit_sha = excluded.commit_sha,
                                                              indexed_at = excluded.indexed_at",
        )
        .bind(repository_id)
        .bind(git_ref)
        .bind(commit)
        .bind(&stamp)
        .execute(&mut *tx)
        .await?;
        if changes.replace_all {
            sqlx::query("DELETE FROM files WHERE repository_id = ? AND git_ref = ?")
                .bind(repository_id)
                .bind(git_ref)
                .execute(&mut *tx)
                .await?;
        }
        for path in &changes.removed {
            sqlx::query("DELETE FROM files WHERE repository_id = ? AND git_ref = ? AND path = ?")
                .bind(repository_id)
                .bind(git_ref)
                .bind(path)
                .execute(&mut *tx)
                .await?;
        }
        for chunk in changes.upserted.chunks(CHUNK) {
            let mut sql = QueryBuilder::<Sqlite>::new(
                "INSERT OR REPLACE INTO files (repository_id, git_ref, path, blob, language) ",
            );
            sql.push_values(chunk, |mut row, file| {
                row.push_bind(repository_id)
                    .push_bind(git_ref)
                    .push_bind(&file.path)
                    .push_bind(&file.blob)
                    .push_bind(file.language);
            });
            sql.build().execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct PathKey {
    symbol_id: i64,
    repository_id: String,
    git_ref: String,
}

#[derive(Clone, Debug)]
struct PathNode {
    key: PathKey,
    path: String,
    line: i64,
    kind: String,
    name: String,
}

impl From<&Definition> for PathNode {
    fn from(definition: &Definition) -> Self {
        Self {
            key: PathKey {
                symbol_id: definition.id,
                repository_id: definition.repository_id.clone(),
                git_ref: definition.git_ref.clone(),
            },
            path: definition.path.clone(),
            line: definition.start_line,
            kind: definition.kind.clone(),
            name: definition.name.clone(),
        }
    }
}

#[derive(Clone, Debug)]
struct PathEdge {
    kind: String,
    confidence: String,
}

#[derive(Clone, Debug)]
struct PathVisit {
    node: PathNode,
    /// Forward: the prior node and its edge into this one. Backward: the next
    /// node and this node's edge into it.
    link: Option<(PathKey, PathEdge)>,
}

#[derive(Clone, Debug)]
struct PathNeighbor {
    near: PathKey,
    far: PathNode,
    edge: PathEdge,
}

#[derive(sqlx::FromRow)]
struct PathEdgeRow {
    near_symbol: i64,
    far_symbol: i64,
    far_repository: String,
    far_ref: String,
    path: String,
    line: i64,
    kind: String,
    name: String,
    edge_kind: String,
    confidence: String,
}

fn path_node_order(a: &PathNode, b: &PathNode) -> std::cmp::Ordering {
    a.key
        .repository_id
        .cmp(&b.key.repository_id)
        .then_with(|| a.path.cmp(&b.path))
        .then(a.line.cmp(&b.line))
}

fn path_hop(node: &PathNode, edge: Option<&PathEdge>) -> PathHop {
    PathHop {
        repository_id: node.key.repository_id.clone(),
        path: node.path.clone(),
        line: node.line,
        kind: node.kind.clone(),
        name: node.name.clone(),
        edge_kind: edge.map(|edge| edge.kind.clone()),
        confidence: edge.map(|edge| edge.confidence.clone()),
    }
}

fn join_path(
    meeting: &PathKey,
    forward: &HashMap<PathKey, PathVisit>,
    backward: &HashMap<PathKey, PathVisit>,
) -> Vec<PathHop> {
    let mut answer = Vec::new();
    let mut at = meeting;
    loop {
        let visit = &forward[at];
        answer.push(path_hop(
            &visit.node,
            visit.link.as_ref().map(|(_, edge)| edge),
        ));
        match &visit.link {
            Some((previous, _)) => at = previous,
            None => break,
        }
    }
    answer.reverse();

    let mut at = meeting;
    while let Some((next, edge)) = &backward[at].link {
        answer.push(path_hop(&backward[next].node, Some(edge)));
        at = next;
    }
    answer
}

/// One interaction as the query reads it, both ends flat.
#[derive(sqlx::FromRow)]
struct InteractionRow {
    kind: String,
    from_repository: String,
    from_path: String,
    from_line: i64,
    from_symbol: String,
    to_repository: String,
    to_path: String,
    to_line: i64,
    to_symbol: String,
    confidence: String,
}

/// Which end of an edge a listing names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum End {
    #[default]
    From,
    To,
}

/// One list of a symbol's context: which end is listed, over which kinds,
/// and whether the walk goes two edges out to find the tests.
#[derive(Clone, Copy, Debug, Default)]
struct RelatedQuery {
    listed: End,
    kinds: &'static [&'static str],
    two_hops: bool,
    tests_only: bool,
}

/// `<table>kind IN (…)`, over the kinds given; `table` is the alias the
/// edges are read under, with its dot, or nothing.
fn push_kinds(sql: &mut QueryBuilder<Sqlite>, table: &str, kinds: &[&str]) {
    sql.push(table).push("kind IN (");
    let mut values = sql.separated(", ");
    for kind in kinds {
        values.push_bind(kind.to_string());
    }
    sql.push(")");
}

/// Insert `edges` from one ref to another, ten values a row so a chunk stays
/// well under SQLite's bind limit.
async fn insert_edges(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    from_repository: &str,
    from_ref: &str,
    to_repository: &str,
    to_ref: &str,
    edges: &[Edge],
) -> Result<()> {
    for chunk in edges.chunks(CHUNK / 14) {
        let mut sql = QueryBuilder::<Sqlite>::new(
            "INSERT INTO edges (from_repository, git_ref, from_blob, kind, from_symbol, from_line,
                                to_repository, to_ref, to_blob, to_symbol, to_line, name,
                                confidence, candidates) ",
        );
        sql.push_values(chunk, |mut row, edge| {
            row.push_bind(from_repository)
                .push_bind(from_ref)
                .push_bind(&edge.from_blob)
                .push_bind(&edge.kind)
                .push_bind(edge.from_symbol)
                .push_bind(edge.from_line)
                .push_bind(to_repository)
                .push_bind(to_ref)
                .push_bind(&edge.to_blob)
                .push_bind(edge.to_symbol)
                .push_bind(edge.to_line)
                .push_bind(&edge.name)
                .push_bind(edge.confidence)
                .push_bind(edge.candidates);
        });
        sql.build().execute(&mut **tx).await?;
    }
    Ok(())
}

/// `sql`, whose text ends where a condition starts, narrowed to a list of
/// blobs.
fn in_blobs(mut sql: QueryBuilder<Sqlite>, blobs: &[String]) -> QueryBuilder<Sqlite> {
    sql.push(" blob IN (");
    let mut values = sql.separated(", ");
    for blob in blobs {
        values.push_bind(blob);
    }
    sql.push(")");
    sql
}

/// Drop the blobs no file holds, with their symbols and their FTS rows: two
/// statements, whatever the count. The FTS rows go by rowid, which is the
/// symbol id, and the blobs take their symbols with them.
async fn prune_orphan_blobs(tx: &mut sqlx::Transaction<'_, Sqlite>) -> Result<()> {
    sqlx::query(
        "DELETE FROM symbols_fts WHERE rowid IN
           (SELECT id FROM symbols WHERE blob NOT IN (SELECT blob FROM files))",
    )
    .execute(&mut **tx)
    .await?;
    sqlx::query("DELETE FROM blobs WHERE blob NOT IN (SELECT blob FROM files)")
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// `PRAGMA user_version` of the file at `path`, or `None` for a file SQLite
/// cannot read as a database.
async fn version_of(path: &Path) -> Option<i64> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(false)
                .read_only(true),
        )
        .await
        .ok()?;
    let version = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&pool)
        .await
        .ok();
    pool.close().await;
    version
}

/// Delete the store file and the WAL files beside it.
fn remove(path: &Path) -> Result<()> {
    let name = path.display().to_string();
    for file in [name.clone(), format!("{name}-wal"), format!("{name}-shm")] {
        match std::fs::remove_file(&file) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("removing {file}")),
        }
    }
    Ok(())
}

/// The FTS text of a symbol: its name and its qualified name, each whole and
/// split into the parts an agent might type.
fn terms_of(symbol: &Symbol) -> String {
    let mut terms = identifier_terms(&symbol.name);
    for term in identifier_terms(&symbol.qualified_name) {
        if !terms.contains(&term) {
            terms.push(term);
        }
    }
    terms.join(" ")
}

/// The lowercase words of an identifier or a path: each word whole, then
/// its camelCase and snake_case parts.
pub fn identifier_terms(text: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    let mut push = |term: String| {
        if !term.is_empty() && !terms.contains(&term) {
            terms.push(term);
        }
    };
    for word in text.split(|c: char| !c.is_alphanumeric() && c != '_') {
        if word.is_empty() {
            continue;
        }
        push(word.to_lowercase());
        for part in word.split('_') {
            for piece in camel_parts(part) {
                push(piece.to_lowercase());
            }
        }
    }
    terms
}

/// `HTTPServerName` is `HTTP`, `Server`, `Name`; `addWorktree` is `add`,
/// `Worktree`.
fn camel_parts(word: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = word.char_indices().collect();
    let mut parts = Vec::new();
    let mut start = 0;
    for at in 1..chars.len() {
        let (index, current) = chars[at];
        let previous = chars[at - 1].1;
        let next = chars.get(at + 1).map(|(_, c)| *c);
        let boundary = (current.is_uppercase()
            && (previous.is_lowercase() || previous.is_numeric()))
            || (current.is_uppercase()
                && previous.is_uppercase()
                && next.is_some_and(|next| next.is_lowercase()));
        if boundary {
            parts.push(&word[start..index]);
            start = index;
        }
    }
    parts.push(&word[start..]);
    parts.into_iter().filter(|part| !part.is_empty()).collect()
}

/// The FTS5 query for what an agent typed: every term as a prefix, all of
/// them required. `None` where the text holds no term at all.
fn fts_match(q: &str) -> Option<String> {
    let terms = identifier_terms(q);
    if terms.is_empty() {
        return None;
    }
    Some(
        terms
            .iter()
            .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_identifier_is_split_on_camel_case_snake_case_and_path_segments() {
        assert_eq!(
            identifier_terms("add_worktree"),
            ["add_worktree", "add", "worktree"]
        );
        assert_eq!(
            identifier_terms("GitManager"),
            ["gitmanager", "git", "manager"]
        );
        assert_eq!(
            identifier_terms("HTTPServerName"),
            ["httpservername", "http", "server", "name"]
        );
        assert_eq!(
            identifier_terms("GitManager::add_worktree"),
            [
                "gitmanager",
                "git",
                "manager",
                "add_worktree",
                "add",
                "worktree"
            ]
        );
        assert_eq!(identifier_terms("Title > Sub"), ["title", "sub"]);
        assert!(identifier_terms("  ::  ").is_empty());
    }

    #[test]
    fn a_query_becomes_prefix_terms_that_all_have_to_match() {
        assert_eq!(
            fts_match("add_worktree").as_deref(),
            Some("\"add_worktree\"* \"add\"* \"worktree\"*")
        );
        assert_eq!(
            fts_match("GitMan").as_deref(),
            Some("\"gitman\"* \"git\"* \"man\"*")
        );
        assert_eq!(fts_match("").as_deref(), None);
    }

    /// A walk of the callers three deep over a hundred thousand edges is
    /// answered in well under a tenth of a second: every level reads the
    /// `edges` indexes and nothing else.
    #[tokio::test]
    async fn impact_three_deep_over_a_hundred_thousand_edges_answers_under_a_tenth_of_a_second() {
        /// Two thousand files of ten definitions each, five callers apiece:
        /// twenty thousand definitions joined by a hundred thousand edges.
        const BLOBS: i64 = 2_000;
        const PER_BLOB: i64 = 10;
        const CALLERS: i64 = 5;
        let total = BLOBS * PER_BLOB;

        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.db"))
            .await
            .unwrap();
        let blob = |at: i64| format!("blob{at:04}");
        let parsed: Vec<ParsedBlob> = (0..BLOBS)
            .map(|at| ParsedBlob {
                blob: blob(at),
                language: "rust",
                symbols: (0..PER_BLOB)
                    .map(|n| Symbol {
                        kind: "function".into(),
                        name: format!("f{at}_{n}"),
                        qualified_name: format!("f{at}_{n}"),
                        start_line: n as u32 + 1,
                        end_line: n as u32 + 1,
                        signature: format!("fn f{at}_{n}()"),
                        doc: None,
                        is_test: false,
                    })
                    .collect(),
                references: Vec::new(),
                imports: Vec::new(),
                interfaces: Vec::new(),
            })
            .collect();
        for chunk in parsed.chunks(200) {
            store.commit_blobs(chunk).await.unwrap();
        }
        store
            .commit_files(
                "repo",
                "main",
                "commit",
                &FileChanges {
                    replace_all: true,
                    removed: Vec::new(),
                    upserted: (0..BLOBS)
                        .map(|at| FileRow {
                            path: format!("src/f{at:04}.rs"),
                            blob: blob(at),
                            language: "rust",
                        })
                        .collect(),
                },
            )
            .await
            .unwrap();

        // Spread so no definition is a hub: every one has the same few
        // callers, which is what an ordinary symbol looks like.
        let edges: Vec<Edge> = (1..=total)
            .flat_map(|to| {
                (0..CALLERS).filter_map(move |k| {
                    let from = (to * 7 + k * 13) % total + 1;
                    (from != to).then(|| Edge {
                        kind: "calls".into(),
                        from_blob: blob((from - 1) / PER_BLOB),
                        from_symbol: Some(from),
                        from_line: 1,
                        to_blob: blob((to - 1) / PER_BLOB),
                        to_symbol: Some(to),
                        to_line: 1,
                        name: None,
                        confidence: "exact",
                        candidates: 1,
                    })
                })
            })
            .collect();
        assert!(edges.len() > 99_000, "{} edges", edges.len());
        store
            .commit_edges("repo", "main", &[], &edges)
            .await
            .unwrap();

        let started = std::time::Instant::now();
        let (callers, stopped) = store.impact(total / 2, "repo", "main", 3).await.unwrap();
        let took = started.elapsed();
        assert!(stopped.is_empty(), "{stopped:?}");
        assert!(callers.len() > 20, "{} callers", callers.len());
        assert!(callers.iter().all(|caller| caller.depth <= 3));
        assert!(
            took < Duration::from_millis(100),
            "a three-deep walk over {} edges took {took:?}",
            edges.len()
        );
    }

    #[tokio::test]
    async fn a_path_is_found_between_two_definitions_and_none_past_the_depth() {
        let dir = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(dir.path().join("knowledge.db"))
            .await
            .unwrap();
        let parsed: Vec<ParsedBlob> = ["a", "b", "c"]
            .into_iter()
            .map(|name| ParsedBlob {
                blob: format!("blob-{name}"),
                language: "rust",
                symbols: vec![Symbol {
                    kind: "function".into(),
                    name: name.into(),
                    qualified_name: name.into(),
                    start_line: 1,
                    end_line: 1,
                    signature: format!("fn {name}()"),
                    doc: None,
                    is_test: false,
                }],
                references: Vec::new(),
                imports: Vec::new(),
                interfaces: Vec::new(),
            })
            .collect();
        store.commit_blobs(&parsed).await.unwrap();
        store
            .commit_files(
                "repo",
                "main",
                "commit",
                &FileChanges {
                    replace_all: true,
                    removed: Vec::new(),
                    upserted: ["a", "b", "c"]
                        .into_iter()
                        .map(|name| FileRow {
                            path: format!("src/{name}.rs"),
                            blob: format!("blob-{name}"),
                            language: "rust",
                        })
                        .collect(),
                },
            )
            .await
            .unwrap();
        let scope = [("repo".into(), "main".into())];
        let a = store.definitions("a", &scope).await.unwrap().remove(0);
        let b = store.definitions("b", &scope).await.unwrap().remove(0);
        let c = store.definitions("c", &scope).await.unwrap().remove(0);
        let edge = |from: &Definition, to: &Definition| Edge {
            kind: "calls".into(),
            from_blob: from.blob.clone(),
            from_symbol: Some(from.id),
            from_line: from.start_line,
            to_blob: to.blob.clone(),
            to_symbol: Some(to.id),
            to_line: to.start_line,
            name: None,
            confidence: "exact",
            candidates: 1,
        };
        store
            .commit_edges(
                "repo",
                "main",
                &[],
                &[edge(&a, &b), edge(&b, &c), edge(&a, &c)],
            )
            .await
            .unwrap();

        let path = store
            .path(std::slice::from_ref(&a), std::slice::from_ref(&c), 1)
            .await
            .unwrap();
        assert_eq!(
            path.iter().map(|hop| hop.name.as_str()).collect::<Vec<_>>(),
            ["a", "c"]
        );
        assert_eq!(path[0].edge_kind, None);
        assert_eq!(path[1].edge_kind.as_deref(), Some("calls"));

        let path = store
            .path(std::slice::from_ref(&a), std::slice::from_ref(&c), 6)
            .await
            .unwrap();
        assert_eq!(
            path.iter().map(|hop| hop.name.as_str()).collect::<Vec<_>>(),
            ["a", "c"]
        );

        assert!(store.path(&[a], &[b], 0).await.unwrap().is_empty());
    }

    /// The schema is applied to a fresh file, and a file at another version
    /// is thrown away rather than migrated.
    #[tokio::test]
    async fn a_store_at_another_schema_version_is_rebuilt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("knowledge.db");
        let store = KnowledgeStore::open(&path).await.unwrap();
        store
            .set_state("repo", State::Failed, Some("boom"))
            .await
            .unwrap();
        drop(store);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(SqliteConnectOptions::new().filename(&path))
            .await
            .unwrap();
        sqlx::raw_sql("PRAGMA user_version = 999")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        let store = KnowledgeStore::open(&path).await.unwrap();
        let status = store.status("repo").await.unwrap();
        assert_eq!(status.state, State::Idle, "the old rows are gone");
        assert_eq!(status.error, None);
        assert_eq!(version_of(&path).await, Some(SCHEMA_VERSION));
    }
}
