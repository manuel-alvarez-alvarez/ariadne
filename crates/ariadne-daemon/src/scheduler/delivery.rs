//! Typing into an agent's pane, off the loop.
//!
//! `send_submitted` is slow by design — a paste, an Enter, and the pane read
//! back on a widening backoff — so a pass with three agents to nudge waits on
//! none of them: the typing happens in a task of its own and what came of each
//! one arrives back here as a [`DeliveryReport`].

use tracing::{info, warn};

use ariadne_core::AttentionReason;
use ariadne_store::AgentSession;

/// What one keystroke delivery came to, reported back to the loop that asked
/// for it.
#[derive(Debug)]
pub(super) struct DeliveryReport {
    /// The session whose pane it went into.
    pub(super) session_id: String,
    pub(super) outcome: DeliveryOutcome,
}

/// How a delivery ended: exactly one of confirmed, left in the composer, or
/// refused by tmux.
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
    /// Type `text` into a session's pane in a task of its own, which reports
    /// back what came of it.
    ///
    /// Off the loop because [`TmuxManager::send_submitted`] is slow by
    /// design: it lets a paste settle, presses Enter, reads the pane back and
    /// tries again on a widening backoff — seconds of waiting that the
    /// scheduler used to do inline, one agent at a time, while every other
    /// event queued behind it.
    pub(super) fn spawn_delivery(&mut self, session: &AgentSession, text: String) {
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
                session_id,
                outcome,
            });
        });
    }

    /// A delivery has come back: the one place that decides what an agent that
    /// never heard its nudge costs.
    pub(super) async fn delivery_settled(&mut self, report: DeliveryReport) {
        self.typing.remove(&report.session_id);
        match report.outcome {
            // A nudge that went in is a nudge spent — what follows one nobody
            // acts on is the user, and a nudge that gave itself back would ask
            // for ever and tell nobody.
            DeliveryOutcome::Confirmed => {
                info!(session = %report.session_id, "the agent took the nudge");
            }
            DeliveryOutcome::Unsubmitted => {
                warn!(session = %report.session_id, "what was typed stayed in the agent's composer, flagging for user attention");
                if let Err(e) = self
                    .store
                    .set_session_attention(&report.session_id, AttentionReason::Stalled)
                    .await
                {
                    warn!(session = %report.session_id, error = %e, "flagging the session failed");
                }
            }
            // Nothing was typed, so the nudge is unspent rather than lost: the
            // next pass over this session sends it again.
            DeliveryOutcome::Refused => {
                if let Some(done) = self.quiet.get_mut(&report.session_id) {
                    done.nudged = false;
                }
            }
        }
    }
}
