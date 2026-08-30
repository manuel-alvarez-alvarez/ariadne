//! The two passes that see every session, whatever it belongs to.
//!
//! Both run on the tick rather than on an event, because what they are about
//! is state nothing reported: a pane that goes away says nothing, and a flag
//! left behind by a daemon that has since restarted has nobody to take it
//! down.

use tracing::{info, warn};

use ariadne_core::{AttentionReason, SessionStatus};
use ariadne_store::{AgentSession, SessionFilter};

use crate::attention;

use super::START_GRACE_SECS;

impl super::Scheduler {
    /// Mark sessions whose tmux process died as exited, and note the grid the
    /// living ones are drawing at.
    ///
    /// Measuring a pane answers both: `display-message` fails on a session
    /// that is not there. The size is written down because it is only
    /// knowable while the pane exists — a viewer opening the console log of a
    /// session that has ended has no other way to learn what width its bytes
    /// were written at (see `Launcher::record_pane_size`). This sweep is the
    /// only place that sees every session, watched or not.
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
            match self
                .launcher
                .tmux
                .pane_geometry(&session.tmux_session)
                .await
            {
                Ok(geometry) => {
                    alive += 1;
                    self.launcher
                        .record_pane_size(&session.id, geometry.cols, geometry.rows)
                        .await;
                }
                // Confirmed before acting on it, and only an answer counts as
                // confirmation: marking a session exited ends its work, which
                // is too much to hang on a line of tmux output that failed to
                // parse — or on a `has-session` that never ran. Both leave the
                // session alone for the next sweep to ask again.
                Err(e) => match self
                    .launcher
                    .tmux
                    .has_session_checked(&session.tmux_session)
                    .await
                {
                    // Except while it is still starting: a row goes into
                    // `starting` before tmux has anything under it, so a
                    // session on its way up has no pane yet and no more went
                    // wrong than that this sweep got there first. Counted as
                    // alive — a launch in flight is no reason to let the
                    // machine sleep — and asked again next time, by which
                    // point the window has either produced a pane or run out.
                    Ok(false) if starting_up(&session) => {
                        alive += 1;
                        info!(session = %session.id, tmux = %session.tmux_session, "no pane yet, but the session is still starting; left for the next sweep");
                    }
                    Ok(false) => {
                        info!(session = %session.id, tmux = %session.tmux_session, "session process gone, marking exited");
                        let _ = self
                            .store
                            .set_session_status(&session.id, SessionStatus::Exited)
                            .await;
                        // A pane that went away while its work is still going
                        // is not a session that finished: whatever was waiting
                        // on this agent is now waiting on nobody, so it is
                        // raised for the user. The flag outlives the session
                        // row's `exited` status on purpose — it stays up until
                        // the agent is resumed or replaced.
                        if attention::work_is_active(&self.store, &session).await {
                            warn!(session = %session.id, role = %session.role, "agent disconnected with work still active");
                            let _ = self
                                .store
                                .set_session_attention(&session.id, AttentionReason::Disconnected)
                                .await;
                        }
                    }
                    Ok(true) => {
                        alive += 1;
                        warn!(session = %session.id, error = %e, "measuring the pane failed")
                    }
                    // Unknown, so counted as alive: an unreachable tmux is no
                    // reason to let the machine sleep on a working agent.
                    Err(check) => {
                        alive += 1;
                        warn!(session = %session.id, error = %e, check = %check, "cannot reach tmux")
                    }
                },
            }
        }
        Some(alive)
    }

    /// Take down attention nobody can act on any more.
    ///
    /// A flag raised by an agent event is only ever taken down by another
    /// one, and a session sitting on a dialog emits nothing: an engineer
    /// blocked on a permission prompt whose task then goes under review would
    /// keep asking for the user for ever. Whatever put a flag up, it comes
    /// down once the work it was about stopped being this session's — the
    /// same question the sweep above asks before raising one.
    ///
    /// Two ways for that to be true, and a dead agent is the second: a prompt
    /// is a dialog on a pane, so a session that has ended cannot be waiting
    /// on an answer whatever its row still says. Retiring a session clears
    /// the flag as it goes (`set_session_status`); this is what heals the
    /// rows that were already stale when the daemon started, and it is not
    /// the same question as the one above — an exited planner of a goal still
    /// being planned is very much owed, which is what the sweep before this
    /// one raises as `disconnected`.
    ///
    /// One flag stands whatever the work did: a prompt on a live session
    /// that still owes a compaction. The dialog is on the screen whether or
    /// not anybody waits on the agent, and the daemon is about to type into
    /// that pane — the flag is what keeps it from typing into the dialog
    /// (see `compaction::ready_for_compaction`), and the user answering it
    /// is what lets the compaction run. It comes down with the debt: paid,
    /// or written off once it has waited too long.
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
            } else if prompt && session.status().is_live() && self.compaction_pending(&session) {
                continue;
            } else if !attention::work_is_active(&self.store, &session).await {
                "the work moved on"
            } else {
                continue;
            };
            info!(session = %session.id, role = %session.role, why, "dropping attention");
            let _ = self.store.clear_session_attention(&session.id).await;
        }
    }
}

/// Whether this session went into `starting` less than [`START_GRACE_SECS`]
/// ago: a launch that may not have reached tmux yet, rather than one that
/// never will.
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
