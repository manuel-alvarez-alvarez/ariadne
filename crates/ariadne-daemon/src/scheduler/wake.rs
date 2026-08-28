//! Who a message is taken to, and where the news of one goes.
//!
//! An addressee with no session to deliver to — a reviewer between rounds, an
//! engineer whose task has not started, one whose session went away — is not a
//! message lost: it keeps its place in the thread, and every briefing sends an
//! agent to read the conversation when it next starts. It is not a message
//! delivered either, though, so it stays on the retry list until there is a
//! session to hand it to — a pass that found nobody tried nothing, and costs
//! the message none of what it is worth.
//!
//! A message for the human wakes nobody. It goes up the attention path the UI
//! strip and `ariadne attention` already show, on the session of the agent
//! that wrote it — the session the user answers in, and the one place the
//! message can be traced back to — and only where somebody is still waiting
//! on that session: a task that has just ended says so to the user in its
//! thread, and its engineer is not an agent anybody is being asked about.

use tracing::{info, warn};

use ariadne_core::{AttentionReason, Role};
use ariadne_store::{AgentSession, Message, SessionFilter, Task};

use crate::agents::prompts;
use crate::notify;

/// What one pass at waking an addressee came to.
pub(super) enum Wake {
    /// Going into its pane now; the report says how that ended.
    InFlight,
    /// The agent has it: a resumed session comes back to it as its
    /// instruction.
    Delivered,
    /// Nothing to deliver: the agent is being woken for what it said itself,
    /// or it is sitting on a dialog nobody but the user may answer — and it
    /// is there to read the thread once it has been.
    Nothing,
    /// Its pane is busy with another delivery; a later tick tries again
    /// without spending an attempt on it.
    Busy,
    /// This pass could not, with the session to raise for the user once the
    /// attempts are gone — `None` when the addressee has no session at all,
    /// whether it has yet to have one or has lost the one it had, which is a
    /// pass that tried nothing and so spends none of them.
    Failed(Option<String>),
}

impl super::Scheduler {
    /// A message for an agent: typed into its pane if it has one, and
    /// otherwise resumed with the message as its instruction.
    ///
    /// An addressee with no session to deliver to — a reviewer between
    /// rounds, an engineer whose task has not started, one whose session went
    /// away — is not a message lost: it keeps its place in the thread, and
    /// the briefings send every agent to read the conversation when it next
    /// starts. It is not a message delivered either, though, so the later
    /// ticks go on asking until the session exists — nothing was typed, so
    /// nothing is spent, and what ends the asking is the thread itself being
    /// over rather than a handful of seconds of passes.
    ///
    /// What comes back is one of [`Wake`]; the caller keeps the count of what
    /// a message has spent, since only it knows how many passes have been
    /// made at this one.
    pub(super) async fn wake_profile(&mut self, message: &Message, profile_id: &str) -> anyhow::Result<Wake> {
        let Some(session) = self.recipient_session(message, profile_id).await? else {
            info!(message = %message.id, profile = %profile_id, "nobody to wake for this message yet; it waits in the thread");
            return Ok(Wake::Failed(None));
        };
        // An agent does not need waking for what it said itself.
        if message.author_session_id.as_deref() == Some(session.id.as_str()) {
            return Ok(Wake::Nothing);
        }
        let text = prompts::message_delivery(message);
        // Asked rather than assumed, the way the spawn guards ask: a tmux
        // that cannot be reached has said nothing about the pane, and an
        // agent relaunched on top of a live one is two agents on one task.
        match self
            .launcher
            .tmux
            .has_session_checked(&session.tmux_session)
            .await
        {
            Ok(true) => {}
            Ok(false) => {
                info!(message = %message.id, session = %session.id, role = %session.role, "resuming the addressed agent with the message");
                return match self.launcher.revive_session(&session.id, Some(&text)).await {
                    Ok(_) => Ok(Wake::Delivered),
                    // Nothing to resume from (no agent id yet, a working
                    // directory that is gone) is worth another pass, and then
                    // the user: a message whose agent cannot be brought back
                    // is one nobody will ever answer.
                    Err(e) => {
                        info!(message = %message.id, session = %session.id, error = %format!("{e:#}"), "the addressed agent could not be resumed");
                        Ok(Wake::Failed(Some(session.id)))
                    }
                };
            }
            Err(e) => {
                warn!(message = %message.id, session = %session.id, error = %format!("{e:#}"), "tmux cannot say whether the addressed agent still has a pane");
                return Ok(Wake::Failed(Some(session.id)));
            }
        }
        // An agent sitting on a dialog is not typed into: what a pane holding
        // a permission prompt does with the Enter behind a paste is answer it,
        // and that is the one decision the daemon must not make. The message
        // waits in the thread, where the agent reads it once the user has
        // dealt with the prompt.
        if matches!(
            session.attention_reason(),
            Some(AttentionReason::WaitingPermission | AttentionReason::WaitingInput)
        ) {
            info!(message = %message.id, session = %session.id, "the addressed agent is waiting on the user; the message waits in the thread");
            return Ok(Wake::Nothing);
        }
        if self.typing.contains(&session.id) {
            return Ok(Wake::Busy);
        }
        info!(message = %message.id, session = %session.id, role = %session.role, "nudging the addressed agent with the message");
        self.spawn_delivery(&session, text, Some(message.id.clone()));
        Ok(Wake::InFlight)
    }

    /// The session a message's addressee works in, the most recent one first.
    ///
    /// A goal-thread message looks at the goal's own sessions — the ones with
    /// no task — because that is where the planner runs, and the planner is
    /// the only agent a goal thread can address; filtering by goal alone
    /// would reach into its tasks.
    ///
    /// A task message looks in that task, and then — for the planner alone —
    /// at the goal's own sessions. Every task thread can address the planner
    /// that wrote it (see `http::recipients`), and the planner works at the
    /// goal level: filtering by task alone would find no session for it and
    /// wake nobody at all. Everyone else a task thread addresses works in
    /// that task, so a session of theirs outside it is somebody else's
    /// conversation and is not typed into for this one.
    async fn recipient_session(
        &self,
        message: &Message,
        profile_id: &str,
    ) -> anyhow::Result<Option<AgentSession>> {
        let sessions = self
            .store
            .list_sessions(SessionFilter {
                goal_id: Some(message.goal_id.clone()),
                ..Default::default()
            })
            .await?;
        let mut at_goal = None;
        for session in sessions.into_iter().rev() {
            if session.profile_id != profile_id {
                continue;
            }
            match &session.task_id {
                Some(_) if session.task_id == message.task_id => return Ok(Some(session)),
                None if at_goal.is_none() && session.role() == Role::Planner => {
                    at_goal = Some(session)
                }
                _ => {}
            }
        }
        // Which leaves the goal's own planner, reached from the task thread
        // that addressed it — and, for a goal thread, the only session it was
        // ever allowed to reach.
        Ok(at_goal)
    }

    /// A message for the human, which no agent is woken for: it goes up the
    /// attention path the UI strip and `ariadne attention` already show, on
    /// the session of the agent that wrote it — the session the user answers
    /// in, and the one place the message can be traced back to.
    ///
    /// This is the only place a message addressed to the user raises
    /// anything, whoever wrote it: what the daemon says to the user — a pull
    /// request opened, an approval, a task that ended — travels as a message
    /// like everything else and goes up here, rather than beside a
    /// `create_message` call with a flag of its own.
    ///
    /// Raising it asks the question the prompt path already asks
    /// ([`crate::attention::work_is_active`]): whether anybody is still
    /// waiting on the session it would land on. A task that has just been
    /// merged, failed or cancelled says so to the user in its thread, and its
    /// engineer is nobody's to answer — putting "waiting for you" on that
    /// session for the seconds until the sweep sees it is a flash on every
    /// ending. The notice keeps the user as its recipient either way; what is
    /// withheld is the flag.
    pub(super) async fn raise_for_user(&self, message: &Message) -> anyhow::Result<()> {
        let session = match &message.author_session_id {
            Some(session_id) => self.store.get_session(session_id).await?,
            // Written by the daemon rather than by an agent, so there is no
            // author's session to point at: the flag goes on the agent the
            // task is with, which is the row its attention is read from. A
            // task with nothing running, and a notice in a goal's thread,
            // raise nothing — the message waits where it was written.
            None => match self.session_the_task_is_with(message).await? {
                Some(session) => session,
                None => {
                    info!(message = %message.id, "message addressed to the user with no session to raise it on; it waits in the thread");
                    return Ok(());
                }
            },
        };
        if !crate::attention::work_is_active(&self.store, &session).await {
            info!(message = %message.id, session = %session.id, role = %session.role, "nobody is waiting on the session this would go up on; it waits in the thread");
            return Ok(());
        }
        info!(message = %message.id, session = %session.id, "message addressed to the user, raising it for them");
        self.store
            .set_session_attention(&session.id, AttentionReason::WaitingUser)
            .await?;
        Ok(())
    }

    /// The live session a task's own notices are raised on: its engineer, the
    /// most recent one it has.
    async fn session_the_task_is_with(
        &self,
        message: &Message,
    ) -> anyhow::Result<Option<AgentSession>> {
        let Some(task_id) = &message.task_id else {
            return Ok(None);
        };
        let sessions = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task_id.clone()),
                live_only: true,
                ..Default::default()
            })
            .await?;
        Ok(sessions
            .iter()
            .rev()
            .find(|s| s.role() == Role::Engineer)
            .cloned())
    }

    /// Tell the user a task the daemon itself ended is over, and deliver it
    /// the way the HTTP path delivers its own.
    ///
    /// Best effort on both halves: a notice that cannot be written is not a
    /// reason to leave the transition half-made, and the ending is in the
    /// task's status either way.
    pub(super) async fn announce_ending(&mut self, task: &Task, reason: Option<&str>) {
        match notify::task_ended(&self.store, task, reason).await {
            Ok(Some(message)) => self.deliver_message(&message.id).await,
            Ok(None) => {}
            Err(e) => {
                warn!(task = %task.id, error = %e, "telling the user the task ended failed")
            }
        }
    }
}
