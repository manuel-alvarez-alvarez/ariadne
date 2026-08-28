//! Taking a posted message to whoever it addresses.
//!
//! A conversation an agent only reads when it next happens to look is a
//! conversation nobody can hold. What is never spent is the attempt itself: a
//! message struck off before it was typed is one nobody receives and nobody is
//! told about, so the only things that end a delivery are a confirmation and
//! running out of [`DELIVERY_ATTEMPTS`] at a session that is there to be typed
//! into.
//!
//! An addressee with no session at all — a reviewer before its round starts,
//! an engineer whose task has not begun — is not a pass at anything: nothing
//! was tried, so nothing is spent, and the message waits on the retry list
//! until there is somebody to hand it to. What keeps that list finite is the
//! conversation it was said in: once the message's task is merged or
//! cancelled — or, for a goal thread, the goal itself — nobody will ever be
//! started to read what is still owed, and the next pass strikes it off the
//! list. Only what is owed: a message posted into a thread that has just
//! ended is carried to whoever is there to read it, since the engineer of a
//! merged task is still at its pane.
//!
//! Typing happens off the loop. `send_submitted` is slow by design — a paste,
//! an Enter, and the pane read back on a widening backoff — so a pass with
//! three agents to nudge waits on none of them, and what came of each one
//! arrives back here as a [`DeliveryReport`].

use tracing::{info, warn};

use ariadne_core::AttentionReason;
use ariadne_store::{AgentSession, Message, Recipient};

use super::DELIVERY_ATTEMPTS;
use super::wake::Wake;

/// What one keystroke delivery came to, reported back to the loop that asked
/// for it.
///
/// Typing into a pane takes seconds — a paste, an Enter, and the pane read
/// back to see whether the composer let go of it — so it happens in a task of
/// its own and the loop hears about it here. A tick that has three agents to
/// nudge waits on none of them.
#[derive(Debug)]
pub(super) struct DeliveryReport {
    /// The message it carried, or `None` for a stall nudge, which is no
    /// message and nobody's to retry.
    pub(super) message_id: Option<String>,
    /// The session whose pane it went into.
    pub(super) session_id: String,
    pub(super) outcome: DeliveryOutcome,
}

/// How a delivery ended: exactly one of confirmed, worth another pass, or
/// given up on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DeliveryOutcome {
    /// The composer let go of it: the agent has it.
    Confirmed,
    /// Typed in and never submitted — the pane is there, and whoever is in
    /// front of it is not listening.
    Unsubmitted,
    /// tmux would not take it at all, which says nothing about the agent.
    Refused,
}

impl super::Scheduler {
    /// Take a posted message to whoever it addresses.
    ///
    /// A conversation an agent only reads when it next happens to look is a
    /// conversation nobody can hold: a message with a recipient is carried to
    /// that recipient, and one addressed to the thread wakes nobody, exactly
    /// as every message did before recipients existed.
    pub(super) async fn deliver_message(&mut self, message_id: &str) {
        if let Err(e) = self.deliver(message_id).await {
            warn!(message = %message_id, error = %format!("{e:#}"), "delivering the message failed");
        }
    }

    /// One pass at an addressed message, which ends in exactly one of three
    /// places: the agent has it, another tick tries again, or the user is
    /// told nobody ever got it.
    ///
    /// What is never spent is the attempt itself. A message struck off before
    /// it was typed — because that is when the daemon happened to look — is a
    /// message nobody ever receives and nobody is told about, so the only
    /// things that stop a message being tried again are a confirmation,
    /// running out of [`DELIVERY_ATTEMPTS`] at a session that exists, and the
    /// conversation it was said in ending.
    async fn deliver(&mut self, message_id: &str) -> anyhow::Result<()> {
        if self.delivered.contains(message_id) || self.given_up_on(message_id) {
            return Ok(());
        }
        let message = self.store.get_message(message_id).await?;
        let Some(recipient) = message.recipient() else {
            return Ok(());
        };
        match recipient {
            Recipient::User => {
                self.raise_for_user(&message).await?;
                self.delivered.insert(message.id.clone());
                Ok(())
            }
            Recipient::Profile(profile_id) => {
                // A message already waiting for somebody who never came, in a
                // conversation that is over, is struck off: the reviewer of a
                // cancelled task is never started, and the list the tick
                // works through would carry it for as long as the daemon
                // runs. Only one that is owed, mind — a message posted into a
                // thread that has just ended is still carried to whoever is
                // there to read it, the way the engineer of a merged task is
                // still at its pane. It keeps its place in the thread either
                // way.
                if self.attempts.contains_key(&message.id) && self.thread_is_over(&message).await? {
                    self.attempts.remove(&message.id);
                    info!(message = %message.id, "nobody ever came for this message and its conversation is over; it is off the retry list");
                    return Ok(());
                }
                match self.wake_profile(&message, &profile_id).await? {
                    // The report it comes back with says what became of it.
                    Wake::InFlight => Ok(()),
                    Wake::Delivered => {
                        self.attempts.remove(&message.id);
                        self.delivered.insert(message.id.clone());
                        Ok(())
                    }
                    Wake::Nothing => {
                        self.attempts.remove(&message.id);
                        Ok(())
                    }
                    // Nothing was tried, so nothing was spent: the message
                    // stays on the list the tick works through.
                    Wake::Busy => {
                        self.attempts.entry(message.id.clone()).or_insert(0);
                        Ok(())
                    }
                    // A pass that found no session at all tried nothing, so
                    // it spends nothing: the message keeps its place on the
                    // list and the later ticks ask again, until the addressee
                    // has a session or its thread is over. Only a pass at a
                    // session that is there — a tmux out of reach, a resume
                    // that failed — is one of the attempts.
                    Wake::Failed(None) => {
                        self.attempts.entry(message.id.clone()).or_insert(0);
                        Ok(())
                    }
                    Wake::Failed(Some(session_id)) => {
                        self.delivery_failed(&message.id, &session_id).await
                    }
                }
            }
        }
    }

    /// Whether this message has spent everything it was worth and the user
    /// has been told: nothing is typed for it again.
    fn given_up_on(&self, message_id: &str) -> bool {
        self.attempts
            .get(message_id)
            .is_some_and(|spent| *spent >= DELIVERY_ATTEMPTS)
    }

    /// Whether the conversation this message was said in is over: its task
    /// merged or cancelled, or — for a goal thread, which has no task — the
    /// goal itself.
    ///
    /// This is what keeps the retry list finite. A message whose addressee
    /// has no session is tried again for as long as its thread is live, which
    /// is exactly as long as somebody might still turn up to be typed into: a
    /// reviewer that was never spawned because the task was cancelled is
    /// never coming, and nothing is owed for the message that waited on it.
    /// It is asked of what a pass already owes, not of a message just posted,
    /// which goes to whoever is at their pane whatever the thread's status.
    async fn thread_is_over(&self, message: &Message) -> anyhow::Result<bool> {
        if let Some(task_id) = &message.task_id {
            return Ok(self.store.get_task(task_id).await?.status().is_terminal());
        }
        Ok(self
            .store
            .get_goal(&message.goal_id)
            .await?
            .status()
            .is_terminal())
    }

    /// Every message still owed a delivery, offered another pass.
    ///
    /// This is what makes "tried again on a later tick" true: a tmux that
    /// would not take a message has said nothing about whether the agent is
    /// there to hear it, so the message waits here and every tick asks again
    /// until it goes through or the attempts run out.
    pub(super) async fn retry_deliveries(&mut self) {
        let owed: Vec<String> = self
            .attempts
            .iter()
            .filter(|(_, spent)| **spent < DELIVERY_ATTEMPTS)
            .map(|(id, _)| id.clone())
            .collect();
        for message_id in owed {
            self.deliver_message(&message_id).await;
        }
    }

    /// Type `text` into a session's pane in a task of its own, which reports
    /// back what came of it.
    ///
    /// Off the loop because [`TmuxManager::send_submitted`] is slow by
    /// design: it lets a paste settle, presses Enter, reads the pane back and
    /// tries again on a widening backoff — seconds of waiting that the
    /// scheduler used to do inline, one agent at a time, while every other
    /// event queued behind it.
    pub(super) fn spawn_delivery(&mut self, session: &AgentSession, text: String, message_id: Option<String>) {
        self.typing.insert(session.id.clone());
        let tmux = self.launcher.tmux.clone();
        let reports = self.reports.clone();
        let pane = session.tmux_session.clone();
        let session_id = session.id.clone();
        tokio::spawn(async move {
            let outcome = match tmux.send_submitted(&pane, &text).await {
                Ok(true) => DeliveryOutcome::Confirmed,
                Ok(false) => DeliveryOutcome::Unsubmitted,
                Err(e) => {
                    warn!(session = %session_id, error = %format!("{e:#}"), "typing into the agent's pane failed");
                    DeliveryOutcome::Refused
                }
            };
            let _ = reports.send(DeliveryReport {
                message_id,
                session_id,
                outcome,
            });
        });
    }

    /// A delivery has come back: the one place that may call a message
    /// arrived, and the one that decides what an agent that never heard it
    /// costs.
    pub(super) async fn delivery_settled(&mut self, report: DeliveryReport) {
        self.typing.remove(&report.session_id);
        match report.outcome {
            DeliveryOutcome::Confirmed => {
                // Only a message. A nudge that went in is a nudge spent —
                // what follows one nobody acts on is the user, and a nudge
                // that gave itself back would ask for ever and tell nobody.
                if let Some(message_id) = &report.message_id {
                    info!(message = %message_id, session = %report.session_id, "the addressed agent has the message");
                    self.attempts.remove(message_id);
                    self.delivered.insert(message_id.clone());
                    // This agent has just been told what to do: the quiet
                    // clock starts again here and the nudge that may have
                    // been spent comes back, so that nothing tells it to get
                    // on with what it was asked to do a moment ago.
                    if let Some(done) = self.quiet.get_mut(&report.session_id) {
                        done.nudged = false;
                    }
                    self.delivered_at
                        .insert(report.session_id.clone(), chrono::Utc::now());
                }
            }
            DeliveryOutcome::Unsubmitted => {
                warn!(session = %report.session_id, message = ?report.message_id, "what was typed stayed in the agent's composer, flagging for user attention");
                // Not tried again: a pane that would not submit it this pass
                // will not submit it the next, and a second paste would leave
                // the composer holding the same thing twice.
                if let Some(message_id) = report.message_id {
                    self.attempts.insert(message_id, DELIVERY_ATTEMPTS);
                }
                if let Err(e) = self
                    .store
                    .set_session_attention(&report.session_id, AttentionReason::Stalled)
                    .await
                {
                    warn!(session = %report.session_id, error = %e, "flagging the session failed");
                }
            }
            DeliveryOutcome::Refused => match report.message_id {
                Some(message_id) => {
                    if let Err(e) = self.delivery_failed(&message_id, &report.session_id).await {
                        warn!(message = %message_id, error = %format!("{e:#}"), "giving up on the message failed");
                    }
                }
                // Nothing was typed, so the nudge is unspent rather than
                // lost: the next pass over this session sends it again.
                None => {
                    if let Some(done) = self.quiet.get_mut(&report.session_id) {
                        done.nudged = false;
                    }
                }
            },
        }
        // The pane is free again, so whatever was waiting for it goes in now
        // rather than at the next tick — unless tmux is the thing that
        // refused, in which case asking it again this second only spends the
        // attempts of everything queued behind it.
        if report.outcome != DeliveryOutcome::Refused {
            self.retry_deliveries().await;
        }
    }

    /// One pass that could not deliver: another tick tries again, and once
    /// the passes are gone the user is told rather than the message being
    /// left with nobody.
    async fn delivery_failed(&mut self, message_id: &str, session_id: &str) -> anyhow::Result<()> {
        let spent = {
            let spent = self.attempts.entry(message_id.to_string()).or_insert(0);
            *spent += 1;
            *spent
        };
        if spent < DELIVERY_ATTEMPTS {
            info!(message = %message_id, spent, "the message did not reach its agent; trying again on a later tick");
            return Ok(());
        }
        let message = self.store.get_message(message_id).await?;
        self.give_up(&message, session_id).await
    }

    /// A message that will not be delivered, put where the user will see it:
    /// on the addressee's session — stalled while its pane is still there,
    /// disconnected once it is gone — and, when that session's row has gone
    /// from under the daemon between the passes, on the session of whoever
    /// wrote it, which is the pane they are watching for an answer. The
    /// message itself stays in the thread either way; what is raised is that
    /// nobody came for it.
    ///
    /// The author is raised as the user's to deal with rather than as an
    /// agent waiting on an answer: it asked nothing of the human, and
    /// `waiting_input` would leave it unreachable — the quiet watchdog skips
    /// a session waiting on the user and nothing is typed into one either,
    /// so the author would sit there with nobody able to reach it.
    async fn give_up(&self, message: &Message, session_id: &str) -> anyhow::Result<()> {
        let Ok(session) = self.store.get_session(session_id).await else {
            let Some(author) = &message.author_session_id else {
                warn!(message = %message.id, "the message reached nobody, and there is nobody to tell");
                return Ok(());
            };
            warn!(message = %message.id, session = %author, "the message reached nobody; raising its author for the user");
            self.store
                .set_session_attention(author, AttentionReason::WaitingUser)
                .await?;
            return Ok(());
        };
        let reason = match self
            .launcher
            .tmux
            .has_session_checked(&session.tmux_session)
            .await
        {
            Ok(false) => AttentionReason::Disconnected,
            _ => AttentionReason::Stalled,
        };
        warn!(message = %message.id, session = %session.id, reason = reason.as_str(), "the message never reached the agent, flagging for user attention");
        self.store
            .set_session_attention(&session.id, reason)
            .await?;
        Ok(())
    }
}
