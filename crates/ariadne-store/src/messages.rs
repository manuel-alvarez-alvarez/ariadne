//! Conversation-message repository.

use ariadne_core::AuthorRole;
use ariadne_core::id::new_id;

use crate::{Change, Message, Recipient, Result, Store, now};

#[derive(Debug, Clone)]
pub struct NewMessage {
    pub goal_id: String,
    /// None = goal-level thread (planner discussion).
    pub task_id: Option<String>,
    pub author_role: AuthorRole,
    pub author_session_id: Option<String>,
    /// Whom the message addresses. None = the thread, addressed to nobody in
    /// particular, which is what every message was before recipients existed.
    pub recipient: Option<Recipient>,
    pub body: String,
}

impl Store {
    pub async fn create_message(&self, new: NewMessage) -> Result<Message> {
        let id = new_id();
        sqlx::query(
            "INSERT INTO messages (id, goal_id, task_id, author_role, author_session_id,
                                   recipient_kind, recipient_profile_id, body, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&new.goal_id)
        .bind(&new.task_id)
        .bind(new.author_role.as_str())
        .bind(&new.author_session_id)
        .bind(new.recipient.as_ref().map(|r| r.kind().as_str()))
        .bind(new.recipient.as_ref().and_then(Recipient::profile_id))
        .bind(&new.body)
        .bind(now())
        .execute(self.w())
        .await?;
        let message = sqlx::query_as::<_, Message>("SELECT * FROM messages WHERE id = ?")
            .bind(&id)
            .fetch_one(self.r())
            .await?;
        self.publish(Change::MessageCreated(message.clone()));
        Ok(message)
    }

    /// Messages of a task thread, keyset-paginated by id.
    pub async fn list_task_messages(
        &self,
        task_id: &str,
        after: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Message>> {
        Ok(sqlx::query_as::<_, Message>(
            "SELECT * FROM messages WHERE task_id = ? AND id > ? ORDER BY id LIMIT ?",
        )
        .bind(task_id)
        .bind(after.unwrap_or(""))
        .bind(limit)
        .fetch_all(self.r())
        .await?)
    }

    /// Goal-level thread (task_id IS NULL), keyset-paginated by id.
    pub async fn list_goal_messages(
        &self,
        goal_id: &str,
        after: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Message>> {
        Ok(sqlx::query_as::<_, Message>(
            "SELECT * FROM messages WHERE goal_id = ? AND task_id IS NULL AND id > ? ORDER BY id LIMIT ?",
        )
        .bind(goal_id)
        .bind(after.unwrap_or(""))
        .bind(limit)
        .fetch_all(self.r())
        .await?)
    }
}
