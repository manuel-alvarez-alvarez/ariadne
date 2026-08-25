//! SQLite persistence layer.
//!
//! One [`Store`] owns two pools: a single-connection write pool (SQLite has a
//! single writer) and a small read pool. All status changes go through
//! [`Store::transition_task`], which validates against the core state machine
//! and records the audit row in the same transaction.

mod agents;
mod change;
pub mod defaults;
mod entities;
mod events;
mod goals;
mod messages;
mod profiles;
mod prompts;
mod repositories;
mod reviews;
mod sessions;
mod tasks;

pub use change::Change;
pub use entities::*;
pub use events::{EventFilter, NewAgentEvent};
pub use goals::NewGoal;
pub use messages::NewMessage;
pub use profiles::{NewProfile, ProfileUpdate};
pub use prompts::parse_prompt_kind;
pub use repositories::{NewRepository, RepositoryUpdate};
pub use reviews::NewReview;
pub use sessions::{NewSession, SessionFilter};
pub use tasks::{NewTask, TaskFilter, TaskUpdate};

use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Pool, Sqlite};
use tokio::sync::mpsc;

use ariadne_core::TransitionError;

/// The one migration this release ships. There is exactly one: the schema is
/// squashed, and prompts are overrides, so nothing rewrites a default in SQL
/// any more (see `migrations/0001_init.sql`).
static MIGRATIONS: Migrator = sqlx::migrate!("./migrations");

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{entity} not found: {id}")]
    NotFound { entity: &'static str, id: String },
    #[error("conflict: {0}")]
    Conflict(String),
    #[error(transparent)]
    Transition(#[from] TransitionError),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Current time in the canonical stored format (ISO-8601 UTC, second precision
/// is not enough for ordering — we rely on ULID ids for that).
pub(crate) fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub(crate) fn not_found(entity: &'static str, id: &str) -> StoreError {
    StoreError::NotFound {
        entity,
        id: id.to_string(),
    }
}

/// Whether the database at `path` was written before the 29 migrations were
/// squashed into one, which is the only thing that stops this release opening
/// a database it otherwise understands. `Some` is the sentence to show; `None`
/// is a database this release can open, a file that is not one of ours and a
/// path with nothing on it — a report never calls anything old on a guess.
///
/// For `ariadne doctor`, which is asked why the daemon will not start and is
/// the only thing still running to answer it.
pub async fn pre_squash_database(path: impl AsRef<Path>) -> Option<String> {
    let path = path.as_ref();
    if !path.is_file() {
        return None;
    }
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
    let old = applied_elsewhere(&pool).await.ok()?;
    pool.close().await;
    old.then(|| pre_squash_message(path))
}

/// Whether `_sqlx_migrations` records a migration this release does not ship —
/// a later version of the chain that was squashed away, or a version 1 whose
/// checksum is the old `0001_init.sql`. Either way sqlx refuses to run, and
/// the database is one from before the squash.
///
/// A database with no `_sqlx_migrations` table at all is a fresh one.
async fn applied_elsewhere(pool: &Pool<Sqlite>) -> Result<bool> {
    let recorded: Option<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
    )
    .fetch_optional(pool)
    .await?;
    if recorded.is_none() {
        return Ok(false);
    }
    let applied: Vec<(i64, Vec<u8>)> =
        sqlx::query_as("SELECT version, checksum FROM _sqlx_migrations")
            .fetch_all(pool)
            .await?;
    Ok(applied.iter().any(|(version, checksum)| {
        !MIGRATIONS
            .iter()
            .any(|m| m.version == *version && *m.checksum == checksum[..])
    }))
}

/// What a user holding one is told. Ariadne is pre-1.0: a database is
/// recreated rather than migrated, so the fix is one file to delete — named in
/// full, since it is wherever `db_path` puts it.
fn pre_squash_message(path: &Path) -> String {
    let path = path.display();
    format!(
        "{path} predates the squashed schema: it was written by a release whose \
         migrations this one no longer ships, and there is no upgrade from it. \
         Delete {path} (and its -wal and -shm files, if any) and start again — \
         Ariadne is pre-1.0, so a database is recreated rather than migrated."
    )
}

#[derive(Clone)]
pub struct Store {
    /// Single-connection pool: every write serializes here.
    write: Pool<Sqlite>,
    /// Read-only pool for queries.
    read: Pool<Sqlite>,
    /// Change sink installed by [`Store::watch_changes`]. Shared by every
    /// clone, so a write through any handle is announced.
    changes: Arc<OnceLock<mpsc::UnboundedSender<Change>>>,
}

impl Store {
    /// Open (creating if needed) the database at `path` and run migrations.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path.as_ref())
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true);

        let write = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await?;

        // Before sqlx gets to refuse it with a checksum, in a sentence naming
        // what to do about it.
        if applied_elsewhere(&write).await? {
            return Err(StoreError::Invalid(pre_squash_message(path.as_ref())));
        }
        MIGRATIONS
            .run(&write)
            .await
            .map_err(|e| StoreError::Invalid(format!("migration failed: {e}")))?;

        // Opened after the migrations, not before: a connection that read the
        // schema first keeps the old column set, and a migration that adds a
        // column would then have every `SELECT *` on that table read short by
        // one until the process restarts.
        //
        // Safe to open read-only: the write pool above connected with
        // `create_if_missing`, so the file already exists.
        let read = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options.read_only(true))
            .await?;

        let store = Self {
            write,
            read,
            changes: Arc::default(),
        };
        store.seed_builtin_profiles().await?;
        store.seed_agent_configs().await?;
        Ok(store)
    }

    /// Subscribe to committed writes (see [`Change`]).
    ///
    /// Returns `None` when a watcher is already installed: there is exactly
    /// one consumer, the daemon's event bus, installed at startup. Changes
    /// written before it is installed — or with no watcher at all — are
    /// dropped, since clients bootstrap their state over REST anyway.
    pub fn watch_changes(&self) -> Option<mpsc::UnboundedReceiver<Change>> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.changes.set(tx).ok()?;
        Some(rx)
    }

    /// Announce a committed write. Non-blocking; a no-op without a watcher.
    pub(crate) fn publish(&self, change: Change) {
        if let Some(tx) = self.changes.get() {
            let _ = tx.send(change);
        }
    }

    pub(crate) fn w(&self) -> &Pool<Sqlite> {
        &self.write
    }

    pub(crate) fn r(&self) -> &Pool<Sqlite> {
        &self.read
    }
}
