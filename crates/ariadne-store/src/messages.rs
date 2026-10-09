//! What the agents say to each other.
//!
//! One channel for all of it: a question of one agent to another, and the
//! answer back. A review used to run on it too; a workflow column moves a
//! task through the step routes now, and what is left on the channel is what
//! the agents say ([`MessageKind`]).
//!
//! A message has exactly one recipient. `delivered_at` says the recipient's
//! agent has the text. It is stamped when the prompt that carries it goes
//! out, or when a delivering read returns it, and never when a prompt is only
//! queued. An undelivered message is one its agent does not have yet, and a
//! read still hands it over — see `scheduler::messages`.

use ariadne_core::id::new_id;
use ariadne_core::{Actor, MessageKind};

use crate::query::Filtered;
use crate::{Change, Message, Result, Store, now};

/// One message to send: who it is from, who it is for, and what it says.
#[derive(Debug, Clone)]
pub struct NewMessage {
    pub goal_id: String,
    /// The task it is about, or None for a message about the goal itself.
    pub task_id: Option<String>,
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
    pub body: String,
}

/// What to read back out of the channel.
#[derive(Debug, Clone, Default)]
pub struct MessageFilter {
    pub goal_id: Option<String>,
    pub task_id: Option<String>,
    /// Messages for this staffed agent.
    pub to_agent_id: Option<String>,
    /// Messages for whoever sits in this seat, the orchestrator included.
    pub to_actor: Option<Actor>,
    /// Only the ones that have not reached their agent yet.
    pub undelivered_only: bool,
    /// Only the messages about the goal itself, and none about one of its
    /// tasks.
    ///
    /// A message about a task carries its goal too, so a filter on the goal
    /// alone reads every task's channel as well as the goal's own. The goal's
    /// channel is the orchestrator's inbox, and what is said about a task is
    /// on that task (spec 018).
    pub goal_channel_only: bool,
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
            "INSERT INTO messages (id, goal_id, task_id, kind, from_actor, from_agent_id,
                                   from_session, to_actor, to_agent_id, body, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&new.goal_id)
        .bind(&new.task_id)
        .bind(new.kind.as_str())
        .bind(new.from_actor.as_str())
        .bind(&new.from_agent_id)
        .bind(&new.from_session)
        .bind(new.to_actor.as_str())
        .bind(&new.to_agent_id)
        .bind(&new.body)
        .bind(now())
        .execute(&mut **tx)
        .await?;
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
            .maybe(" AND to_agent_id = ?", filter.to_agent_id)
            .maybe(" AND to_actor = ?", filter.to_actor.map(|a| a.as_str()))
            .flag(" AND delivered_at IS NULL", filter.undelivered_only)
            .flag(" AND task_id IS NULL", filter.goal_channel_only)
            .fetch(self, " ORDER BY id", &[])
            .await
    }

    /// Claim a message for delivery: stamp it, and answer whether this call
    /// is the one that did.
    ///
    /// The runtime claims a message right before its prompt goes out, and a
    /// delivering read claims what it returns. The two can race for one
    /// row, and the claim is one write gated on the row being unstamped, so
    /// exactly one of them wins. The first stamp is the one kept: a second
    /// claim answers false and leaves the time as it was.
    pub async fn mark_message_delivered(&self, id: &str) -> Result<bool> {
        let claimed = sqlx::query(
            "UPDATE messages SET delivered_at = ? WHERE id = ? AND delivered_at IS NULL",
        )
        .bind(now())
        .bind(id)
        .execute(self.w())
        .await?;
        Ok(claimed.rows_affected() == 1)
    }

    /// Give a claim back: the prompt it was made for never went out, so the
    /// message waits for the next live agent again.
    pub async fn unmark_message_delivered(&self, id: &str) -> Result<()> {
        sqlx::query("UPDATE messages SET delivered_at = NULL WHERE id = ?")
            .bind(id)
            .execute(self.w())
            .await?;
        Ok(())
    }
}
