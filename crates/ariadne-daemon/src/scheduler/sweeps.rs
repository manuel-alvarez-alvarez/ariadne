//! The two passes that see every session, whatever it belongs to.
//!
//! Both run on the tick rather than on an event, because what they are about
//! is state nothing reported: an agent that dies before it can say so, and a
//! flag left behind by a daemon that has since restarted has nobody to take
//! it down.

use tracing::{info, warn};

use ariadne_core::{AttentionReason, SessionStatus};
use ariadne_store::{AgentSession, SessionFilter};

use crate::attention;

use super::START_GRACE_SECS;

impl super::Scheduler {
    /// Mark sessions whose agent process died as exited.
    ///
    /// The runtime that owns each child process answers
    /// (`AcpRuntime::is_running`), and it answers for certain — there is no
    /// "could not be asked" to leave the row alone on. A session still
    /// starting is left alone: the row exists before the child does.
    ///
    /// Returns how many sessions came out of it still alive, or `None` if the
    /// store could not be listed.
    pub(super) async fn liveness_sweep(&mut self) -> Option<usize> {
        let Ok(live) = self
            .store
            .list_sessions(SessionFilter {
                live_only: true,
                ..Default::default()
            })
            .await
        else {
            return None;
        };
        let mut alive = 0;
        for session in live {
            if self.launcher.acp.is_running(&session.id) || starting_up(&session) {
                alive += 1;
            } else {
                info!(session = %session.id, "agent process gone, marking exited");
                self.retire_disconnected(&session).await;
            }
        }
        Some(alive)
    }

    /// Retire a session whose agent process went away without saying so:
    /// marked exited, and raised for the user where its work is still going.
    ///
    /// A process that went away mid-work is not a session that finished:
    /// whatever was waiting on this agent is now waiting on nobody, so it is
    /// raised for the user. The flag outlives the session row's `exited`
    /// status on purpose — it stays up until the agent is resumed or
    /// replaced.
    async fn retire_disconnected(&self, session: &AgentSession) {
        let _ = self
            .store
            .set_session_status(&session.id, SessionStatus::Exited)
            .await;
        if attention::work_is_active(&self.store, session).await {
            warn!(session = %session.id, seat = %session.seat, "agent disconnected with work still active");
            let _ = self
                .store
                .set_session_attention(&session.id, AttentionReason::Disconnected)
                .await;
        }
    }

    /// Take down attention nobody can act on any more.
    ///
    /// A flag raised by an agent event is only ever taken down by another
    /// one, and a session waiting on an answer emits nothing: an author
    /// blocked on a permission request whose task then goes under review
    /// would keep asking for the user for ever. Whatever put a flag up, it comes
    /// down once the work it was about stopped being this session's — the
    /// same question the sweep above asks before raising one.
    ///
    /// Two ways for that to be true, and a dead agent is the second: a prompt
    /// is a question to a live agent, so a session that has ended cannot be
    /// waiting on an answer whatever its row still says. Retiring a session clears
    /// the flag as it goes (`set_session_status`); this is what heals the
    /// rows that were already stale when the daemon started, and it is not
    /// the same question as the one above — an exited orchestrator of a goal
    /// still being planned is very much owed, which is what the sweep before
    /// this one raises as `disconnected`.
    pub(super) async fn stale_attention_sweep(&self) {
        let Ok(flagged) = self
            .store
            .list_sessions(SessionFilter {
                attention_only: true,
                ..Default::default()
            })
            .await
        else {
            return;
        };
        for session in flagged {
            let prompt = session.attention_reason().is_some_and(|r| r.is_prompt());
            let why = if !session.status().is_live() && prompt {
                "the session ended on a prompt nobody can answer"
            } else if !attention::work_is_active(&self.store, &session).await {
                "the work moved on"
            } else {
                continue;
            };
            info!(session = %session.id, seat = %session.seat, why, "dropping attention");
            let _ = self.store.clear_session_attention(&session.id).await;
        }
    }
}

/// Whether this session went into `starting` less than [`START_GRACE_SECS`]
/// ago: a launch whose agent may not be up yet, rather than one that never
/// will be.
///
/// The start is dated from the latest of the three columns that stamp one,
/// since which of them holds it depends on how the session got there:
/// `created_at` for a row `create_session` has just written and not yet
/// launched, `last_activity_at` for one `restart_session` put back on its feet
/// under its own id, and `launched_at` for the launch itself. A row none of
/// them dates recently has been starting for longer than the window, whatever
/// wrote it.
fn starting_up(session: &AgentSession) -> bool {
    if session.status() != SessionStatus::Starting {
        return false;
    }
    let stamped = |at: &str| {
        chrono::DateTime::parse_from_rfc3339(at)
            .ok()
            .map(|at| at.with_timezone(&chrono::Utc))
    };
    [
        stamped(&session.created_at),
        session.last_activity_at.as_deref().and_then(stamped),
        session.launched_at.as_deref().and_then(stamped),
    ]
    .into_iter()
    .flatten()
    .max()
    .is_some_and(|started| {
        chrono::Utc::now() - started < chrono::Duration::seconds(START_GRACE_SECS)
    })
}
