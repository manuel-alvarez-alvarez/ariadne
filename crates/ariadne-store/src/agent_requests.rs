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
    pub async fn create_agent_request(
        &self,
        session_id: &str,
        summary: &str,
    ) -> Result<AgentRequest> {
        let id = new_id();
        let result = sqlx::query(
            "INSERT INTO agent_requests (id, session_id, summary, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(session_id)
        .bind(summary)
        .bind(now())
        .execute(self.w())
        .await;
        match result {
            Ok(_) => {}
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
                return Ok(sqlx::query_as("SELECT * FROM agent_requests WHERE session_id = ? AND summary = ? AND answered_at IS NULL")
                    .bind(session_id).bind(summary).fetch_one(self.r()).await?);
            }
            Err(error) => return Err(error.into()),
        }
        let request = self.get_agent_request(&id).await?;
        self.publish_session_update(session_id).await?;
        Ok(request)
    }

    pub async fn pending_agent_requests(&self) -> Result<Vec<AgentRequest>> {
        Ok(
            sqlx::query_as("SELECT * FROM agent_requests WHERE answered_at IS NULL ORDER BY id")
                .fetch_all(self.r())
                .await?,
        )
    }

    pub async fn answer_agent_request(&self, session_id: &str) -> Result<()> {
        sqlx::query("UPDATE agent_requests SET answered_at = ? WHERE id = (SELECT id FROM agent_requests WHERE session_id = ? AND answered_at IS NULL ORDER BY id LIMIT 1)")
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
