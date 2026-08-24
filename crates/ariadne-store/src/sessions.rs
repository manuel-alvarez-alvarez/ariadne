//! Agent-session repository.

use ariadne_core::id::new_id;
use ariadne_core::{AgentKind, AttentionReason, Role, SessionStatus};

use crate::{AgentSession, Change, Result, Store, not_found, now};

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
        sqlx::query_as::<_, AgentSession>("SELECT * FROM agent_sessions WHERE id = ?")
            .bind(id)
            .fetch_optional(self.r())
            .await?
            .ok_or_else(|| not_found("session", id))
    }

    pub async fn list_sessions(&self, filter: SessionFilter) -> Result<Vec<AgentSession>> {
        let mut sql = String::from("SELECT * FROM agent_sessions WHERE 1=1");
        if filter.goal_id.is_some() {
            sql.push_str(" AND goal_id = ?");
        }
        if filter.task_id.is_some() {
            sql.push_str(" AND task_id = ?");
        }
        if filter.status.is_some() {
            sql.push_str(" AND status = ?");
        }
        if filter.live_only {
            sql.push_str(" AND status IN ('starting', 'running', 'idle')");
        }
        if filter.attention_only {
            sql.push_str(" AND attention_reason IS NOT NULL");
        }
        sql.push_str(" ORDER BY id");
        // Safe: only fixed clause fragments are appended; values are bound.
        let mut q = sqlx::query_as::<_, AgentSession>(sqlx::AssertSqlSafe(sql));
        if let Some(g) = &filter.goal_id {
            q = q.bind(g.clone());
        }
        if let Some(t) = &filter.task_id {
            q = q.bind(t.clone());
        }
        if let Some(s) = filter.status {
            q = q.bind(s.as_str());
        }
        Ok(q.fetch_all(self.r()).await?)
    }

    /// Move a session to a new lifecycle status.
    ///
    /// Retiring one takes any prompt-style attention down with it, wherever
    /// the status write came from: `waiting_permission` and `waiting_input`
    /// describe a dialog on the agent's terminal, and a session that has
    /// ended has no terminal to answer in — a flag left behind that way asks
    /// the user to go and reply to nobody. The reasons a session ends
    /// *carrying* (an error, a disconnect, a stall) are meant to outlive it
    /// and are left alone; so is a session that stays live.
    pub async fn set_session_status(&self, id: &str, status: SessionStatus) -> Result<()> {
        let ended_at = match status {
            SessionStatus::Exited | SessionStatus::Failed => Some(now()),
            _ => None,
        };
        let n = sqlx::query(
            "UPDATE agent_sessions SET status = ?, ended_at = COALESCE(?, ended_at) WHERE id = ?",
        )
        .bind(status.as_str())
        .bind(ended_at)
        .bind(id)
        .execute(self.w())
        .await?
        .rows_affected();
        if n == 0 {
            return Err(not_found("session", id));
        }
        if !status.is_live() {
            self.clear_prompt_attention(id).await?;
        }
        self.publish_session_update(id).await
    }

    /// Drop the prompt-style reasons (`AttentionReason::is_prompt`), leaving
    /// every other reason where it is. Its own statement next to the status
    /// write, whose `publish_session_update` runs after both: what watchers
    /// are handed is the session with the flag already gone.
    async fn clear_prompt_attention(&self, id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE agent_sessions
                SET attention_reason = NULL, attention_since = NULL
              WHERE id = ? AND attention_reason IN (?, ?)",
        )
        .bind(id)
        .bind(AttentionReason::WaitingPermission.as_str())
        .bind(AttentionReason::WaitingInput.as_str())
        .execute(self.w())
        .await?;
        Ok(())
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
    /// moment old, and a permission event being ingested as the session is
    /// retired must not write the dialog back onto the row the retirement
    /// just cleaned. The reasons a session ends carrying go up whatever its
    /// status is — flagging a dead agent is what `disconnected` is for.
    ///
    /// A withheld raise is not an error: the session exists, it just is not
    /// somewhere the flag means anything.
    pub async fn set_session_attention(&self, id: &str, reason: AttentionReason) -> Result<()> {
        // Same statuses as `SessionStatus::is_live`, in the one place SQL has
        // to know them next to the live filter in `list_sessions`.
        let while_live = match reason.is_prompt() {
            true => " AND status IN ('starting', 'running', 'idle')",
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
        if n == 0 {
            // The session is gone, it already carries this reason, or it has
            // ended and cannot be waiting on a dialog.
            self.get_session(id).await?;
            return Ok(());
        }
        self.sync_task_stall(id).await?;
        self.publish_session_update(id).await
    }

    /// The clear an agent's own event makes: every reason but the one raised
    /// for the user.
    ///
    /// An agent that is working again is not waiting on a permission prompt
    /// or on an answer, and says so by working — but `waiting_user` was never
    /// its flag. Nobody raised it on the agent's behalf and the agent getting
    /// on with something else is not the user having merged the request or
    /// read the message, so it stays up until the user acts or the work it is
    /// about stops being this session's ([`Store::clear_session_attention`],
    /// which the sweep calls, drops it like any other).
    pub async fn clear_agent_attention(&self, id: &str) -> Result<()> {
        let n = sqlx::query(
            "UPDATE agent_sessions
                SET attention_reason = NULL, attention_since = NULL
              WHERE id = ? AND attention_reason IS NOT NULL
                AND attention_reason <> ?",
        )
        .bind(id)
        .bind(AttentionReason::WaitingUser.as_str())
        .execute(self.w())
        .await?
        .rows_affected();
        if n == 0 {
            self.get_session(id).await?;
            return Ok(());
        }
        self.publish_session_update(id).await
    }

    /// Drop any attention flag from a session (the agent moved on).
    pub async fn clear_session_attention(&self, id: &str) -> Result<()> {
        let n = sqlx::query(
            "UPDATE agent_sessions
                SET attention_reason = NULL, attention_since = NULL
              WHERE id = ? AND attention_reason IS NOT NULL",
        )
        .bind(id)
        .execute(self.w())
        .await?
        .rows_affected();
        if n == 0 {
            self.get_session(id).await?;
            return Ok(());
        }
        self.sync_task_stall(id).await?;
        self.publish_session_update(id).await
    }

    /// Put a finished session back into its pre-spawn state so it can be
    /// relaunched under its own id: resuming an agent conversation in a fresh
    /// tmux keeps the one row (same id, same console log) instead of leaving a
    /// sibling behind per review round. `worktree_path` overwrites the stored
    /// one when given — the relaunch decides where the agent actually runs —
    /// and so does `review_round`, which for a reviewer session names the
    /// round it is being relaunched for.
    ///
    /// Whatever the session needed the user for is dropped too: a relaunch is
    /// the recovery, so an agent put back on its feet leaves the attention
    /// list instead of carrying the reason its previous run ended into a run
    /// that has not gone wrong yet.
    pub async fn restart_session(
        &self,
        id: &str,
        worktree_path: Option<&str>,
        review_round: Option<i64>,
    ) -> Result<AgentSession> {
        let n = sqlx::query(
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
        .bind(id)
        .execute(self.w())
        .await?
        .rows_affected();
        if n == 0 {
            return Err(not_found("session", id));
        }
        self.sync_task_stall(id).await?;
        let session = self.get_session(id).await?;
        self.publish(Change::SessionUpdated(session.clone()));
        Ok(session)
    }

    /// Record the agent-internal id (claude session uuid / codex thread id /
    /// opencode session id) once known.
    pub async fn set_session_internal_id(&self, id: &str, internal: &str) -> Result<()> {
        let n = sqlx::query("UPDATE agent_sessions SET internal_session_id = ? WHERE id = ?")
            .bind(internal)
            .bind(id)
            .execute(self.w())
            .await?
            .rows_affected();
        if n == 0 {
            return Err(not_found("session", id));
        }
        self.publish_session_update(id).await
    }

    /// Stamp the moment this session's agent process was started.
    ///
    /// Every launch overwrites it, resumes included: what a watcher asks of
    /// the column is whether *this* run of the agent has got going, and the
    /// run before it says nothing about that.
    pub async fn mark_session_launched(&self, id: &str) -> Result<()> {
        let n = sqlx::query("UPDATE agent_sessions SET launched_at = ? WHERE id = ?")
            .bind(now())
            .bind(id)
            .execute(self.w())
            .await?
            .rows_affected();
        if n == 0 {
            return Err(not_found("session", id));
        }
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

    async fn publish_session_update(&self, id: &str) -> Result<()> {
        let session = self.get_session(id).await?;
        self.publish(Change::SessionUpdated(session));
        Ok(())
    }
}
