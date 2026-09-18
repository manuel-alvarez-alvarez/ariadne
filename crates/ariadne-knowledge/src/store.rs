//! The SQLite file the index lives in, and the questions asked of it.
//!
//! `knowledge.db` sits beside the daemon's database and is disposable: the
//! schema in `schema.sql` is stamped into `PRAGMA user_version`, and a file
//! at any other version is deleted and rebuilt rather than migrated. Symbols
//! are keyed by blob, so a file whose content has not changed is parsed once
//! and shared by every ref and every repository that holds it.

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Pool, QueryBuilder, Sqlite};

use crate::parser::Symbol;

/// The schema this build writes. Bump it with every change to `schema.sql`:
/// a store at another version is thrown away and indexed again.
pub const SCHEMA_VERSION: i64 = 2;

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

/// One blob's definitions, ready to store.
#[derive(Clone, Debug)]
pub(crate) struct ParsedBlob {
    pub blob: String,
    pub language: &'static str,
    pub symbols: Vec<Symbol>,
}

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
