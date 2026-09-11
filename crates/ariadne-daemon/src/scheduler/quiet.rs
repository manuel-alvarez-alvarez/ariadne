//! The watchdog over an agent that stopped reporting.
//!
//! One clock — how long since the session was last heard from at all — and one
//! timeline on it: a nudge, then the user, then the agent killed and put back
//! on its feet. Only an agent between turns is nudged: one inside a turn is
//! left to the thresholds behind the nudge.

use tracing::{info, warn};

use ariadne_core::{Actor, AttentionReason, Seat, SessionStatus, TaskStatus};
use ariadne_store::AgentSession;

use super::{QUIET_FLAG_SECS, QUIET_NUDGE_SECS, QUIET_RELAUNCH_SECS, SPAWN_RETRY_BUDGET};

/// What the watchdog has already done about one session's silence.
///
/// One record per session. `situation` is what keeps the nudge to one per
/// situation rather than one per session for ever: an agent whose task moved
/// on — a new status, a new review round — has a fresh reason to get on with
/// it, and so has one that was just put back on its feet.
#[derive(Debug, Default)]
pub(super) struct Quiet {
    /// What the two steps below were taken in: the status of the task for an
    /// author or a reviewer, of the goal for an orchestrator — and, for a
    /// task under review, the review it is under, since two reviews of one
    /// task read as the same status and are not the same situation.
    pub(super) situation: String,
    /// Whether the one nudge for that situation has been spent.
    pub(super) nudged: bool,
    /// Whether the user has been told about it.
    pub(super) flagged: bool,
    /// Relaunches spent on this session, which no change of situation gives
    /// back. Bounded by [`SPAWN_RETRY_BUDGET`] like a task's spawn attempts
    /// are: an agent that goes quiet again after every relaunch is not one
    /// more relaunch away from working.
    pub(super) relaunches: u32,
}

impl super::Scheduler {
    /// One agent that has reported nothing for too long.
    ///
    /// One clock and one timeline, whatever the shape of the silence.
    /// [`Self::last_heard_from`] says when this session was last heard from at
    /// all, and three thresholds are read off it: a nudge at
    /// [`QUIET_NUDGE_SECS`], the user at [`QUIET_FLAG_SECS`], and at
    /// [`QUIET_RELAUNCH_SECS`] the agent killed and put back on its feet. Each
    /// of them is done once for the situation the agent is in, and a pass that
    /// arrives late does what the clock says now rather than going back for
    /// the steps it never had a chance to take.
    ///
    /// Only an idle agent is nudged. It finished a turn and stopped with the
    /// work still in front of it, so it is told to get on with it, in the
    /// words it would be started again with, as a `session/prompt`. A running
    /// one is inside a turn: it is left alone until the thresholds behind the
    /// nudge, which is where a turn that never ends is answered for.
    ///
    /// `situation` is what the nudge and the flag are spent on — what the
    /// agent went quiet in — so moving on earns fresh ones, and `resume` is
    /// both what it is nudged with and what it is revived with.
    pub(super) async fn check_session_quiet(
        &mut self,
        session: &AgentSession,
        situation: String,
        resume: &str,
    ) -> anyhow::Result<()> {
        if !matches!(
            session.status(),
            SessionStatus::Idle | SessionStatus::Running
        ) {
            return Ok(());
        }
        // An agent waiting on a person is blocked, not quiet: killing it would
        // throw away the question it is waiting on an answer to. An agent that
        // reported an error is already asking for the user by name, and
        // overwriting that with a stall would take away the more useful half
        // of what it said.
        if matches!(
            session.attention_reason(),
            Some(
                AttentionReason::WaitingPermission
                    | AttentionReason::WaitingInput
                    | AttentionReason::AgentError
            )
        ) {
            return Ok(());
        }
        let Some(since) = self.last_heard_from(session) else {
            return Ok(());
        };
        let quiet_secs = (chrono::Utc::now() - since).num_seconds();
        if quiet_secs < QUIET_NUDGE_SECS {
            return Ok(());
        }
        let done = self.quiet.entry(session.id.clone()).or_default();
        if done.situation != situation {
            done.situation = situation;
            done.nudged = false;
            done.flagged = false;
        }
        if quiet_secs >= QUIET_RELAUNCH_SECS {
            return self.relaunch_wedged(session, resume).await;
        }
        if quiet_secs >= QUIET_FLAG_SECS {
            // A flag raised for the user is left where it is: what
            // `waiting_user` says — a message written to them, a request that
            // is theirs to merge — is more use to them than "stalled", and it
            // is not the daemon's to take down on the agent's behalf. Nothing
            // is written down either, so a session that is still silent once
            // the user has had what they were owed is raised then. The silence
            // is measured all the same, and the relaunch above still happens.
            if session.attention_reason() == Some(AttentionReason::WaitingUser) {
                return Ok(());
            }
            if std::mem::replace(&mut done.flagged, true) {
                return Ok(());
            }
            warn!(session = %session.id, seat = %session.seat, quiet_secs, "the agent has reported nothing, flagging for user attention");
            self.store
                .set_session_attention(&session.id, AttentionReason::Stalled)
                .await?;
            return Ok(());
        }
        if done.nudged {
            return Ok(());
        }
        // A running agent is inside a turn and is left alone — the relaunch
        // threshold answers for a turn that never ends.
        if session.status() == SessionStatus::Running {
            return Ok(());
        }
        info!(session = %session.id, seat = %session.seat, quiet_secs, "nudging idle agent");
        // Spent as the prompt goes out. One the runtime would not take — no
        // agent runs for the session — is given back by `hand_prompt`, so
        // the next pass over this session sends it again.
        self.quiet.entry(session.id.clone()).or_default().nudged = true;
        self.hand_prompt(session, resume.to_string());
        Ok(())
    }

    /// The one clock: when this session was last heard from at all.
    ///
    /// Two things count, and the later of them is the answer. What the agent
    /// reported is the plain one — every event the runtime reports stamps
    /// `last_activity_at`, so an agent that is working keeps its own clock
    /// reset however slowly it works, and a wedged one is exactly the one that
    /// cannot. And the launch counts because a session that has reported
    /// nothing at all still has to be measured from something.
    ///
    /// A nudge that went in counts for neither: the whole point of the
    /// thresholds behind it is that an agent which was told to get on with the
    /// work and still says nothing is escalated, and a clock the nudge itself
    /// reset would never reach them.
    ///
    /// `None` when neither is known, which is a session nothing is concluded
    /// about.
    fn last_heard_from(&self, session: &AgentSession) -> Option<chrono::DateTime<chrono::Utc>> {
        let stamped = |at: &Option<String>| {
            at.as_deref()
                .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                .map(|at| at.with_timezone(&chrono::Utc))
        };
        [
            stamped(&session.last_activity_at),
            stamped(&session.launched_at),
        ]
        .into_iter()
        .flatten()
        .max()
    }

    /// Put a wedged agent back on its feet: the agent killed, and the same
    /// session row relaunched on the agent conversation it was already having.
    ///
    /// The relaunch is spent out of a budget for the same reason a spawn is:
    /// an agent that goes quiet, is put back and goes quiet again is not one
    /// more relaunch away from working, so [`SPAWN_RETRY_BUDGET`] of them is
    /// what there is and a task whose agent will not run is failed rather
    /// than restarted for ever. An orchestrator has no task to fail — its own
    /// flag is what is left, and it stands.
    ///
    /// An author is started the way a task with no live author is started —
    /// [`Self::start_author`], which renders what it is picked up with, the
    /// landing briefing included where the task is approved. An orchestrator
    /// and a reviewer have no such path: they are revived with the resume the
    /// caller already rendered, through the same `revive_session` a message
    /// addressed to a dead agent takes.
    ///
    /// What the user is owed outlives the relaunch: whatever this session was
    /// carrying for them goes back on whatever came back up
    /// ([`Self::keep_waiting_user`]), since a relaunch is not the user having
    /// merged the request or read the message.
    async fn relaunch_wedged(
        &mut self,
        session: &AgentSession,
        revival: &str,
    ) -> anyhow::Result<()> {
        let done = self.quiet.entry(session.id.clone()).or_default();
        done.relaunches += 1;
        // A relaunched agent is a fresh instruction that may be stuck in its
        // own right, so the steps taken for the situation come back with the
        // launch: the clock starts again, and so does the timeline on it.
        done.nudged = false;
        done.flagged = false;
        let spent = done.relaunches;
        if spent >= SPAWN_RETRY_BUDGET {
            warn!(session = %session.id, seat = %session.seat, relaunches = spent - 1, "the agent went quiet again after every relaunch");
            let Some(task_id) = session.task_id.clone() else {
                return Ok(());
            };
            if let Err(e) = self.launcher.kill_session(&session.id).await {
                warn!(session = %session.id, error = %e, "killing the wedged session failed");
            }
            // The task carries why it ended: its status and the reason on it
            // are what the user reads.
            let _ = self
                .store
                .transition_task(
                    &task_id,
                    TaskStatus::Failed,
                    Actor::Daemon,
                    Some("its agent stopped mid-turn after every relaunch"),
                    None,
                )
                .await;
            return Ok(());
        }
        info!(session = %session.id, seat = %session.seat, relaunch = spent, "the agent has reported nothing for too long, relaunching it");
        let carried = session.attention_reason();
        self.launcher.kill_session(&session.id).await?;
        if let Some(task_id) = session.task_id.clone()
            && session.seat() == Seat::Author
        {
            let task = self.store.get_task(&task_id).await?;
            self.start_author(&task).await?;
        } else {
            self.launcher
                .revive_session(&session.id, Some(revival))
                .await?;
        }
        let back = self.relaunched_session(session).await;
        self.keep_waiting_user(&back, carried).await
    }

    /// The session the agent came back as: the same row wherever it was
    /// resumed, and the one the seat is live in when the relaunch had to
    /// spawn afresh instead. The row that was killed is the answer of last
    /// resort — a relaunch that left nothing running is not a row to lose the
    /// flag over.
    async fn relaunched_session(&self, session: &AgentSession) -> AgentSession {
        self.live_sessions(&session.goal_id, session.task_id.as_deref(), session.seat())
            .await
            .ok()
            .and_then(|mut live| live.pop())
            .unwrap_or_else(|| session.clone())
    }
}
