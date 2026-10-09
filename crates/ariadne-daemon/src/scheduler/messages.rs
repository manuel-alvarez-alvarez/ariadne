//! Carrying what one agent said to another to the agent it was said to.
//!
//! A message is written by the agent that sent it and delivered by the daemon:
//! handed to the recipient's agent as a `session/prompt`, the way a nudge is.
//! That is the whole of the transport — there is no inbox an agent has to
//! poll, and nothing it has to be told to check, because the message arrives
//! as a turn.
//!
//! A message to an agent mid-turn is queued behind the turn by the runtime.
//! `delivered_at` says the agent has the text. Nothing stamps a message when
//! it is queued: the driver claims it right before its prompt goes out, and a
//! delivering read claims what it returns. A queued message is unstamped, so
//! an agent that waits inside its turn still gets it through a read. Its
//! prompt is then skipped, because the claim fails.

use tracing::{debug, info};

use ariadne_core::{Actor, PromptKind, Seat};
use ariadne_store::{AgentSession, Message, MessageFilter, SessionFilter};

use crate::agents::prompts;

impl super::Scheduler {
    /// Deliver everything still waiting for an agent on this task.
    pub(super) async fn deliver_task_messages(&mut self, task_id: &str) {
        let waiting = self
            .store
            .list_messages(MessageFilter {
                task_id: Some(task_id.to_string()),
                undelivered_only: true,
                ..Default::default()
            })
            .await;
        self.deliver(waiting.unwrap_or_default()).await;
    }

    /// And everything waiting on the goal's own channel, which is the
    /// orchestrator's inbox.
    pub(super) async fn deliver_goal_messages(&mut self, goal_id: &str) {
        // A message about a task is delivered on that task's pass, so the
        // goal's own pass carries only what is not about one.
        let waiting = self
            .store
            .list_messages(MessageFilter {
                goal_id: Some(goal_id.to_string()),
                goal_channel_only: true,
                undelivered_only: true,
                ..Default::default()
            })
            .await;
        self.deliver(waiting.unwrap_or_default()).await;
    }

    /// One pass over a batch of undelivered messages, in the order they were
    /// written: the runtime queues each prompt behind the one before it.
    ///
    /// A message to the agent of a column the task is not in waits: that
    /// agent sits idle until its column is current, and a message is not a
    /// reason to wake it.
    async fn deliver(&mut self, waiting: Vec<Message>) {
        for message in waiting {
            let Some(session) = self.recipient_session(&message).await else {
                debug!(message = %message.id, "nothing live to deliver the message to yet");
                continue;
            };
            if session.seat() == Some(Seat::Agent)
                && !crate::attention::work_is_active(&self.store, &session).await
            {
                continue;
            }
            let seat = message.from_actor().map_or("agent", |actor| actor.as_str());
            let task_title = self.sender_task_title(&message).await;
            let skills = self.sender_skills(&message).await;
            let template = prompts::template_for(PromptKind::IncomingMessage);
            let text = prompts::incoming_message_briefing(
                template,
                &message,
                seat,
                task_title.as_deref(),
                &skills,
            );
            info!(
                message = %message.id,
                session = %session.id,
                kind = %message.kind,
                "delivering a message to the agent it is for"
            );
            // Stamped by nobody here: the driver claims it when its prompt
            // goes out. One the runtime refused — the agent went away
            // between the lookup and the hand-off — waits for the next pass.
            self.hand_message(&session, &message.id, text);
        }
    }

    /// The live session a message is for, or None while there is none.
    ///
    /// A message outlives the session that will read it: an agent that is
    /// being started again gets it on the pass after, and one whose task is
    /// over never does — which is why nothing here starts a session. Waking an
    /// agent is the lifecycle's business, and a message is not a reason to put
    /// one back on a task nobody is working on.
    async fn recipient_session(&self, message: &Message) -> Option<AgentSession> {
        let filter = match (message.to_actor()?, &message.task_id) {
            (Actor::Orchestrator, _) => SessionFilter {
                goal_id: Some(message.goal_id.clone()),
                live_only: true,
                ..Default::default()
            },
            (_, Some(task_id)) => SessionFilter {
                task_id: Some(task_id.clone()),
                live_only: true,
                ..Default::default()
            },
            _ => return None,
        };
        let live = self.store.list_sessions(filter).await.ok()?;
        live.into_iter().find(|s| match message.to_actor() {
            Some(Actor::Orchestrator) => s.seat() == Some(Seat::Orchestrator),
            // Addressed to one staffed agent, and to no other of the task:
            // the agents of two columns are two recipients.
            _ => s.task_agent_id.is_some() && s.task_agent_id == message.to_agent_id,
        })
    }

    /// The title of the task the sender's agent is staffed on, for the
    /// briefing to name: None where there is none to name, because the
    /// sender carries no agent id or no task.
    async fn sender_task_title(&self, message: &Message) -> Option<String> {
        let (Some(_), Some(task_id)) = (&message.from_agent_id, &message.task_id) else {
            return None;
        };
        self.store.get_task(task_id).await.ok().map(|t| t.title)
    }

    /// The skills of the sender's agent, in the order they reach it, for the
    /// briefing to name: none for a message with no agent of its own, such
    /// as the orchestrator's or the user's.
    async fn sender_skills(&self, message: &Message) -> Vec<String> {
        let Some(agent_id) = &message.from_agent_id else {
            return Vec::new();
        };
        self.store
            .agent_skills(agent_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.name)
            .collect()
    }
}
