//! Telemetry persistence: a second SQLite file, `telemetry.db`, holding
//! every log record and metric sample the daemon's OpenTelemetry pipeline
//! produces. Kept apart from `ariadne.db` ([`crate::Store`]) so that the
//! volume a running daemon logs and measures never grows the file every
//! other repository query reads.
//!
//! [`TelemetryStore`] owns the same shape of pools `Store` does — one
//! single-connection write pool, since SQLite allows one writer, and a
//! small read pool — under the same WAL pragmas, and folds its own
//! write-ahead log back in the same way (`checkpoint`).

mod labels;
mod logs;
mod metrics;
mod retention;

pub use labels::{LabelMatcher, MatchOp, NAME_LABEL};
pub use logs::{LogFilter, LogRecord, NewLogRecord, severity_number_for_level};
pub use metrics::{
    MetricKind, MetricMetadata, MetricPoint, RollupPoint, Sample, SeriesMeta, TimeRange,
};
pub use retention::RetentionReport;

use std::path::Path;
use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Pool, Sqlite};

use crate::Result;

/// The one representation SQLite compares: UTC RFC 3339, millisecond
/// precision. Every stored timestamp and every query boundary passes here,
/// so equal instants sort and compare equally regardless of input offset or
/// fractional-precision spelling.
pub fn normalize_timestamp(value: &str) -> Result<String> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|timestamp| {
            timestamp
                .with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Millis, true)
        })
        .map_err(|error| {
            crate::StoreError::Invalid(format!("invalid RFC 3339 timestamp {value:?}: {error}"))
        })
}

/// The migrations this release ships for the telemetry database, kept apart
/// from the main database's own migrator since the two files are
/// independent schemas.
static TELEMETRY_MIGRATIONS: Migrator = sqlx::migrate!("./telemetry-migrations");

#[derive(Clone, Debug)]
pub struct TelemetryStore {
    /// Single-connection pool: every write serializes here, as in [`crate::Store`].
    write: Pool<Sqlite>,
    /// Read-only pool for queries.
    read: Pool<Sqlite>,
}

impl TelemetryStore {
    /// Open (creating if needed) the telemetry database at `path` and run
    /// its migrations.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path.as_ref())
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true)
            // Off for the same reason `Store::open` turns it off: nothing on
            // the commit path should pay for a checkpoint it cannot finish
            // while a reader holds an older snapshot. `checkpoint` below
            // does it off the commit path instead.
            .pragma("wal_autocheckpoint", "0")
            .pragma("cache_size", format!("-{}", 64 * 1024));

        let write = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await?;
        TELEMETRY_MIGRATIONS
            .run(&write)
            .await
            .map_err(|e| crate::StoreError::Invalid(format!("telemetry migration failed: {e}")))?;

        let read = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options.read_only(true))
            .await?;

        Ok(Self { write, read })
    }

    /// Close the store: every query after this fails, on every clone.
    pub async fn close(&self) {
        self.read.close().await;
        self.write.close().await;
    }

    /// Fold the write-ahead log back into the database and start it again.
    /// See [`crate::Store::checkpoint`] for why this runs off the commit path.
    pub async fn checkpoint(&self) -> Result<()> {
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.write)
            .await?;
        Ok(())
    }

    pub(crate) fn w(&self) -> &Pool<Sqlite> {
        &self.write
    }

    pub(crate) fn r(&self) -> &Pool<Sqlite> {
        &self.read
    }
}
