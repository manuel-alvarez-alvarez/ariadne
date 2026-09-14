//! Agent-event repository: what the ACP runtime reports each agent doing.

use ariadne_core::id::new_id;

use crate::{AgentEvent, Change, Result, Store, now};

#[derive(Debug, Clone)]
pub struct NewAgentEvent {
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub kind: String,
    pub payload: serde_json::Value,
}

/// Which end of the recorded events a page is taken from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EventOrder {
    /// Forward from the oldest, which is what a sweep with `after` walks.
    #[default]
    Asc,
    /// Back from the newest, which is what a snapshot of the recent past
    /// wants.
    Desc,
}

#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    /// The goal an event belongs to, through its session or through its task.
    pub goal_id: Option<String>,
    /// Only events with an id above this one.
    pub after: Option<String>,
    /// Only events with an id below this one, which is how a descending page
    /// walks further back.
    pub before: Option<String>,
    pub order: EventOrder,
    pub limit: i64,
}

impl Store {
    pub async fn create_event(&self, new: NewAgentEvent) -> Result<AgentEvent> {
        // The id is taken and the event published under one lock, so two
        // writers racing on the write pool cannot publish a higher id before
        // a lower one: the order on the change channel is the order of ids.
        let _order = self.event_order().lock().await;
        let id = new_id();
        sqlx::query(
            "INSERT INTO agent_events (id, session_id, task_id, kind, payload, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&new.session_id)
        .bind(&new.task_id)
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

    /// The events of a session with an id above `after`, in order: what a
    /// console stream committed to the store but not yet published on the
    /// bus when a live event overtook it (008). `None` is every event.
    pub async fn list_session_events_after(
        &self,
        session_id: &str,
        after: Option<&str>,
    ) -> Result<Vec<AgentEvent>> {
        Ok(sqlx::query_as::<_, AgentEvent>(
            "SELECT * FROM agent_events WHERE session_id = ? AND id > ? ORDER BY id",
        )
        .bind(session_id)
        .bind(after.unwrap_or(""))
        .fetch_all(self.r())
        .await?)
    }

    /// Every event a session has produced, in order: the whole transcript an
    /// console replays from, where `list_events`'s page cap would
    /// truncate a long conversation.
    pub async fn list_session_events(&self, session_id: &str) -> Result<Vec<AgentEvent>> {
        Ok(sqlx::query_as::<_, AgentEvent>(
            "SELECT * FROM agent_events WHERE session_id = ? ORDER BY id",
        )
        .bind(session_id)
        .fetch_all(self.r())
        .await?)
    }

    /// A page of recorded events, narrowed by whichever filters were set.
    ///
    /// The query is assembled here rather than by `Filtered`, which builds one
    /// `SELECT * FROM <table>` and binds each clause once: the goal reaches an
    /// event through two other tables and binds its id twice. Every fragment
    /// below is a literal and every value is bound, which is what makes the
    /// assembled string safe to assert.
    pub async fn list_events(&self, filter: EventFilter) -> Result<Vec<AgentEvent>> {
        let limit = match filter.limit {
            n if n <= 0 => 50,
            n => n.min(200),
        };
        let mut sql = String::from("SELECT * FROM agent_events WHERE 1=1");
        let mut binds: Vec<String> = Vec::new();
        // A cursor is bound only where it is set: `id < ''` matches no row,
        // so an unset `before` written as an empty string would answer
        // nothing at all.
        if let Some(after) = filter.after {
            sql.push_str(" AND id > ?");
            binds.push(after);
        }
        if let Some(before) = filter.before {
            sql.push_str(" AND id < ?");
            binds.push(before);
        }
        if let Some(session_id) = filter.session_id {
            sql.push_str(" AND session_id = ?");
            binds.push(session_id);
        }
        if let Some(task_id) = filter.task_id {
            sql.push_str(" AND task_id = ?");
            binds.push(task_id);
        }
        // `agent_events` holds no goal of its own: an event belongs to the
        // goal of the session that reported it, or of the task it was on.
        // Both are read, because `agent_events.session_id` is `ON DELETE SET
        // NULL` and an event can carry a task and no session.
        if let Some(goal_id) = filter.goal_id {
            sql.push_str(
                " AND (session_id IN (SELECT id FROM agent_sessions WHERE goal_id = ?)
                    OR task_id IN (SELECT id FROM tasks WHERE goal_id = ?))",
            );
            binds.push(goal_id.clone());
            binds.push(goal_id);
        }
        sql.push_str(match filter.order {
            EventOrder::Asc => " ORDER BY id LIMIT ?",
            EventOrder::Desc => " ORDER BY id DESC LIMIT ?",
        });
        let mut q = sqlx::query_as::<_, AgentEvent>(sqlx::AssertSqlSafe(sql));
        for bind in binds {
            q = q.bind(bind);
        }
        Ok(q.bind(limit).fetch_all(self.r()).await?)
    }
}
