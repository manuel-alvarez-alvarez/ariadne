//! Agent-session repository.

use ariadne_core::id::new_id;
use ariadne_core::{AgentKind, AttentionReason, Role, SessionStatus};

use crate::query::Filtered;
use crate::{AgentSession, Change, Result, Store, not_found, now};

/// [`SessionStatus::is_live`] in SQL, in the one place SQL has to know it.
const LIVE_STATUSES: &str = " AND status IN ('starting', 'running', 'idle')";

/// The clause naming the reasons a retired session can no longer be waiting
/// on ([`AttentionReason::is_prompt`]).
const PROMPTS_ONLY: &str = " AND attention_reason IN (?, ?)";

/// The clause naming the reasons an agent's own idle disproves — the silence
/// and the failed turn of [`Store::clear_attention_after_idle`].
const SILENCE_AND_ERROR: &str = " AND attention_reason IN (?, ?)";

#[derive(Debug, Clone)]
pub struct NewSession {
    pub goal_id: String,
    pub task_id: Option<String>,
    pub role: Role,
    pub profile_id: String,
    pub agent_kind: AgentKind,
    /// Model to launch with; None = the agent CLI's own default.
    pub model: Option<String>,
    pub tmux_session: String,
    pub worktree_path: Option<String>,
    pub review_round: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct SessionFilter {
    pub goal_id: Option<String>,
    pub task_id: Option<String>,
    pub status: Option<SessionStatus>,
    /// Only sessions in a live status (starting/running/idle).
    pub live_only: bool,
    /// Only sessions currently flagged as needing attention.
    pub attention_only: bool,
}

impl Store {
    /// Create a session row before spawning; its id becomes ARIADNE_SESSION_ID.
    pub async fn create_session(&self, new: NewSession) -> Result<AgentSession> {
        let id = new_id();
        sqlx::query(
            "INSERT INTO agent_sessions (id, goal_id, task_id, role, profile_id, agent_kind, model,
                                         tmux_session, worktree_path, review_round, status, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'starting', ?)",
        )
        .bind(&id)
        .bind(&new.goal_id)
        .bind(&new.task_id)
        .bind(new.role.as_str())
        .bind(&new.profile_id)
        .bind(new.agent_kind.as_str())
        .bind(&new.model)
        .bind(&new.tmux_session)
        .bind(&new.worktree_path)
        .bind(new.review_round)
        .bind(now())
        .execute(self.w())
        .await?;
        let session = self.get_session(&id).await?;
        self.publish(Change::SessionCreated(session.clone()));
        Ok(session)
    }

    pub async fn get_session(&self, id: &str) -> Result<AgentSession> {
        self.fetch_by("session", "agent_sessions", "id", id).await
    }

    pub async fn list_sessions(&self, filter: SessionFilter) -> Result<Vec<AgentSession>> {
        Filtered::new("agent_sessions")
            .maybe(" AND goal_id = ?", filter.goal_id)
            .maybe(" AND task_id = ?", filter.task_id)
            .maybe(" AND status = ?", filter.status.map(|s| s.as_str()))
            .flag(LIVE_STATUSES, filter.live_only)
            .flag(" AND attention_reason IS NOT NULL", filter.attention_only)
            .fetch(self, " ORDER BY id", &[])
            .await
    }

    /// Move a session to a new lifecycle status.
    ///
    /// Retiring one takes any prompt-style attention down with it: a session
    /// that has ended has no terminal to answer a dialog in, and a flag left
    /// behind that way asks the user to go and reply to nobody. The reasons a
    /// session ends *carrying* — an error, a disconnect, a stall — are meant
    /// to outlive it and are left alone.
    pub async fn set_session_status(&self, id: &str, status: SessionStatus) -> Result<()> {
        let ended_at = match status {
            SessionStatus::Exited | SessionStatus::Failed => Some(now()),
            _ => None,
        };
        self.write_session(
            id,
            sqlx::query(
                "UPDATE agent_sessions SET status = ?, ended_at = COALESCE(?, ended_at) WHERE id = ?",
            )
            .bind(status.as_str())
            .bind(ended_at)
            .bind(id),
        )
        .await?;
        if !status.is_live() {
            // Its own statement before the announcement, so what watchers are
            // handed is the session with the flag already gone.
            let prompts = [
                AttentionReason::WaitingPermission.as_str(),
                AttentionReason::WaitingInput.as_str(),
            ];
            self.clear_attention(id, PROMPTS_ONLY, &prompts).await?;
        }
        self.publish_session_update(id).await
    }

    /// Flag a session as needing the user's attention.
    ///
    /// Re-raising the reason already stored is a no-op, `attention_since`
    /// included: a detector that keeps seeing the same permission prompt must
    /// not keep resetting how long the agent has been stuck on it.
    ///
    /// A prompt-style reason additionally only lands on a session that is
    /// still live, and that half of the question rides in the `UPDATE` rather
    /// than being asked first: the caller's view of the status is always a
    /// moment old, and a permission event ingested as the session is retired
    /// must not write the dialog back onto the row the retirement just
    /// cleaned. A withheld raise is not an error — the session exists, the
    /// flag just means nothing there.
    pub async fn set_session_attention(&self, id: &str, reason: AttentionReason) -> Result<()> {
        let while_live = match reason.is_prompt() {
            true => LIVE_STATUSES,
            false => "",
        };
        let n = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE agent_sessions
                SET attention_reason = ?, attention_since = ?
              WHERE id = ? AND (attention_reason IS NULL OR attention_reason <> ?){while_live}"
        )))
        .bind(reason.as_str())
        .bind(now())
        .bind(id)
        .bind(reason.as_str())
        .execute(self.w())
        .await?
        .rows_affected();
        // A write that changed nothing: the session is gone, it already
        // carries this reason, or it has ended and cannot be waiting on a
        // dialog. Only the first of those is an error.
        self.announce_attention(id, n).await
    }

    /// The clear an agent's own event makes: every reason but the one raised
    /// for the user.
    ///
    /// An agent that is working again says so by working — but `waiting_user`
    /// was never its flag, and the agent getting on with something else is
    /// not the user having merged the request or read the message. It stays
    /// up until the user acts, or until the sweep's
    /// [`Store::clear_session_attention`] drops it like any other.
    pub async fn clear_agent_attention(&self, id: &str) -> Result<()> {
        let cleared = self
            .clear_attention(
                id,
                " AND attention_reason <> ?",
                &[AttentionReason::WaitingUser.as_str()],
            )
            .await?;
        self.announce_attention(id, cleared).await
    }

    /// The clear a session reporting itself idle makes: the two reasons its
    /// own report has just disproved, and no others.
    ///
    /// `stalled` says the agent stopped reporting, and an agent that reported
    /// anything at all is not silent. `agent_error` says a turn failed, and a
    /// turn that ends on idle rather than on another error has recovered — the
    /// session is back at its prompt, which is where the next instruction is
    /// taken.
    ///
    /// Everything else stands. Going idle is exactly when a permission prompt
    /// or a question is up, so the prompt reasons survive it, and
    /// `waiting_user` was never the agent's to take down — see
    /// [`Store::clear_agent_attention`], which is the clear a session going
    /// back to *work* makes instead.
    pub async fn clear_attention_after_idle(&self, id: &str) -> Result<()> {
        let reasons = [
            AttentionReason::Stalled.as_str(),
            AttentionReason::AgentError.as_str(),
        ];
        let cleared = self.clear_attention(id, SILENCE_AND_ERROR, &reasons).await?;
        self.announce_attention(id, cleared).await
    }

    /// Drop any attention flag from a session (the agent moved on).
    pub async fn clear_session_attention(&self, id: &str) -> Result<()> {
        let cleared = self.clear_attention(id, "", &[]).await?;
        self.announce_attention(id, cleared).await
    }

    /// Take the attention flag down, narrowed by `and`: the caller's clause
    /// says which reasons its clear is allowed to take with it. Answers how
    /// many rows it changed, which is none for a session with nothing up.
    async fn clear_attention(&self, id: &str, and: &str, reasons: &[&str]) -> Result<u64> {
        let mut q = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE agent_sessions
                SET attention_reason = NULL, attention_since = NULL
              WHERE id = ? AND attention_reason IS NOT NULL{and}"
        )))
        .bind(id);
        for reason in reasons {
            q = q.bind(*reason);
        }
        Ok(q.execute(self.w()).await?.rows_affected())
    }

    /// Put a finished session back into its pre-spawn state so it can be
    /// relaunched under its own id: resuming a conversation in a fresh tmux
    /// keeps the one row — same id, same console log — instead of leaving a
    /// sibling behind per review round. `worktree_path` and `review_round`
    /// overwrite the stored ones when given, since the relaunch is what
    /// decides them.
    ///
    /// Whatever the session needed the user for is dropped too: a relaunch is
    /// the recovery, so an agent put back on its feet does not carry the
    /// reason its previous run ended into a run that has not gone wrong.
    pub async fn restart_session(
        &self,
        id: &str,
        worktree_path: Option<&str>,
        review_round: Option<i64>,
    ) -> Result<AgentSession> {
        self.write_session(
            id,
            sqlx::query(
                "UPDATE agent_sessions
                SET status = 'starting', ended_at = NULL, last_activity_at = ?,
                    attention_reason = NULL, attention_since = NULL,
                    worktree_path = COALESCE(?, worktree_path),
                    review_round = COALESCE(?, review_round)
              WHERE id = ?",
            )
            .bind(now())
            .bind(worktree_path)
            .bind(review_round)
            .bind(id),
        )
        .await?;
        self.sync_task_stall(id).await?;
        let session = self.get_session(id).await?;
        self.publish(Change::SessionUpdated(session.clone()));
        Ok(session)
    }

    /// Record the agent-internal id (claude session uuid / codex thread id /
    /// opencode session id) once known.
    pub async fn set_session_internal_id(&self, id: &str, internal: &str) -> Result<()> {
        self.write_session(
            id,
            sqlx::query("UPDATE agent_sessions SET internal_session_id = ? WHERE id = ?")
                .bind(internal)
                .bind(id),
        )
        .await?;
        self.publish_session_update(id).await
    }

    /// Stamp the moment this session's agent process was started. Every
    /// launch overwrites it, resumes included: what a watcher asks of the
    /// column is whether *this* run has got going.
    pub async fn mark_session_launched(&self, id: &str) -> Result<()> {
        self.write_session(
            id,
            sqlx::query("UPDATE agent_sessions SET launched_at = ? WHERE id = ?")
                .bind(now())
                .bind(id),
        )
        .await?;
        self.publish_session_update(id).await
    }

    pub async fn touch_session(&self, id: &str) -> Result<()> {
        let n = sqlx::query("UPDATE agent_sessions SET last_activity_at = ? WHERE id = ?")
            .bind(now())
            .bind(id)
            .execute(self.w())
            .await?
            .rows_affected();
        if n == 0 {
            return Ok(());
        }
        self.publish_session_update(id).await
    }

    /// One write against a session row, refusing an id that names none.
    async fn write_session<'q>(
        &self,
        id: &str,
        query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments>,
    ) -> Result<()> {
        match query.execute(self.w()).await?.rows_affected() {
            0 => Err(not_found("session", id)),
            _ => Ok(()),
        }
    }

    /// Announce an attention write and the task stall it may have moved with
    /// it. A write that matched no row still has to answer for the session:
    /// it may simply not exist.
    async fn announce_attention(&self, id: &str, changed: u64) -> Result<()> {
        if changed == 0 {
            self.get_session(id).await?;
            return Ok(());
        }
        self.sync_task_stall(id).await?;
        self.publish_session_update(id).await
    }

    pub(crate) async fn publish_session_update(&self, id: &str) -> Result<()> {
        let session = self.get_session(id).await?;
        self.publish(Change::SessionUpdated(session));
        Ok(())
    }
}
