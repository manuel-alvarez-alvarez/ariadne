//! SQLite persistence layer.
//!
//! One [`Store`] owns two pools: a single-connection write pool (SQLite has a
//! single writer) and a small read pool. All status changes go through
//! [`Store::transition_task`], which validates against the core state machine
//! and records the audit row in the same transaction.

mod acp_catalogs;
mod acp_registry;
mod agents;
mod ai_permissions;
mod change;
pub mod defaults;
mod entities;
mod events;
mod forge;
mod goals;
mod messages;
mod models;
mod permissions;
mod pull_request_comments;
mod pull_requests;
mod query;
mod repositories;
mod sessions;
mod skills;
mod stats;
mod task_agents;
mod tasks;
mod usage;
mod webhooks;
mod workflows;

pub use ai_permissions::AiPermissionSettingsUpdate;
pub use change::Change;
pub use entities::*;
pub use events::{EventFilter, EventOrder, NewAgentEvent};
pub use forge::{ForgeWrite, SetForgeIntegration};
pub use goals::NewGoal;
pub use messages::{MessageFilter, NewMessage};
pub use permissions::NewLearnedPermission;
pub use pull_request_comments::NewPullRequestComment;
pub use pull_requests::{NewPullRequest, PullRequestFilter, PullRequestTold};
pub use repositories::{NewRepository, RepositoryUpdate};
pub use sessions::{NewSession, SessionFilter};
pub use skills::NewSkill;
pub use stats::{
    AttentionStats, Bucket, LeadTime, ModelSpend, ModelStat, ModelStats, NewStatFact,
    PerFinishedTask, PersonWait, SpendBucket, SpendStats, SpendTotals, StatsFilter, StatusTime,
    TimeStats, WorkStats,
};
pub use task_agents::NewTaskAgent;
pub use tasks::{NewTask, TaskFilter, TaskUpdate, unstaffed_columns};
pub use usage::{AgentUsage, SeatUsage};
pub use workflows::NewWorkflow;

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Pool, Sqlite};
use tokio::sync::mpsc;

use ariadne_core::TransitionError;

/// The migrations this release ships.
static MIGRATIONS: Migrator = sqlx::migrate!("./migrations");

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("workflow {0} is still in use")]
    WorkflowInUse(String),
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

/// Whether the database at `path` predates the squash of the migrations into
/// one, which is the only thing that stops this release opening a
/// database it otherwise understands. `Some` is the sentence to show; a file
/// that is not one of ours and a path with nothing on it are both `None` — a
/// report never calls anything old on a guess.
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
/// a later version of the squashed-away chain, or a version 1 whose checksum
/// is the old `0001_init.sql`. Either way sqlx refuses to run over it. A
/// database with no `_sqlx_migrations` table at all is a fresh one.
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

/// Where a backup of the database at `path` goes before a migration that
/// may drop something, named by the schema version it is a copy of: a
/// repeated attempt at the same upgrade then finds its own backup already
/// there, rather than overwriting it or failing beside it.
fn backup_path(path: &Path, version: i64) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".backup-schema-{version}"));
    PathBuf::from(name)
}

/// What a user holding one is told: the fix is one file to delete, named in
/// full since it is wherever `db_path` puts it.
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
    /// Held from the moment an agent event takes its id to the moment it is
    /// published, so the ids go out in the order they were taken. Shared by
    /// every clone, and lent to whoever else gives events ids from the same
    /// generator — the live events of the console — so they keep that order
    /// too.
    event_order: Arc<tokio::sync::Mutex<()>>,
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
            .foreign_keys(true)
            // No checkpoint on the commit path. SQLite's default runs one
            // every 1000 pages of WAL, on the connection that committed —
            // and a checkpoint cannot reset the WAL while any reader is on
            // an older snapshot. Something always is: the scheduler reads
            // every five seconds, and a console or the desktop app reads
            // constantly. So the WAL sits above the threshold, every commit
            // pays for a checkpoint that can never finish, and the one write
            // connection is the whole daemon's. `checkpoint` below does it
            // off the commit path instead.
            .pragma("wal_autocheckpoint", "0")
            // 64 MB rather than SQLite's 2 MB, against a database that is
            // hundreds of megabytes: the pages a read walks are mostly ones
            // another read just walked.
            .pragma("cache_size", format!("-{}", 64 * 1024));

        let write = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await?;

        // Before sqlx gets to refuse it with a checksum, in a sentence naming
        // what to do about it.
        if applied_elsewhere(&write).await? {
            return Err(StoreError::Invalid(pre_squash_message(path.as_ref())));
        }
        // Rebuilds preserve child rows only with foreign keys off outside
        // the migration transaction. Restore enforcement before any writer runs.
        let mut connection = write.acquire().await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *connection)
            .await?;
        // A database already on some schema, with a migration still pending,
        // loses whatever that migration drops — 0023 drops whole tables and
        // columns. Back it up beside itself first, so an upgrade a person
        // did not mean to run is a file away from undone. A database just
        // created above, or already on every migration this release ships,
        // has nothing to lose and gets no backup.
        let on_a_schema: Option<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations'",
        )
        .fetch_optional(&mut *connection)
        .await?;
        if on_a_schema.is_some() {
            let applied: Vec<i64> = sqlx::query_scalar("SELECT version FROM _sqlx_migrations")
                .fetch_all(&mut *connection)
                .await?;
            if MIGRATIONS.iter().any(|m| !applied.contains(&m.version)) {
                let backup = backup_path(path.as_ref(), applied.iter().max().copied().unwrap_or(0));
                // `VACUUM INTO` creates its destination the moment it starts
                // and fills it as it runs: a process killed mid-backup, on a
                // release before this one wrote straight to this name, left
                // it holding an empty file, which a plain existence check
                // takes for a finished backup and skips redoing. A backup
                // worth trusting is never empty.
                let done = std::fs::metadata(&backup)
                    .map(|m| m.len() > 0)
                    .unwrap_or(false);
                if !done {
                    // Written under a name of its own and renamed into place
                    // only once whole, so this release's own attempt never
                    // leaves a half-written file at the final name for a
                    // later open to mistake for one — `rename` replaces
                    // whatever stale, empty file sat there already.
                    let mut tmp = backup.clone().into_os_string();
                    tmp.push(".tmp");
                    let tmp = PathBuf::from(tmp);
                    let _ = std::fs::remove_file(&tmp);
                    let to = tmp.to_str().ok_or_else(|| {
                        StoreError::Invalid(format!("{} is not valid UTF-8", tmp.display()))
                    })?;
                    sqlx::query("VACUUM INTO ?")
                        .bind(to)
                        .execute(&mut *connection)
                        .await
                        .map_err(|e| {
                            StoreError::Invalid(format!(
                                "could not back the database up before migrating it: {e}"
                            ))
                        })?;
                    std::fs::rename(&tmp, &backup).map_err(|e| {
                        StoreError::Invalid(format!(
                            "could not finish the database backup at {}: {e}",
                            backup.display()
                        ))
                    })?;
                }
            }
        }
        let migrated = MIGRATIONS.run_direct(None, &mut *connection, false).await;
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *connection)
            .await?;
        migrated.map_err(|e| StoreError::Invalid(format!("migration failed: {e}")))?;
        let violations = sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *connection)
            .await?;
        if !violations.is_empty() {
            return Err(StoreError::Invalid(
                "migration left invalid references".into(),
            ));
        }
        drop(connection);

        // Opened after the migrations, not before: a connection that read the
        // schema first keeps the old column set, so every `SELECT *` on a
        // table a migration widened would read short by one until the process
        // restarts. Read-only is safe here — the write pool above connected
        // with `create_if_missing`, so the file exists.
        let read = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options.read_only(true))
            .await?;

        let store = Self {
            write,
            read,
            changes: Arc::default(),
            event_order: Arc::default(),
        };
        store.seed_builtin_skills().await?;
        store.seed_builtin_workflows().await?;
        Ok(store)
    }

    /// Subscribe to committed writes (see [`Change`]).
    ///
    /// `None` when a watcher is already installed: there is exactly one
    /// consumer, the daemon's event bus. Changes written before it is
    /// installed are dropped, since clients bootstrap over REST anyway.
    pub fn watch_changes(&self) -> Option<mpsc::UnboundedReceiver<Change>> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.changes.set(tx).ok()?;
        Some(rx)
    }

    /// Announce a committed write. Non-blocking; a no-op without a watcher.
    /// The lock an agent event holds from its id to its publication. A
    /// caller that gives an event an id from the same generator and sends it
    /// another way — the console's live events — holds it too, so that no
    /// event is published before one with a lower id.
    pub fn event_order(&self) -> &tokio::sync::Mutex<()> {
        &self.event_order
    }

    /// Fold the write-ahead log back into the database and start it again.
    ///
    /// The commit path no longer does this (`wal_autocheckpoint` is off), so
    /// this is what keeps the WAL from growing without end. `TRUNCATE` waits
    /// for the readers on older snapshots to finish and gives up after
    /// `busy_timeout` where they do not, which is a checkpoint missed and
    /// not a failure: the next one folds in what this one could not.
    ///
    /// Answers the error where SQLite refused, so a caller can say so once
    /// rather than every time.
    pub async fn checkpoint(&self) -> Result<()> {
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(self.w())
            .await?;
        Ok(())
    }

    /// Close the store: every query after this fails, on every clone. What a
    /// test needs to stand in for a store that cannot be read.
    pub async fn close(&self) {
        self.read.close().await;
        self.write.close().await;
    }

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
