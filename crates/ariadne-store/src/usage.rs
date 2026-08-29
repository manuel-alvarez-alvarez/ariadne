//! Token-usage repository: what each session spent, and the rollups a task
//! and a goal are read with.
//!
//! One row per `(session_id, source)`, holding that transcript's cumulative
//! totals — see `migrations/0001_init.sql` for what a source is and why a
//! report replaces rather than adds. Everything above a session is a `SUM`
//! over those rows, grouped by whatever the reader groups by, and a session
//! nothing has reported for sums to zero rather than to nothing.

use ariadne_core::{Role, TokenUsage};

use crate::{Change, Result, Store, now};

/// The usage of one profile in one role — the engineer of a task, or one of
/// its reviewers with every round it sat summed together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileUsage {
    pub role: Role,
    pub profile_id: String,
    pub usage: TokenUsage,
}

/// The usage of every session of one role, whichever profile ran them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleUsage {
    pub role: Role,
    pub usage: TokenUsage,
}

/// The three counters as SQLite holds them: a sum that no row contributed to
/// is `NULL`, and a reader wants a zero.
const SUMS: &str = "COALESCE(SUM(input_tokens), 0), \
                    COALESCE(SUM(cached_input_tokens), 0), \
                    COALESCE(SUM(output_tokens), 0)";

/// One summed row, in the order [`SUMS`] selects them.
type Sums = (i64, i64, i64);

impl Store {
    /// Record what one transcript of a session has spent so far, replacing
    /// whatever it last reported.
    ///
    /// Answers whether that moved anything: a report identical to the stored
    /// one is written nowhere and announced to nobody, which matters because
    /// agents re-report their totals on every event they send.
    ///
    /// A report that does move it is announced three times over — the
    /// session, the task that owns it where there is one, and the goal —
    /// since each of those carries the rollup in its own fat event, and a
    /// watcher holding a stale task would otherwise never hear that its
    /// figures moved.
    pub async fn upsert_session_usage(
        &self,
        session_id: &str,
        source: &str,
        usage: TokenUsage,
    ) -> Result<bool> {
        let changed = sqlx::query(
            "INSERT INTO session_usage (session_id, source, input_tokens,
                                        cached_input_tokens, output_tokens, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT (session_id, source) DO UPDATE
                SET input_tokens        = excluded.input_tokens,
                    cached_input_tokens = excluded.cached_input_tokens,
                    output_tokens       = excluded.output_tokens,
                    updated_at          = excluded.updated_at
              WHERE input_tokens        <> excluded.input_tokens
                 OR cached_input_tokens <> excluded.cached_input_tokens
                 OR output_tokens       <> excluded.output_tokens",
        )
        .bind(session_id)
        .bind(source)
        .bind(stored(usage.input_tokens))
        .bind(stored(usage.cached_input_tokens))
        .bind(stored(usage.output_tokens))
        .bind(now())
        .execute(self.w())
        .await?
        .rows_affected()
            > 0;
        if !changed {
            return Ok(false);
        }
        let session = self.get_session(session_id).await?;
        self.publish(Change::SessionUpdated(session.clone()));
        if let Some(task_id) = &session.task_id {
            self.publish_task_update(task_id, 1).await?;
        }
        self.publish_goal_update(&session.goal_id).await?;
        Ok(true)
    }

    /// What one session has spent, summed over every transcript it reported
    /// under.
    pub async fn session_usage(&self, session_id: &str) -> Result<TokenUsage> {
        let sums: Sums = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {SUMS} FROM session_usage WHERE session_id = ?"
        )))
        .bind(session_id)
        .fetch_one(self.r())
        .await?;
        Ok(usage_of(sums))
    }

    /// What a task has spent, one entry per `(role, profile)` that has a
    /// session on it: its engineer, and each reviewer with all its rounds
    /// summed. Ordered by role and then by profile id, so a reader sees the
    /// same order twice running.
    ///
    /// The join is outer because having spent nothing is not the same as not
    /// being here: a reviewer whose session has yet to report reads as zeros,
    /// and a reviewer with no session at all is absent.
    pub async fn task_usage(&self, task_id: &str) -> Result<Vec<ProfileUsage>> {
        let rows: Vec<(String, String, i64, i64, i64)> =
            sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "SELECT s.role, s.profile_id, {SUMS}
                   FROM agent_sessions s
              LEFT JOIN session_usage u ON u.session_id = s.id
                  WHERE s.task_id = ?
               GROUP BY s.role, s.profile_id
               ORDER BY s.role, s.profile_id"
            )))
            .bind(task_id)
            .fetch_all(self.r())
            .await?;
        Ok(rows
            .into_iter()
            .map(|(role, profile_id, input, cached, output)| ProfileUsage {
                role: role.parse().expect("valid role in db"),
                profile_id,
                usage: usage_of((input, cached, output)),
            })
            .collect())
    }

    /// What a goal has spent, one entry per role that has a session on it —
    /// its planner, every engineer of its tasks, every reviewer of them.
    /// Outer-joined like [`Store::task_usage`], and for the same reason.
    pub async fn goal_usage(&self, goal_id: &str) -> Result<Vec<RoleUsage>> {
        let rows: Vec<(String, i64, i64, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT s.role, {SUMS}
               FROM agent_sessions s
          LEFT JOIN session_usage u ON u.session_id = s.id
              WHERE s.goal_id = ?
           GROUP BY s.role
           ORDER BY s.role"
        )))
        .bind(goal_id)
        .fetch_all(self.r())
        .await?;
        Ok(rows
            .into_iter()
            .map(|(role, input, cached, output)| RoleUsage {
                role: role.parse().expect("valid role in db"),
                usage: usage_of((input, cached, output)),
            })
            .collect())
    }
}

/// A counter on its way into SQLite, whose integers are signed. Nothing an
/// agent reports comes near the clamp; it is here so that a number that does
/// is stored as the largest one rather than refused.
fn stored(tokens: u64) -> i64 {
    i64::try_from(tokens).unwrap_or(i64::MAX)
}

/// One summed row read back. A negative total is a column somebody wrote by
/// hand — the ingestion never stores one — and reads as zero rather than
/// wrapping.
fn usage_of((input, cached, output): Sums) -> TokenUsage {
    TokenUsage {
        input_tokens: input.max(0) as u64,
        cached_input_tokens: cached.max(0) as u64,
        output_tokens: output.max(0) as u64,
    }
}
