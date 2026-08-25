//! The shapes every repository here shares: reading one row by a column, and
//! narrowing a `SELECT *` by whichever filters were set.

use sqlx::sqlite::SqliteRow;
use sqlx::{FromRow, Sqlite, Transaction};

use crate::{Result, Store, not_found};

/// A row type these helpers can read.
pub(crate) trait Row: for<'r> FromRow<'r, SqliteRow> + Send + Unpin {}
impl<T: for<'r> FromRow<'r, SqliteRow> + Send + Unpin> Row for T {}

/// `SELECT * FROM <table> WHERE <column> = ?`. The fragments are literals at
/// every call site; the value is bound.
fn one_of(table: &str, column: &str) -> sqlx::AssertSqlSafe<String> {
    sqlx::AssertSqlSafe(format!("SELECT * FROM {table} WHERE {column} = ?"))
}

impl Store {
    /// The one row of `table` whose `column` holds `value`, or a `NotFound`
    /// naming the entity the caller asked for rather than the table.
    pub(crate) async fn fetch_by<T: Row>(
        &self,
        entity: &'static str,
        table: &str,
        column: &str,
        value: &str,
    ) -> Result<T> {
        sqlx::query_as::<_, T>(one_of(table, column))
            .bind(value)
            .fetch_optional(self.r())
            .await?
            .ok_or_else(|| not_found(entity, value))
    }

    /// The same read through an open write transaction, for callers that copy
    /// values off the row into rows they are writing or check a status they
    /// are about to change: the read pool could hand back a version older than
    /// the one the transaction is holding.
    pub(crate) async fn fetch_by_in_tx<T: Row>(
        tx: &mut Transaction<'_, Sqlite>,
        entity: &'static str,
        table: &str,
        id: &str,
    ) -> Result<T> {
        sqlx::query_as::<_, T>(one_of(table, "id"))
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(|| not_found(entity, id))
    }
}

/// A `SELECT *` narrowed by the filters that were actually set.
///
/// Only fixed clause fragments are appended — every value goes in as a
/// binding — which is what makes the assembled string safe to assert.
pub(crate) struct Filtered {
    sql: String,
    binds: Vec<String>,
}

impl Filtered {
    pub(crate) fn new(table: &str) -> Self {
        Self {
            sql: format!("SELECT * FROM {table} WHERE 1=1"),
            binds: Vec::new(),
        }
    }

    /// `clause` when `value` is set, with the value bound in its place.
    pub(crate) fn maybe(mut self, clause: &str, value: Option<impl Into<String>>) -> Self {
        if let Some(value) = value {
            self.sql.push_str(clause);
            self.binds.push(value.into());
        }
        self
    }

    /// `clause` when `on`; it carries no value of its own.
    pub(crate) fn flag(mut self, clause: &str, on: bool) -> Self {
        if on {
            self.sql.push_str(clause);
        }
        self
    }

    /// Everything that matched, in `tail` order (`ORDER BY`, and a `LIMIT`
    /// where one belongs), with `extra` bound after the filters.
    pub(crate) async fn fetch<T: Row>(
        mut self,
        store: &Store,
        tail: &str,
        extra: &[i64],
    ) -> Result<Vec<T>> {
        self.sql.push_str(tail);
        let mut q = sqlx::query_as::<_, T>(sqlx::AssertSqlSafe(self.sql));
        for bind in self.binds {
            q = q.bind(bind);
        }
        for bind in extra {
            q = q.bind(*bind);
        }
        Ok(q.fetch_all(store.r()).await?)
    }
}
