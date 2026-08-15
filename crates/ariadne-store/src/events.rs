//! Agent-event repository (raw hook/notify/plugin payloads).

use ariadne_core::AgentKind;
use ariadne_core::id::new_id;

use crate::{AgentEvent, Result, Store, now};

#[derive(Debug, Clone)]
pub struct NewAgentEvent {
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub agent_kind: Option<AgentKind>,
    pub kind: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub after: Option<String>,
    pub limit: i64,
}

impl Store {
    pub async fn create_event(&self, new: NewAgentEvent) -> Result<AgentEvent> {
        let id = new_id();
        sqlx::query(
            "INSERT INTO agent_events (id, session_id, task_id, agent_kind, kind, payload, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&new.session_id)
        .bind(&new.task_id)
        .bind(new.agent_kind.map(|k| k.as_str()))
        .bind(&new.kind)
        .bind(new.payload.to_string())
        .bind(now())
        .execute(self.w())
        .await?;
        Ok(
            sqlx::query_as::<_, AgentEvent>("SELECT * FROM agent_events WHERE id = ?")
                .bind(&id)
                .fetch_one(self.r())
                .await?,
        )
    }

    pub async fn list_events(&self, filter: EventFilter) -> Result<Vec<AgentEvent>> {
        let mut sql = String::from("SELECT * FROM agent_events WHERE id > ?");
        if filter.session_id.is_some() {
            sql.push_str(" AND session_id = ?");
        }
        if filter.task_id.is_some() {
            sql.push_str(" AND task_id = ?");
        }
        sql.push_str(" ORDER BY id LIMIT ?");
        // Safe: only fixed clause fragments are appended; values are bound.
        let mut q = sqlx::query_as::<_, AgentEvent>(sqlx::AssertSqlSafe(sql))
            .bind(filter.after.unwrap_or_default());
        if let Some(s) = &filter.session_id {
            q = q.bind(s.clone());
        }
        if let Some(t) = &filter.task_id {
            q = q.bind(t.clone());
        }
        let limit = if filter.limit <= 0 {
            50
        } else {
            filter.limit.min(200)
        };
        Ok(q.bind(limit).fetch_all(self.r()).await?)
    }
}
