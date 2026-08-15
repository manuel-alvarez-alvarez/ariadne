//! SQLite persistence layer.
//!
//! One [`Store`] owns two pools: a single-connection write pool (SQLite has a
//! single writer) and a small read pool. All status changes go through
//! [`Store::transition_task`], which validates against the core state machine
//! and records the audit row in the same transaction.

mod entities;
mod events;
mod goals;
mod messages;
mod profiles;
mod reviews;
mod sessions;
mod tasks;

pub use entities::*;
pub use events::{EventFilter, NewAgentEvent};
pub use goals::NewGoal;
pub use messages::NewMessage;
pub use profiles::{NewProfile, ProfileUpdate};
pub use reviews::NewReview;
pub use sessions::{NewSession, SessionFilter};
pub use tasks::{NewTask, TaskFilter, TaskUpdate};

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Pool, Sqlite};

use ariadne_core::TransitionError;

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

#[derive(Clone)]
pub struct Store {
    /// Single-connection pool: every write serializes here.
    write: Pool<Sqlite>,
    /// Read-only pool for queries.
    read: Pool<Sqlite>,
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
        let read = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options.read_only(false))
            .await?;

        sqlx::migrate!("./migrations")
            .run(&write)
            .await
            .map_err(|e| StoreError::Invalid(format!("migration failed: {e}")))?;

        Ok(Self { write, read })
    }

    /// In-memory store for tests.
    pub async fn open_in_memory() -> Result<Self> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .map_err(StoreError::Db)?
            .foreign_keys(true);
        // A single shared connection: :memory: databases are per-connection.
        let write = SqlitePoolOptions::new()
            .max_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations")
            .run(&write)
            .await
            .map_err(|e| StoreError::Invalid(format!("migration failed: {e}")))?;
        Ok(Self {
            read: write.clone(),
            write,
        })
    }

    pub(crate) fn w(&self) -> &Pool<Sqlite> {
        &self.write
    }

    pub(crate) fn r(&self) -> &Pool<Sqlite> {
        &self.read
    }
}
