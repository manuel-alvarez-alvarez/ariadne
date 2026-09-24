//! Carrying what one agent said to another to the agent it was said to.
//!
//! A message is written by the agent that sent it and delivered by the daemon:
//! handed to the recipient's agent as a `session/prompt`, the way a nudge is.
//! That is the whole of the transport — there is no inbox an agent has to
//! poll, and nothing it has to be told to check, because the message arrives
//! as a turn.
//!
//! A message to an agent mid-turn is queued behind the turn by the runtime.
//! `delivered_at` is what says which have gone: it is stamped when the
//! runtime takes the text, and a message that is still NULL is one still
//! waiting for a live agent to take it.

use tracing::{debug, info, warn};

use ariadne_core::{Actor, MessageKind, PromptKind, Seat};
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
        self.deliver(
            waiting.unwrap_or_default(),
            self.briefs_its_author(task_id).await,
        )
        .await;
    }

    /// Whether this task answers a change request with a briefing of its own.
    ///
    /// A task with one author does: the `changes_requested` arm resumes that
    /// author with the feedback of every reviewer that asked, and stamps
    /// those verdicts. A contested task does not — each of its authors is
    /// nudged on its own branch while the reviews run side by side — so there
    /// the channel is what carries a change request.
    async fn briefs_its_author(&self, task_id: &str) -> bool {
        let Ok(task) = self.store.get_task(task_id).await else {
            return false;
        };
        let Ok(authors) = self.store.list_task_authors(task_id).await else {
            return false;
        };
        authors.len() == 1 || task.picked_agent_id.is_some()
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
        // Nothing on a goal's channel is a verdict, so nothing there is
        // briefed anywhere else.
        self.deliver(waiting.unwrap_or_default(), false).await;
    }

    /// One pass over a batch of undelivered messages, in the order they were
    /// written: the runtime queues each prompt behind the one before it.
    ///
    /// `briefed_author` says that a change request in this batch travels as
    /// the author's own briefing, so it is not handed on here as well.
    async fn deliver(&mut self, waiting: Vec<Message>, briefed_author: bool) {
        for message in waiting {
            // A review request is not a message to hand on: the reviewer's
            // briefing is its delivery, and that briefing stamps it delivered.
            if message.kind() == Some(MessageKind::ReviewRequest)
                && message.to_actor() == Some(Actor::Reviewer)
            {
                debug!(message = %message.id, "a review request travels as the reviewer's briefing");
                continue;
            }
            // And a change request is not one either: the author is resumed
            // with the feedback of every reviewer that asked, in one briefing
            // that stamps them. Handed on here as well, each one would reach
            // the author twice.
            if briefed_author
                && message.kind() == Some(MessageKind::RequestChanges)
                && message.to_actor() == Some(Actor::Author)
            {
                debug!(message = %message.id, "a change request travels as the author's briefing");
                continue;
            }
            let Some(session) = self.recipient_session(&message).await else {
                debug!(message = %message.id, "nothing live to deliver the message to yet");
                continue;
            };
            let from = self.sender_name(&message).await;
            let template = prompts::template_for(PromptKind::IncomingMessage);
            let text = prompts::incoming_message_briefing(template, &message, &from);
            info!(
                message = %message.id,
                session = %session.id,
                kind = %message.kind,
                "delivering a message to the agent it is for"
            );
            // Stamped once the runtime took it: one it refused — the agent
            // went away between the lookup and the hand-off — waits for the
            // next pass.
            if !self.hand_prompt(&session, text) {
                continue;
            }
            if let Err(e) = self.store.mark_message_delivered(&message.id).await {
                warn!(message = %message.id, error = %e, "stamping the message failed");
            }
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
            // Addressed to one staffed agent, and to no other in its seat:
            // two reviewers on a task are two recipients.
            _ => s.task_agent_id.is_some() && s.task_agent_id == message.to_agent_id,
        })
    }

    /// How the sender is named to the agent that reads it.
    ///
    /// An agent has no name of its own, so it is named by its seat and the
    /// skills it works with — "reviewer (code-review)" — which is the only
    /// thing about it that means anything to the reader. The orchestrator and
    /// the user are named by what they are.
    async fn sender_name(&self, message: &Message) -> String {
        let seat = message
            .from_actor()
            .map_or("agent", |actor| actor.as_str())
            .to_string();
        let Some(agent_id) = &message.from_agent_id else {
            return seat;
        };
        let skills = self.store.agent_skills(agent_id).await.unwrap_or_default();
        match skills.is_empty() {
            true => format!("{seat} {agent_id}"),
            false => format!(
                "{seat} ({})",
                skills
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}
