//! Agent-event repository (raw hook/notify/plugin payloads).

use ariadne_core::AgentKind;
use ariadne_core::id::new_id;

use crate::query::Filtered;
use crate::{AgentEvent, Change, Result, Store, now};

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
        let event = sqlx::query_as::<_, AgentEvent>("SELECT * FROM agent_events WHERE id = ?")
            .bind(&id)
            .fetch_one(self.r())
            .await?;
        self.publish(Change::AgentEventCreated(event.clone()));
        Ok(event)
    }

    pub async fn list_events(&self, filter: EventFilter) -> Result<Vec<AgentEvent>> {
        let limit = match filter.limit {
            n if n <= 0 => 50,
            n => n.min(200),
        };
        Filtered::new("agent_events")
            .maybe(" AND id > ?", Some(filter.after.unwrap_or_default()))
            .maybe(" AND session_id = ?", filter.session_id)
            .maybe(" AND task_id = ?", filter.task_id)
            .fetch(self, " ORDER BY id LIMIT ?", &[limit])
            .await
    }
}
