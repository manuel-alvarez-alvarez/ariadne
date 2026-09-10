//! Searchable facts learned about one repository.

use ariadne_core::id::new_id;

use crate::{Change, Memory, Result, Store, not_found, now};

#[derive(Debug, Clone)]
pub struct NewMemory {
    pub repository_id: String,
    pub text: String,
    pub source_session_id: String,
    pub source_task_id: Option<String>,
    pub source_goal_id: String,
    pub expires_at: String,
}

impl Store {
    pub async fn create_memory(&self, new: NewMemory) -> Result<Memory> {
        self.get_repository(&new.repository_id).await?;
        let id = new_id();
        sqlx::query(
            "INSERT INTO memories
                (id, repository_id, text, source_session_id, source_task_id,
                 source_goal_id, created_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&new.repository_id)
        .bind(&new.text)
        .bind(&new.source_session_id)
        .bind(&new.source_task_id)
        .bind(&new.source_goal_id)
        .bind(now())
        .bind(&new.expires_at)
        .execute(self.w())
        .await?;
        let memory = self.get_memory(&new.repository_id, &id).await?;
        self.publish(Change::MemoryCreated(memory.clone()));
        Ok(memory)
    }

    pub async fn get_memory(&self, repository_id: &str, id: &str) -> Result<Memory> {
        sqlx::query_as::<_, Memory>("SELECT * FROM memories WHERE repository_id = ? AND id = ?")
            .bind(repository_id)
            .bind(id)
            .fetch_optional(self.r())
            .await?
            .ok_or_else(|| not_found("memory", id))
    }

    /// List active memories, newest first.
    pub async fn list_memories(&self, repository_id: &str) -> Result<Vec<Memory>> {
        self.get_repository(repository_id).await?;
        Ok(sqlx::query_as::<_, Memory>(
            "SELECT * FROM memories
              WHERE repository_id = ? AND expires_at > ?
              ORDER BY id DESC",
        )
        .bind(repository_id)
        .bind(now())
        .fetch_all(self.r())
        .await?)
    }

    /// Find active memories containing `query`, without case sensitivity.
    pub async fn search_memories(&self, repository_id: &str, query: &str) -> Result<Vec<Memory>> {
        self.get_repository(repository_id).await?;
        Ok(sqlx::query_as::<_, Memory>(
            "SELECT * FROM memories
              WHERE repository_id = ? AND expires_at > ?
                AND instr(lower(text), lower(?)) > 0
              ORDER BY id DESC",
        )
        .bind(repository_id)
        .bind(now())
        .bind(query)
        .fetch_all(self.r())
        .await?)
    }

    pub async fn delete_memory(&self, repository_id: &str, id: &str) -> Result<()> {
        self.get_memory(repository_id, id).await?;
        sqlx::query("DELETE FROM memories WHERE repository_id = ? AND id = ?")
            .bind(repository_id)
            .bind(id)
            .execute(self.w())
            .await?;
        self.publish(Change::MemoryDeleted(id.to_string()));
        Ok(())
    }
}
