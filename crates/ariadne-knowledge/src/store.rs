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

use crate::parser::{Import, Reference, Symbol};
use crate::resolve::{Candidate, Edge, Mention, Names};

/// The schema this build writes. Bump it with every change to `schema.sql`:
/// a store at another version is thrown away and indexed again.
pub const SCHEMA_VERSION: i64 = 4;

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
    pub tests: Vec<Related>,
}

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

/// How many callers a symbol may have and still be walked past. A symbol
/// over it is a utility everything touches, and its callers say nothing
/// about what one change reaches.
pub const MAX_FANOUT: i64 = 200;

/// How many entries each list of a symbol's context holds.
pub const CONTEXT_LIMIT: i64 = 20;

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
        // Edges are keyed by the ref they were resolved at, which no cascade
        // reaches: a blob another repository still holds keeps its own rows.
        sqlx::query("DELETE FROM edges WHERE from_repository = ?")
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
        sqlx::query("DELETE FROM edges WHERE from_repository = ? AND git_ref = ?")
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
    /// calls, what implements it, and the tests that reach it.
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
        let at = |join, filter, close| {
            self.related(
                join,
                filter,
                close,
                symbol_id,
                repository_id,
                git_ref,
                limit,
            )
        };
        Ok(SymbolContext {
            // Who calls it: the far end of a `calls` edge into it.
            callers: at(
                "s.id = e.from_symbol",
                "e.kind = 'calls' AND e.to_symbol = ",
                "",
            )
            .await?,
            // What it calls: the far end of a `calls` edge out of it.
            callees: at(
                "s.id = e.to_symbol",
                "e.kind = 'calls' AND e.from_symbol = ",
                "",
            )
            .await?,
            // What implements it, or what it is a base of.
            implementations: at(
                "s.id = e.from_symbol",
                "e.kind IN ('implements', 'extends') AND e.to_symbol = ",
                "",
            )
            .await?,
            // The tests at most two edges away: a test that calls it, and a
            // test that calls something that calls it.
            tests: at(
                "s.id = e.from_symbol AND s.is_test = 1",
                "e.kind = 'calls' AND e.to_symbol IN (SELECT ?1
                 UNION SELECT from_symbol FROM edges
                 WHERE ?2 AND kind = 'calls'
                   AND from_symbol IS NOT NULL AND to_symbol = ",
                ")",
            )
            .await?,
        })
    }

    /// One side of the edges of a symbol, as the answer names them: `join`
    /// says which end of the edge the symbol row is, `filter` is the
    /// condition up to where the symbol's own id is bound, and `close` is
    /// what follows it.
    #[allow(clippy::too_many_arguments)]
    async fn related(
        &self,
        join: &str,
        filter: &str,
        close: &str,
        symbol_id: i64,
        repository_id: &str,
        git_ref: &str,
        limit: i64,
    ) -> Result<Vec<Related>> {
        let mut sql = QueryBuilder::<Sqlite>::new(
            "SELECT DISTINCT f.repository_id, f.path, s.start_line AS line, s.name, e.confidence
             FROM edges e JOIN symbols s ON ",
        );
        sql.push(join)
            .push(" JOIN files f ON f.blob = s.blob WHERE e.from_repository = ")
            .push_bind(repository_id)
            .push(" AND e.git_ref = ")
            .push_bind(git_ref)
            .push(" AND ");
        // `?1` inside the filter is the same symbol, bound once more, and
        // `?2` is the ref the edges of a subquery are read at.
        for (at, part) in filter.split("?1").enumerate() {
            if at > 0 {
                sql.push_bind(symbol_id);
            }
            for (at, part) in part.split("?2").enumerate() {
                if at > 0 {
                    sql.push("from_repository = ")
                        .push_bind(repository_id)
                        .push(" AND git_ref = ")
                        .push_bind(git_ref);
                }
                sql.push(part);
            }
        }
        sql.push_bind(symbol_id)
            .push(close)
            .push(" AND f.repository_id = ")
            .push_bind(repository_id)
            .push(" AND f.git_ref = ")
            .push_bind(git_ref)
            .push(" AND s.id <> ")
            .push_bind(symbol_id)
            .push(" ORDER BY f.path, line LIMIT ")
            .push_bind(limit.max(1));
        Ok(sql
            .build_query_as::<Related>()
            .fetch_all(&self.read)
            .await?)
    }

    /// The callers of `symbol_id`, walked out to `depth`, with the symbols
    /// the walk did not go past.
    ///
    /// One level is two statements: how many callers each symbol of the
    /// level has, and then the callers of the ones under [`MAX_FANOUT`].
    /// Both read the `edges(kind, to_symbol, …)` index and nothing else, so
    /// the walk costs the callers it answers with and no more.
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
        let mut level = vec![symbol_id];
        for at in 1..=depth.max(1) {
            if level.is_empty() {
                break;
            }
            // How many callers each symbol of the level has, in one
            // statement: the index answers it without reading a row of the
            // table.
            let mut counts: HashMap<i64, i64> = HashMap::new();
            for chunk in level.chunks(CHUNK) {
                let mut sql = QueryBuilder::<Sqlite>::new(
                    "SELECT to_symbol, COUNT(*) FROM edges WHERE from_repository = ",
                );
                sql.push_bind(repository_id)
                    .push(" AND git_ref = ")
                    .push_bind(git_ref)
                    .push(" AND kind = 'calls' AND to_symbol IN (");
                let mut values = sql.separated(", ");
                for id in chunk {
                    values.push_bind(id);
                }
                sql.push(") GROUP BY to_symbol");
                let rows: Vec<(i64, i64)> = sql.build_query_as().fetch_all(&self.read).await?;
                counts.extend(rows);
            }
            let mut wanted: Vec<i64> = Vec::new();
            for id in &level {
                match counts.get(id).copied().unwrap_or(0) > MAX_FANOUT {
                    true => stopped.push(self.name_of(*id).await?),
                    false => wanted.push(*id),
                }
            }
            let mut next = Vec::new();
            for chunk in wanted.chunks(CHUNK) {
                let mut sql = QueryBuilder::<Sqlite>::new(
                    "SELECT DISTINCT s.id, f.repository_id, f.path, s.start_line, s.name,
                            e.confidence
                     FROM edges e
                     JOIN symbols s ON s.id = e.from_symbol
                     JOIN files f ON f.blob = s.blob
                     WHERE e.kind = 'calls' AND e.from_repository = ",
                );
                sql.push_bind(repository_id)
                    .push(" AND e.git_ref = ")
                    .push_bind(git_ref)
                    .push(" AND f.repository_id = ")
                    .push_bind(repository_id)
                    .push(" AND f.git_ref = ")
                    .push_bind(git_ref)
                    .push(" AND e.to_symbol IN (");
                let mut values = sql.separated(", ");
                for id in chunk {
                    values.push_bind(id);
                }
                sql.push(")");
                let rows: Vec<(i64, String, String, i64, String, String)> =
                    sql.build_query_as().fetch_all(&self.read).await?;
                for (id, repository_id, path, line, name, confidence) in rows {
                    if !seen.insert(id) {
                        continue;
                    }
                    next.push(id);
                    callers.push(ImpactCaller {
                        depth: at,
                        repository_id,
                        path,
                        line,
                        name,
                        confidence,
                    });
                }
            }
            level = next;
        }
        callers.sort_by(|a, b| {
            a.depth
                .cmp(&b.depth)
                .then_with(|| a.path.cmp(&b.path))
                .then(a.line.cmp(&b.line))
        });
        stopped.sort();
        stopped.dedup();
        Ok((callers, stopped))
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
                "SELECT s.name, s.id, s.blob, f.path, s.qualified_name
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
            let found: Vec<(String, i64, String, String, String)> =
                sql.build_query_as().fetch_all(&self.read).await?;
            for (name, id, blob, path, qualified_name) in found {
                candidates.entry(name).or_default().push(Candidate {
                    id,
                    blob,
                    path,
                    qualified_name,
                });
            }
        }
        Ok(candidates)
    }

    /// Replace the edges of `blobs` with `edges`, in one transaction: a
    /// blob's edges are all derived together or not at all.
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
        // Ten values a row, so a chunk stays well under SQLite's bind limit.
        for chunk in edges.chunks(CHUNK / 10) {
            let mut sql = QueryBuilder::<Sqlite>::new(
                "INSERT INTO edges (from_repository, git_ref, from_blob, kind, from_symbol,
                                    to_symbol, to_repository, from_line, confidence, candidates) ",
            );
            sql.push_values(chunk, |mut row, edge| {
                row.push_bind(repository_id)
                    .push_bind(git_ref)
                    .push_bind(&edge.from_blob)
                    .push_bind(&edge.kind)
                    .push_bind(edge.from_symbol)
                    .push_bind(edge.to_symbol)
                    .push_bind(repository_id)
                    .push_bind(edge.from_line)
                    .push_bind(edge.confidence)
                    .push_bind(edge.candidates);
            });
            sql.build().execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
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
        sqlx::query(
            "INSERT OR IGNORE INTO repositories (id, state, error, updated_at) VALUES (?, 'idle', NULL, ?)",
        )
        .bind(repository_id)
        .bind(&stamp)
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
                        to_symbol: to,
                        from_line: 1,
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
