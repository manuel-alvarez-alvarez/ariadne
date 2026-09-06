//! What the agents say to each other.
//!
//! One channel for all of it. A verdict used to be a table of its own, and the
//! consequence was that the only thing a reviewer could say to an author was
//! approve or request changes — a reviewer that did not understand the change
//! had nowhere to ask. Everything an agent says is a message now, and what
//! tells them apart is [`MessageKind`].
//!
//! A message has exactly one recipient. The daemon types it into that
//! recipient's pane and stamps `delivered_at`, so an undelivered message is
//! one still waiting for a pane to be free — see
//! `scheduler::messages`.

use ariadne_core::id::new_id;
use ariadne_core::{Actor, MessageKind};

use crate::query::Filtered;
use crate::{Change, Message, Result, Store, StoreError, now};

/// One message to send: who it is from, who it is for, and what it says.
#[derive(Debug, Clone)]
pub struct NewMessage {
    pub goal_id: String,
    /// The task it is about, or None for a message about the goal itself.
    pub task_id: Option<String>,
    /// The review round it belongs to. Read for a verdict and ignored
    /// otherwise, so a question asked mid-round is not counted as one.
    pub round: i64,
    pub kind: MessageKind,
    pub from_actor: Actor,
    /// The staffed agent that sent it, or None for the orchestrator, the
    /// daemon and the user, which are staffed on no task.
    pub from_agent_id: Option<String>,
    /// The session it was sent from, for the record.
    pub from_session: Option<String>,
    pub to_actor: Actor,
    /// The staffed agent it is for, or None for the orchestrator.
    pub to_agent_id: Option<String>,
    /// The message this answers, where it answers one.
    pub in_reply_to: Option<String>,
    pub body: String,
}

/// What to read back out of the channel.
#[derive(Debug, Clone, Default)]
pub struct MessageFilter {
    pub goal_id: Option<String>,
    pub task_id: Option<String>,
    /// The round, for the verdicts of one round alone.
    pub round: Option<i64>,
    /// Messages for this staffed agent.
    pub to_agent_id: Option<String>,
    /// Messages for whoever sits in this seat, the orchestrator included.
    pub to_actor: Option<Actor>,
    /// Only the ones that have not reached a pane yet.
    pub undelivered_only: bool,
}

impl Store {
    /// Send a message. What carries it to the recipient is the scheduler; this
    /// is the record of it.
    pub async fn send_message(&self, new: NewMessage) -> Result<Message> {
        let mut tx = self.w().begin().await?;
        let message = Self::insert_message_in_tx(&mut tx, &new).await?;
        tx.commit().await?;
        self.publish(Change::MessageSent(message.clone()));
        Ok(message)
    }

    /// Write one message inside the caller's transaction and read back the row
    /// it became, so a message that belongs with other writes commits with
    /// them. Announcing it is the caller's, after the commit: a row nobody has
    /// committed is not news.
    pub(crate) async fn insert_message_in_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        new: &NewMessage,
    ) -> Result<Message> {
        let id = new_id();
        sqlx::query(
            "INSERT INTO messages (id, goal_id, task_id, round, kind, from_actor, from_agent_id,
                                   from_session, to_actor, to_agent_id, in_reply_to, body,
                                   created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&new.goal_id)
        .bind(&new.task_id)
        .bind(new.round)
        .bind(new.kind.as_str())
        .bind(new.from_actor.as_str())
        .bind(&new.from_agent_id)
        .bind(&new.from_session)
        .bind(new.to_actor.as_str())
        .bind(&new.to_agent_id)
        .bind(&new.in_reply_to)
        .bind(&new.body)
        .bind(now())
        .execute(&mut **tx)
        .await
        .map_err(|e| match e {
            // The one uniqueness there is: a reviewer votes once a round.
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                StoreError::Conflict(format!(
                    "agent {} has already given its verdict on round {} of task {}",
                    new.from_agent_id.as_deref().unwrap_or("?"),
                    new.round,
                    new.task_id.as_deref().unwrap_or("?"),
                ))
            }
            other => StoreError::Db(other),
        })?;
        Ok(
            sqlx::query_as::<_, Message>("SELECT * FROM messages WHERE id = ?")
                .bind(&id)
                .fetch_one(&mut **tx)
                .await?,
        )
    }

    pub async fn get_message(&self, id: &str) -> Result<Message> {
        self.fetch_by("message", "messages", "id", id).await
    }

    /// The messages a filter names, oldest first: a channel reads in the order
    /// it was written.
    pub async fn list_messages(&self, filter: MessageFilter) -> Result<Vec<Message>> {
        Filtered::new("messages")
            .maybe(" AND goal_id = ?", filter.goal_id)
            .maybe(" AND task_id = ?", filter.task_id)
            .maybe(" AND round = ?", filter.round.map(|r| r.to_string()))
            .maybe(" AND to_agent_id = ?", filter.to_agent_id)
            .maybe(" AND to_actor = ?", filter.to_actor.map(|a| a.as_str()))
            .flag(" AND delivered_at IS NULL", filter.undelivered_only)
            .fetch(self, " ORDER BY id", &[])
            .await
    }

    /// The verdicts of one round of a task, which is what closes it.
    pub async fn round_verdicts(&self, task_id: &str, round: i64) -> Result<Vec<Message>> {
        Ok(self
            .list_messages(MessageFilter {
                task_id: Some(task_id.to_string()),
                round: Some(round),
                ..Default::default()
            })
            .await?
            .into_iter()
            .filter(|m| m.kind().is_some_and(|k| k.is_verdict()))
            .collect())
    }

    /// Stamp a message as delivered: it reached the recipient's pane.
    ///
    /// Idempotent, and the first stamp is the one kept: a message typed twice
    /// is a bug in the caller, and overwriting the time would hide it.
    pub async fn mark_message_delivered(&self, id: &str) -> Result<()> {
        sqlx::query("UPDATE messages SET delivered_at = ? WHERE id = ? AND delivered_at IS NULL")
            .bind(now())
            .bind(id)
            .execute(self.w())
            .await?;
        Ok(())
    }
}
