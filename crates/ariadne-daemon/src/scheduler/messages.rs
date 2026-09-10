//! Carrying what one agent said to another into the pane it was said to.
//!
//! A message is written by the agent that sent it and delivered by the daemon:
//! typed into the recipient's composer and submitted, the way a nudge is. That
//! is the whole of the transport — there is no inbox an agent has to poll, and
//! nothing it has to be told to check, because the message arrives as a turn.
//!
//! A pane that is busy is not typed into, so a message to an agent mid-turn
//! waits for the pass after it finishes. `delivered_at` is what says which
//! have gone: it is stamped when tmux takes the text, and a message that is
//! still NULL is one still waiting for a pane.

use tracing::{debug, info, warn};

use ariadne_core::{Actor, MessageKind, PromptKind, Seat};
use ariadne_store::{AgentSession, Message, MessageFilter, SessionFilter};

use crate::agents::prompts;

impl super::Scheduler {
    /// Deliver everything still waiting for a pane on this task.
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
        let waiting = self
            .store
            .list_messages(MessageFilter {
                goal_id: Some(goal_id.to_string()),
                undelivered_only: true,
                ..Default::default()
            })
            .await;
        // A message about a task is delivered on that task's pass, so the
        // goal's own pass carries only what is not about one.
        let waiting = waiting
            .unwrap_or_default()
            .into_iter()
            .filter(|m| m.task_id.is_none())
            .collect();
        self.deliver(waiting).await;
    }

    /// One pass over a batch of undelivered messages.
    ///
    /// In the order they were written, and one per pane per pass: two pastes
    /// into one composer at once would interleave into something neither of
    /// them said, and `spawn_delivery` runs off the loop. What is left over
    /// goes on the next pass.
    async fn deliver(&mut self, waiting: Vec<Message>) {
        let mut typed: std::collections::HashSet<String> = std::collections::HashSet::new();
        for message in waiting {
            // A contested task's review request is not a message to type: the
            // summary alone names neither the author nor the branch, and it
            // would land while the reviewer's worktree still stands on the
            // review before it. The reviewer's full briefing is what carries
            // it (`rouse_reviewer_for`), and that briefing stamps it
            // delivered.
            if message.kind() == Some(MessageKind::ReviewRequest)
                && message.to_actor() == Some(Actor::Reviewer)
                && self.contested(message.task_id.as_deref()).await
            {
                debug!(message = %message.id, "a contested review request travels as the reviewer's briefing");
                continue;
            }
            let Some(session) = self.recipient_session(&message).await else {
                debug!(message = %message.id, "nothing live to deliver the message to yet");
                continue;
            };
            if self.pane_busy(&session.id) || !typed.insert(session.id.clone()) {
                continue;
            }
            let from = self.sender_name(&message).await;
            let template = prompts::template_for(PromptKind::IncomingMessage);
            let text = prompts::incoming_message_briefing(template, &message, &from);
            info!(
                message = %message.id,
                session = %session.id,
                kind = %message.kind,
                "delivering a message to the agent it is for"
            );
            // Stamped as the delivery goes out rather than when it comes back:
            // a message typed twice is worse than one the record calls
            // delivered a moment early, and what tmux made of it is the
            // delivery report's to say ([`Self::delivery_settled`]).
            if let Err(e) = self.store.mark_message_delivered(&message.id).await {
                warn!(message = %message.id, error = %e, "stamping the message failed");
                continue;
            }
            self.spawn_delivery(&session, text);
        }
    }

    /// Whether a message's task is contested: staffed with several authors,
    /// and no winner picked yet. Once the pick settles — and on every
    /// one-author task — the review flow is the lone author's, and its
    /// requests travel the channel like any other message.
    async fn contested(&self, task_id: Option<&str>) -> bool {
        let Some(task_id) = task_id else {
            return false;
        };
        let Ok(task) = self.store.get_task(task_id).await else {
            return false;
        };
        if task.picked_agent_id.is_some() {
            return false;
        }
        self.store
            .list_task_authors(task_id)
            .await
            .map(|authors| authors.len() > 1)
            .unwrap_or(false)
    }

    /// The live session a message is for, or None while there is none.
    ///
    /// A message outlives the session that will read it: an agent whose pane
    /// is being started again gets it on the pass after, and one whose task is
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
            Some(Actor::Orchestrator) => s.seat() == Seat::Orchestrator,
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
