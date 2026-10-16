//! Persistent questions an agent has explicitly addressed to a person.

use ariadne_core::id::new_id;

use crate::{Result, Store, now};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AgentRequest {
    pub id: String,
    pub session_id: String,
    pub summary: String,
    pub created_at: String,
    pub answered_at: Option<String>,
}

impl Store {
    /// Open one request, or return the pending request this session
    /// already holds under the same summary. The flag says which: `true`
    /// for a row this call created, `false` for the one it found.
    pub async fn create_agent_request(
        &self,
        session_id: &str,
        summary: &str,
        turn_id: Option<&str>,
    ) -> Result<(AgentRequest, bool)> {
        let id = new_id();
        let result = sqlx::query(
            "INSERT INTO agent_requests (id, session_id, summary, created_at, last_raised_turn_id)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(session_id)
        .bind(summary)
        .bind(now())
        .bind(turn_id)
        .execute(self.w())
        .await;
        match result {
            Ok(_) => {}
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
                sqlx::query(
                    "UPDATE agent_requests SET last_raised_turn_id = ?
                     WHERE session_id = ? AND summary = ? AND answered_at IS NULL",
                )
                .bind(turn_id)
                .bind(session_id)
                .bind(summary)
                .execute(self.w())
                .await?;
                let existing = sqlx::query_as("SELECT * FROM agent_requests WHERE session_id = ? AND summary = ? AND answered_at IS NULL")
                    .bind(session_id).bind(summary).fetch_one(self.r()).await?;
                self.publish_session_update(session_id).await?;
                return Ok((existing, false));
            }
            Err(error) => return Err(error.into()),
        }
        let request = self.get_agent_request(&id).await?;
        self.publish_session_update(session_id).await?;
        Ok((request, true))
    }

    pub async fn pending_agent_requests(&self) -> Result<Vec<AgentRequest>> {
        Ok(
            sqlx::query_as("SELECT * FROM agent_requests WHERE answered_at IS NULL ORDER BY id")
                .fetch_all(self.r())
                .await?,
        )
    }

    /// End one turn's wait. A pending request survives only where that turn
    /// called `wait_for_user` with its hint, including a repeated hint.
    pub async fn finish_agent_request_turn(&self, session_id: &str, turn_id: &str) -> Result<()> {
        let answered = sqlx::query(
            "UPDATE agent_requests SET answered_at = ?
             WHERE session_id = ? AND answered_at IS NULL
                   AND (last_raised_turn_id IS NULL OR last_raised_turn_id <> ?)",
        )
        .bind(now())
        .bind(session_id)
        .bind(turn_id)
        .execute(self.w())
        .await?
        .rows_affected();
        if answered > 0 {
            self.publish_session_update(session_id).await?;
        }
        Ok(())
    }

    /// Console input is an answer to every question its session has open.
    pub async fn answer_agent_request(&self, session_id: &str) -> Result<()> {
        sqlx::query("UPDATE agent_requests SET answered_at = ? WHERE session_id = ? AND answered_at IS NULL")
            .bind(now()).bind(session_id).execute(self.w()).await?;
        self.publish_session_update(session_id).await?;
        Ok(())
    }

    pub async fn withdraw_agent_request(&self, id: &str, session_id: &str) -> Result<()> {
        sqlx::query("UPDATE agent_requests SET answered_at = ? WHERE id = ? AND session_id = ? AND answered_at IS NULL")
            .bind(now()).bind(id).bind(session_id).execute(self.w()).await?;
        self.publish_session_update(session_id).await?;
        Ok(())
    }

    async fn get_agent_request(&self, id: &str) -> Result<AgentRequest> {
        Ok(sqlx::query_as("SELECT * FROM agent_requests WHERE id = ?")
            .bind(id)
            .fetch_one(self.r())
            .await?)
    }
}
