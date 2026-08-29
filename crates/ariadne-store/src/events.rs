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

    /// Whether this session's log ends on a `tool` call nobody has answered.
    ///
    /// A tool that asks the user something is pending for as long as it takes
    /// them to answer, and the log is where that stretch of time is recorded:
    /// its `pre_tool_use` opens it, and the first of its own `post_tool_use`,
    /// a `user_prompt_submit` or a `stop` closes it. So the last event that
    /// does either is the whole answer — everything the turn reported in
    /// between belongs to its other tool calls and says nothing about the
    /// question.
    ///
    /// Which tool blocks on a person is the agent's vocabulary rather than
    /// the store's, so the caller names it (`ariadne-daemon`'s
    /// `classify::QUESTION_TOOL`).
    pub async fn tool_call_is_pending(&self, session_id: &str, tool: &str) -> Result<bool> {
        let last: Option<String> = sqlx::query_scalar(
            "SELECT kind FROM agent_events
              WHERE session_id = ?
                AND (kind IN ('user_prompt_submit', 'stop')
                     OR (kind IN ('pre_tool_use', 'post_tool_use')
                         AND json_extract(payload, '$.tool_name') = ?))
              ORDER BY id DESC
              LIMIT 1",
        )
        .bind(session_id)
        .bind(tool)
        .fetch_optional(self.r())
        .await?;
        Ok(last.as_deref() == Some("pre_tool_use"))
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
