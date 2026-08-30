//! The compaction every agent session is owed at a hand-off.
//!
//! A session is long-lived — one engineer per task, one reviewer per task
//! across its rounds, one planner per goal — and every resume of it replays
//! the whole transcript as the first prompt: `claude --resume`, `codex
//! resume`, `opencode --session` and the quiet watchdog's relaunch alike.
//! Nothing shortens that transcript but the CLI's own compaction, and the CLI
//! only runs it on its own near the context limit. So the daemon asks for
//! one at every hand-off, which is where the conversation so far has served
//! its purpose: a plan finalized, a review requested, a verdict given.
//!
//! The debt is written on the row (`compact_owed_at`, see
//! [`Store::owe_compaction`]) by the reconcile pass that sees the hand-off,
//! and paid here. Paying it means typing the CLI's `/compact` into the pane
//! — through the same confirmed delivery a nudge takes — which is only done
//! into a pane that is free: the turn ended, nothing being typed, no dialog
//! waiting on a person. Then the pane is left alone until the CLI says the
//! compaction is over, in its own vocabulary (see the adapters'
//! `compaction_done`), or until the wait for that runs out. Nothing is typed
//! into it and nothing kills it meanwhile: a resume, a nudge or a relaunch
//! that becomes due goes out on the pass after. A session is never held for
//! a compaction longer than that wait, nor for a debt that never got to run
//! ([`COMPACTION_OWED_FOR_SECS`]): whatever ends a compaction short of the
//! CLI's word for it is written off, and the work goes on as it would have.
//!
//! Each compaction is written to the session's event log as the daemon's
//! own `compaction` event, and one that ended any other way as a
//! `compaction_failed` naming why, so `ariadne events` and the session's
//! activity show them beside the CLI's own events.
//!
//! The pane is protected from the moment the command starts going in, not
//! from the moment the paste is confirmed: typing takes seconds — a paste,
//! an Enter, the pane read back — and a CLI with little to summarise can
//! report the compaction done inside them. The done signal pays the debt on
//! the row; the delivery report that follows finds it paid and stands down,
//! rather than opening a wait on a compaction that is already over.

use tokio::time::Instant;
use tracing::{debug, info, warn};

use ariadne_core::{AttentionReason, SessionStatus};
use ariadne_store::{AgentSession, NewAgentEvent, SessionFilter};

use crate::agents::adapter_for;

use super::COMPACTION_OWED_FOR_SECS;
use super::delivery::DeliveryOutcome;

/// Passes at typing the command that tmux may refuse before the debt is
/// written off: a tmux that will not take it says nothing about the agent,
/// so it is asked again on later passes — but not for ever.
const COMPACTION_ATTEMPTS: u32 = 5;

/// A compaction the daemon is typing, or has typed and is waiting on.
#[derive(Debug)]
pub(super) struct Compacting {
    /// When the command started going in, which is what the wait for the
    /// CLI's done signal is measured from.
    since: Instant,
    /// The debt it is paying: the row's `compact_owed_at` at the time. A
    /// debt paid and owed again in the meantime is a different one, and this
    /// record says nothing about it.
    owed_at: String,
    /// Whether the delivery has reported back. Until it has, the record is
    /// the delivery's to close: a done signal that lands meanwhile pays the
    /// debt on the row, and the report finds it paid.
    settled: bool,
}

/// What typing a compaction into a pane came to, reported back to the loop.
#[derive(Debug)]
pub(super) struct CompactionReport {
    pub(super) session_id: String,
    /// The debt the command was typed for, carried through so the record
    /// made of it names the right one.
    owed_at: String,
    command: String,
    outcome: DeliveryOutcome,
}

/// Why a session's pane is not the place for a compaction right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotNow {
    /// The agent is not at its prompt: a turn is running, or the session
    /// has not got going.
    Busy,
    /// A delivery is going into the composer.
    Typing,
    /// A dialog is up that only a person may answer.
    WaitingOnUser,
}

impl super::Scheduler {
    /// Note that `session` owes a compaction for `situation` — the status
    /// and round of the hand-off — once per situation: every later pass
    /// that sees the same hand-off adds nothing, and the moment the debt was
    /// first owed stands.
    pub(super) async fn owe_compaction(
        &mut self,
        session: &AgentSession,
        situation: (String, i64),
    ) {
        if !session.status().is_live() {
            return;
        }
        if self.compaction_owed_for.get(&session.id) == Some(&situation) {
            return;
        }
        self.compaction_owed_for
            .insert(session.id.clone(), situation.clone());
        match self.store.owe_compaction(&session.id).await {
            Ok(true) => {
                info!(session = %session.id, role = %session.role, status = %situation.0, round = situation.1, "the hand-off is made; the session owes a compaction of its conversation")
            }
            Ok(false) => {}
            Err(e) => {
                warn!(session = %session.id, error = %e, "noting the compaction the session owes failed")
            }
        }
    }

    /// Whether this session owes a compaction that has not been paid or
    /// written off, or has one going into its pane: what a kill that would
    /// take the conversation with it waits on.
    pub(super) fn compaction_pending(&self, session: &AgentSession) -> bool {
        session.compact_owed_at.is_some() || self.compaction_in_flight(session)
    }

    /// Whether a compaction is going into or running in this session's
    /// pane right now — being typed, or typed and not yet reported done or
    /// timed out. What a relaunch waits on, and what keeps every other
    /// delivery out of the pane.
    pub(super) fn compaction_in_flight(&self, session: &AgentSession) -> bool {
        self.compacting.contains_key(&session.id)
    }

    /// Whether anything is going into this pane: a delivery being typed, or
    /// a compaction running. Neither is a pane to type into or to kill.
    pub(super) fn pane_busy(&self, session_id: &str) -> bool {
        self.typing.contains(session_id) || self.compacting.contains_key(session_id)
    }

    /// Every session that owes a compaction, offered one — or written off.
    /// Runs on the tick; a session's own events reach
    /// [`Self::settle_compaction`] straight from `reconcile_session`.
    pub(super) async fn compaction_sweep(&mut self) {
        let owing = match self
            .store
            .list_sessions(SessionFilter {
                compaction_owed: true,
                ..Default::default()
            })
            .await
        {
            Ok(owing) => owing,
            Err(e) => {
                warn!(error = %e, "listing the sessions that owe a compaction failed");
                return;
            }
        };
        let mut seen = std::collections::HashSet::new();
        for session in owing {
            seen.insert(session.id.clone());
            self.settle_compaction(&session).await;
        }
        // And the records of sessions that no longer owe one — paid while
        // nothing else brought their row here — so no record outlives the
        // compaction it was about.
        let paid: Vec<String> = self
            .compacting
            .keys()
            .filter(|id| !seen.contains(*id))
            .cloned()
            .collect();
        for id in paid {
            match self.store.get_session(&id).await {
                Ok(session) => self.settle_compaction(&session).await,
                Err(_) => {
                    self.compacting.remove(&id);
                }
            }
        }
    }

    /// One pass over one session's compaction: what its row and its pane say
    /// about the debt, and what that calls for.
    pub(super) async fn settle_compaction(&mut self, session: &AgentSession) {
        let running = self
            .compacting
            .get(&session.id)
            .map(|r| (r.since.elapsed(), r.owed_at.clone(), r.settled));
        if let Some((waited, owed_for, settled)) = running {
            // Whatever the row says, a wait that has run out is over.
            if waited >= self.launcher.cfg.compaction_timeout {
                warn!(session = %session.id, role = %session.role, "the agent never reported its compaction done; giving up waiting for it");
                self.compacting.remove(&session.id);
                if session.compact_owed_at.as_deref() == Some(owed_for.as_str()) {
                    self.write_off_compaction(session, "timed_out").await;
                }
                return;
            }
            // A record the delivery has not reported back on is the
            // delivery's to close (`compaction_settled`): the pane stays
            // protected until the keystrokes have settled, whatever the row
            // says meanwhile.
            if !settled {
                return;
            }
        }
        let Some(owed_at) = session.compact_owed_at.clone() else {
            // Paid — the CLI said so through the ingestion path — or never
            // owed. Whatever was being waited on is over.
            if self.compacting.remove(&session.id).is_some() {
                info!(session = %session.id, role = %session.role, "the compaction is done; the pane is free again");
            }
            return;
        };
        if let Some(running) = self.compacting.get(&session.id) {
            if running.owed_at != owed_at {
                // The record is of a debt since paid; this is a new one.
                self.compacting.remove(&session.id);
            } else if !session.status().is_live() {
                warn!(session = %session.id, role = %session.role, "the session ended while its compaction ran; the debt stands for its next run");
                self.compacting.remove(&session.id);
                return;
            } else {
                return;
            }
        }
        if !session.status().is_live() {
            // Nothing to type into. The debt stands: a resume brings the
            // session back to its prompt, which is where it is paid.
            return;
        }
        if owed_for_too_long(&owed_at) {
            warn!(session = %session.id, role = %session.role, "the compaction it owes never got to run; writing it off");
            self.write_off_compaction(session, "never_started").await;
            return;
        }
        if let Err(why) = ready_for_compaction(session, self.typing.contains(&session.id)) {
            debug!(session = %session.id, ?why, "the session owes a compaction, but its pane is not free for one");
            return;
        }
        let Some(command) = adapter_for(session.agent_kind()).compaction_command(session.role())
        else {
            warn!(session = %session.id, agent = %session.agent_kind, "this agent CLI's compaction cannot be started from outside; the debt is written off");
            self.write_off_compaction(session, "unsupported").await;
            return;
        };
        if self
            .compaction_refused
            .get(&session.id)
            .is_some_and(|refused| *refused >= COMPACTION_ATTEMPTS)
        {
            warn!(session = %session.id, "tmux would not take the compaction command after every attempt; the debt is written off");
            self.compaction_refused.remove(&session.id);
            self.write_off_compaction(session, "refused").await;
            return;
        }
        info!(session = %session.id, role = %session.role, %command, "typing the compaction into the agent's pane");
        self.typing.insert(session.id.clone());
        // Protected from here: a CLI with little to summarise can report the
        // compaction done before the keystrokes have settled.
        self.compacting.insert(
            session.id.clone(),
            Compacting {
                since: Instant::now(),
                owed_at: owed_at.clone(),
                settled: false,
            },
        );
        let tmux = self.launcher.tmux.clone();
        let reports = self.compaction_reports.clone();
        let pane = session.tmux_session.clone();
        let session_id = session.id.clone();
        tokio::spawn(async move {
            let outcome = match tmux.send_submitted(&pane, &command).await {
                Ok(true) => DeliveryOutcome::Confirmed,
                Ok(false) => DeliveryOutcome::Unsubmitted,
                Err(e) => {
                    warn!(session = %session_id, error = %format!("{e:#}"), "typing the compaction into the agent's pane failed");
                    DeliveryOutcome::Refused
                }
            };
            let _ = reports.send(CompactionReport {
                session_id,
                owed_at,
                command,
                outcome,
            });
        });
    }

    /// The compaction command has settled in the pane, one way or another.
    pub(super) async fn compaction_settled(&mut self, report: CompactionReport) {
        self.typing.remove(&report.session_id);
        let Ok(session) = self.store.get_session(&report.session_id).await else {
            self.compacting.remove(&report.session_id);
            return;
        };
        // The debt the command was typed for is no longer on the row: the
        // CLI reported the compaction done while the keystrokes were still
        // settling — or the row was re-owed since. Either way this delivery
        // has nothing left to wait on, and whatever the pane read back says
        // about the composer is a reading of a compaction already over.
        if session.compact_owed_at.as_deref() != Some(&report.owed_at) {
            self.compacting.remove(&session.id);
            self.compaction_refused.remove(&session.id);
            info!(session = %session.id, role = %session.role, outcome = ?report.outcome, "the compaction was reported done before its delivery settled; the pane is free again");
            if report.outcome != DeliveryOutcome::Refused {
                self.record_compaction(
                    &session,
                    "compaction",
                    serde_json::json!({ "command": report.command }),
                )
                .await;
            }
            return;
        }
        match report.outcome {
            DeliveryOutcome::Confirmed => {
                info!(session = %session.id, role = %session.role, "the agent is compacting its conversation; nothing goes into its pane until it is done");
                self.compaction_refused.remove(&session.id);
                if let Some(running) = self.compacting.get_mut(&session.id) {
                    running.settled = true;
                }
                self.record_compaction(
                    &session,
                    "compaction",
                    serde_json::json!({ "command": report.command }),
                )
                .await;
            }
            DeliveryOutcome::Unsubmitted => {
                // Not tried again, for the reason a nudge is not: the
                // composer is holding the command, and a second paste would
                // leave it holding two. The user is pointed at the pane the
                // way they are for any delivery that stayed in it.
                warn!(session = %session.id, "the compaction command stayed in the agent's composer; flagging for user attention");
                self.compacting.remove(&session.id);
                self.write_off_compaction(&session, "not_submitted").await;
                if let Err(e) = self
                    .store
                    .set_session_attention(&session.id, AttentionReason::Stalled)
                    .await
                {
                    warn!(session = %session.id, error = %e, "flagging the session failed");
                }
            }
            DeliveryOutcome::Refused => {
                // Nothing was typed, so the debt stands and the next pass
                // tries again — for as many passes as a message is worth.
                self.compacting.remove(&session.id);
                *self
                    .compaction_refused
                    .entry(session.id.clone())
                    .or_insert(0) += 1;
            }
        }
    }

    /// Give up on the compaction a session owes, saying why in its log: the
    /// debt is cleared so that nothing waits on it any longer.
    async fn write_off_compaction(&mut self, session: &AgentSession, reason: &str) {
        match self.store.clear_compaction_owed(&session.id).await {
            Ok(_) => {}
            Err(e) => {
                warn!(session = %session.id, error = %e, "clearing the compaction the session owed failed")
            }
        }
        self.record_compaction(
            session,
            "compaction_failed",
            serde_json::json!({ "reason": reason }),
        )
        .await;
    }

    /// One line in the session's event log, beside the events its own
    /// hooks report.
    async fn record_compaction(
        &self,
        session: &AgentSession,
        kind: &str,
        payload: serde_json::Value,
    ) {
        if let Err(e) = self
            .store
            .create_event(NewAgentEvent {
                session_id: Some(session.id.clone()),
                task_id: session.task_id.clone(),
                agent_kind: Some(session.agent_kind()),
                kind: kind.into(),
                payload,
            })
            .await
        {
            warn!(session = %session.id, error = %e, "recording the compaction failed");
        }
    }
}

/// Whether a compaction may go into this session's pane now: the turn has
/// ended, nothing is being typed, and no dialog is waiting on a person.
///
/// The one decision a compaction turns on, asked of the row rather than the
/// pane: a running agent has an empty composer that the paste would land in
/// mid-turn, a dialog takes the Enter behind the paste for a yes, and a
/// delivery already going in would interleave with it.
fn ready_for_compaction(session: &AgentSession, typing: bool) -> Result<(), NotNow> {
    if session.status() != SessionStatus::Idle {
        return Err(NotNow::Busy);
    }
    if typing {
        return Err(NotNow::Typing);
    }
    if matches!(
        session.attention_reason(),
        Some(AttentionReason::WaitingPermission | AttentionReason::WaitingInput)
    ) {
        return Err(NotNow::WaitingOnUser);
    }
    Ok(())
}

/// Whether a debt dated `owed_at` has waited longer than
/// [`COMPACTION_OWED_FOR_SECS`] without being paid. A date that cannot be
/// read is a debt that can never be measured, and is treated as old.
fn owed_for_too_long(owed_at: &str) -> bool {
    chrono::DateTime::parse_from_rfc3339(owed_at)
        .ok()
        .is_none_or(|at| {
            chrono::Utc::now() - at.with_timezone(&chrono::Utc)
                >= chrono::Duration::seconds(COMPACTION_OWED_FOR_SECS)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(status: SessionStatus, attention: Option<AttentionReason>) -> AgentSession {
        AgentSession {
            id: "01session".into(),
            goal_id: "01goal".into(),
            task_id: Some("01task".into()),
            role: "engineer".into(),
            profile_id: "01profile".into(),
            agent_kind: "claude_code".into(),
            model: None,
            effort: None,
            internal_session_id: None,
            tmux_session: "ariadne-x".into(),
            worktree_path: None,
            review_round: None,
            status: status.as_str().into(),
            attention_reason: attention.map(|a| a.as_str().into()),
            attention_since: None,
            last_activity_at: None,
            launched_at: None,
            compact_owed_at: Some("2026-08-30T10:00:00.000Z".into()),
            created_at: "2026-08-30T09:00:00.000Z".into(),
            ended_at: None,
        }
    }

    /// The one pane a compaction goes into: at its prompt, with nothing
    /// going in and nobody being asked anything.
    #[test]
    fn a_compaction_goes_into_a_free_pane_and_no_other() {
        assert_eq!(
            ready_for_compaction(&session(SessionStatus::Idle, None), false),
            Ok(())
        );
        // Mid-turn, or not yet going: the composer would take the paste
        // into whatever the agent is doing.
        for status in [
            SessionStatus::Running,
            SessionStatus::Starting,
            SessionStatus::Exited,
            SessionStatus::Failed,
        ] {
            assert_eq!(
                ready_for_compaction(&session(status, None), false),
                Err(NotNow::Busy),
                "{status:?}"
            );
        }
        // A delivery going in.
        assert_eq!(
            ready_for_compaction(&session(SessionStatus::Idle, None), true),
            Err(NotNow::Typing)
        );
        // A dialog only a person may answer: the Enter behind the paste
        // would answer it.
        for reason in [
            AttentionReason::WaitingPermission,
            AttentionReason::WaitingInput,
        ] {
            assert_eq!(
                ready_for_compaction(&session(SessionStatus::Idle, Some(reason)), false),
                Err(NotNow::WaitingOnUser),
                "{reason:?}"
            );
        }
        // Flags that are not a dialog do not hold it up: a stalled agent at
        // its prompt is exactly the pane that can take one.
        for reason in [
            AttentionReason::Stalled,
            AttentionReason::WaitingUser,
            AttentionReason::AgentError,
        ] {
            assert_eq!(
                ready_for_compaction(&session(SessionStatus::Idle, Some(reason)), false),
                Ok(()),
                "{reason:?}"
            );
        }
    }

    /// A debt is measured from when it was first owed, and one nobody can
    /// date is not carried for ever either.
    #[test]
    fn a_debt_older_than_the_wait_is_too_old() {
        let recent = chrono::Utc::now().to_rfc3339();
        assert!(!owed_for_too_long(&recent));
        let old = (chrono::Utc::now() - chrono::Duration::seconds(COMPACTION_OWED_FOR_SECS + 1))
            .to_rfc3339();
        assert!(owed_for_too_long(&old));
        assert!(owed_for_too_long("not a date"));
    }
}
