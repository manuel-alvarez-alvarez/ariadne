//! Agent-session repository.

use ariadne_core::id::new_id;
use ariadne_core::{AgentKind, Role, SessionStatus};

use crate::{AgentSession, Change, Result, Store, not_found, now};

#[derive(Debug, Clone)]
pub struct NewSession {
    pub goal_id: String,
    pub task_id: Option<String>,
    pub role: Role,
    pub profile_id: String,
    pub agent_kind: AgentKind,
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
}

impl Store {
    /// Create a session row before spawning; its id becomes ARIADNE_SESSION_ID.
    pub async fn create_session(&self, new: NewSession) -> Result<AgentSession> {
        let id = new_id();
        sqlx::query(
            "INSERT INTO agent_sessions (id, goal_id, task_id, role, profile_id, agent_kind,
                                         tmux_session, worktree_path, review_round, status, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'starting', ?)",
        )
        .bind(&id)
        .bind(&new.goal_id)
        .bind(&new.task_id)
        .bind(new.role.as_str())
        .bind(&new.profile_id)
        .bind(new.agent_kind.as_str())
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
        self.publish_session_update(id).await
    }

    /// Put a finished session back into its pre-spawn state so it can be
    /// relaunched under its own id: resuming an agent conversation in a fresh
    /// tmux keeps the one row (same id, same console log) instead of leaving a
    /// sibling behind per review round. `worktree_path` overwrites the stored
    /// one when given — the relaunch decides where the agent actually runs —
    /// and so does `review_round`, which for a reviewer session names the
    /// round it is being relaunched for.
    pub async fn restart_session(
        &self,
        id: &str,
        worktree_path: Option<&str>,
        review_round: Option<i64>,
    ) -> Result<AgentSession> {
        let n = sqlx::query(
            "UPDATE agent_sessions
                SET status = 'starting', ended_at = NULL, last_activity_at = ?,
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
