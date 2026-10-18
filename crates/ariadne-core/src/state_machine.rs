//! Task state machine: statuses, actors, and the transition table.
//!
//! This is the single authority on which transitions are legal and who may
//! trigger them. The store validates every status change against it inside
//! the same transaction that records the audit row.
//!
//! The actors are *seats*, not identities: an agent is generic, and what it
//! knows how to do comes from the skills it loads. A seat says only where an
//! agent sits — the orchestrator of a goal, or the agent of one column of a
//! task's workflow — which is the whole of what this table needs to know
//! about it. The columns themselves move with [`check_step_move`]: a task
//! stays `in_progress` from its first column to its last, and the status
//! moves only at the two ends.

use serde::{Deserialize, Serialize};

use crate::wire_enum;

/// Task lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(
    feature = "clap",
    derive(clap::ValueEnum),
    value(rename_all = "kebab-case")
)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Created by the orchestrator; waiting for its dependencies to finish.
    Pending,
    /// Every dependency has finished; waiting for the first column's agent.
    Ready,
    /// A column's agent is working in the task's worktree. The task stays
    /// here from its first column to its last.
    InProgress,
    /// The last column completed: the work is done and whatever it produced
    /// is where it belongs. Terminal.
    Finished,
    /// Cancelled by the user or the orchestrator. Terminal.
    Cancelled,
    /// Failed by an agent or by the daemon. Retryable by the user or the
    /// orchestrator, which starts it again at the first column.
    Failed,
}

/// Who is attempting a transition: a seat, the daemon, or the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Orchestrator,
    Agent,
    Daemon,
    User,
}

/// Refused transition. The `Display` spelling names the Rust variants and is
/// for logs; anything a person reads goes through [`TransitionError::human`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    #[error("illegal transition {from:?} -> {to:?}")]
    IllegalTransition { from: TaskStatus, to: TaskStatus },
    #[error("actor {actor:?} may not perform transition {from:?} -> {to:?}")]
    Forbidden {
        from: TaskStatus,
        to: TaskStatus,
        actor: Actor,
    },
}

impl TransitionError {
    /// One line a person can act on, in the API's snake_case status
    /// vocabulary. This is what the daemon puts in the error envelope.
    pub fn human(&self) -> String {
        match *self {
            Self::IllegalTransition { from, to } => explain(from, to, None),
            Self::Forbidden { from, to, actor } => explain(from, to, Some(actor)),
        }
    }
}

/// Explain a refused `from -> to` (by `actor`, when one was at fault).
///
/// The transitions a user can ask for by name — cancel, retry — get their own
/// wording, because "cannot move a pending task to ready" says nothing about
/// the `ariadne task retry` that provoked it. Everything else falls back to
/// naming the move.
fn explain(from: TaskStatus, to: TaskStatus, actor: Option<Actor>) -> String {
    use TaskStatus as S;
    let (f, t) = (from.as_str(), to.as_str());
    // A no-op is a misunderstanding about where the task already is, not a
    // state machine violation, and reads best said that way.
    if from == to {
        return format!("task is already {f}");
    }
    match to {
        S::Cancelled => match actor {
            Some(a) => format!(
                "only the user or the orchestrator can cancel a task, not the {}",
                a.as_str()
            ),
            None => format!("a {f} task can no longer be cancelled"),
        },
        S::Ready if from != S::Failed => format!("only failed tasks can be retried (task is {f})"),
        S::Finished if from != S::InProgress => {
            format!("only a task in progress can be finished, from its last column (task is {f})")
        }
        _ => match actor {
            Some(a) => format!("the {} may not move a task from {f} to {t}", a.as_str()),
            None => format!("a task cannot move from {f} to {t}"),
        },
    }
}

wire_enum! { TaskStatus, "task status", [
    Pending = "pending",
    Ready = "ready",
    InProgress = "in_progress",
    Finished = "finished",
    Cancelled = "cancelled",
    Failed = "failed",
]}

wire_enum! { Actor, "actor", [
    Orchestrator = "orchestrator",
    Agent = "agent",
    Daemon = "daemon",
    User = "user",
]}

impl TaskStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, TaskStatus::Finished | TaskStatus::Cancelled)
    }
}

/// Validate a transition: `Ok(())` when `actor` may move a task from `from`
/// to `to`. The table below is the whole of it, and
/// `exhaustive_transition_table` checks every (from, to, actor) triple
/// against a second reading of it.
pub fn check_transition(
    from: TaskStatus,
    to: TaskStatus,
    actor: Actor,
) -> Result<(), TransitionError> {
    use Actor as A;
    use TaskStatus as S;

    // Blanket rules first.
    match to {
        // The user cancels a task, and so does the orchestrator: it holds
        // the plan the task belongs to, and a task that the plan has moved
        // past is one it is the only agent able to see.
        S::Cancelled if !from.is_terminal() && from != S::Cancelled => {
            return if matches!(actor, A::User | A::Orchestrator) {
                Ok(())
            } else {
                Err(TransitionError::Forbidden { from, to, actor })
            };
        }
        // The daemon fails a task it cannot keep running, and the agent of
        // any column fails the one it works on: a task that cannot be done as
        // written is that agent's own finding, and `fail_task` is where it
        // says so.
        S::Failed if !from.is_terminal() && from != S::Failed => {
            return if matches!(actor, A::Daemon | A::Agent) {
                Ok(())
            } else {
                Err(TransitionError::Forbidden { from, to, actor })
            };
        }
        _ => {}
    }

    let allowed_actors: &[Actor] = match (from, to) {
        (S::Pending, S::Ready) => &[A::Daemon],
        // Re-added dependencies can send a ready task back to waiting.
        (S::Ready, S::Pending) => &[A::Orchestrator, A::Daemon],
        (S::Ready, S::InProgress) => &[A::Daemon],
        // The agent of the last column completes its step, which finishes
        // the task. The daemon finishes a request column after its agent read
        // the merge and fell quiet.
        (S::InProgress, S::Finished) => &[A::Agent, A::Daemon],
        // Retrying is the same judgement as cancelling, made the other way:
        // the user's, and the orchestrator's, which the daemon wakes when a
        // task fails.
        (S::Failed, S::Ready) => &[A::User, A::Orchestrator],
        _ => return Err(TransitionError::IllegalTransition { from, to }),
    };

    if allowed_actors.contains(&actor) {
        Ok(())
    } else {
        Err(TransitionError::Forbidden { from, to, actor })
    }
}

/// A step moves to one adjacent column within the snapshot.
pub fn check_step_move(from: usize, to: usize, len: usize) -> Result<(), &'static str> {
    if from < len && to < len && from.abs_diff(to) == 1 {
        Ok(())
    } else {
        Err("a step must move to an adjacent column")
    }
}

#[cfg(test)]
mod tests {
    use super::Actor as A;
    use super::TaskStatus as S;
    use super::*;

    /// The complete set of legal (from, to, actor) triples outside the two
    /// blanket rules.
    const LEGAL: &[(S, S, A)] = &[
        (S::Pending, S::Ready, A::Daemon),
        (S::Ready, S::Pending, A::Orchestrator),
        (S::Ready, S::Pending, A::Daemon),
        (S::Ready, S::InProgress, A::Daemon),
        (S::InProgress, S::Finished, A::Agent),
        (S::InProgress, S::Finished, A::Daemon),
        (S::Failed, S::Ready, A::User),
        (S::Failed, S::Ready, A::Orchestrator),
    ];

    fn is_legal(from: S, to: S, actor: A) -> bool {
        if LEGAL.contains(&(from, to, actor)) {
            return true;
        }
        // Blanket cancel / fail rules.
        (to == S::Cancelled
            && matches!(actor, A::User | A::Orchestrator)
            && !from.is_terminal()
            && from != S::Cancelled)
            || (to == S::Failed
                && matches!(actor, A::Daemon | A::Agent)
                && !from.is_terminal()
                && from != S::Failed)
    }

    /// Exhaustively check every (from, to, actor) combination of the six
    /// statuses and the four actors against the reference predicate: nothing
    /// extra is allowed, nothing legal rejected.
    #[test]
    fn exhaustive_transition_table() {
        assert_eq!(S::ALL.len(), 6);
        assert_eq!(A::ALL.len(), 4);
        for from in S::ALL {
            for to in S::ALL {
                for actor in A::ALL {
                    let expected = is_legal(from, to, actor);
                    let actual = check_transition(from, to, actor).is_ok();
                    assert_eq!(
                        expected, actual,
                        "mismatch for {from:?} -> {to:?} by {actor:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn terminal_states_are_frozen() {
        for terminal in [S::Finished, S::Cancelled] {
            for to in S::ALL {
                for actor in A::ALL {
                    assert!(
                        check_transition(terminal, to, actor).is_err(),
                        "{terminal:?} must be terminal, but {to:?} by {actor:?} passed"
                    );
                }
            }
        }
    }

    /// Failing a task is the daemon's and the agent's, and nobody else's:
    /// the daemon fails one whose agent it cannot keep running, and the agent
    /// of any column fails the one it works on when the task cannot be done
    /// as written. The edge exists from wherever its work had got to.
    #[test]
    fn an_agent_may_fail_the_task_it_works_on() {
        for from in [S::Ready, S::InProgress] {
            assert!(check_transition(from, S::Failed, A::Agent).is_ok());
        }
        for actor in [A::Orchestrator, A::User] {
            assert!(check_transition(S::InProgress, S::Failed, actor).is_err());
        }
    }

    #[test]
    fn error_distinguishes_forbidden_actor_from_illegal_edge() {
        // Legal edge, wrong actor.
        assert!(matches!(
            check_transition(S::InProgress, S::Finished, A::User),
            Err(TransitionError::Forbidden { .. })
        ));
        // Edge that exists for no actor.
        assert!(matches!(
            check_transition(S::Pending, S::Finished, A::Daemon),
            Err(TransitionError::IllegalTransition { .. })
        ));
    }

    /// The refusals a user provokes from the CLI/UI by name.
    #[test]
    fn human_messages_name_the_command_that_was_refused() {
        let human = |from, to, actor| check_transition(from, to, actor).unwrap_err().human();
        // `ariadne task retry <pending>`: the edge exists, but for the daemon.
        assert_eq!(
            human(S::Pending, S::Ready, A::User),
            "only failed tasks can be retried (task is pending)"
        );
        // `ariadne task cancel <cancelled>`: a no-op, not a violation.
        assert_eq!(
            human(S::Cancelled, S::Cancelled, A::User),
            "task is already cancelled"
        );
        // `ariadne task cancel <finished>`: too late, and it says so.
        assert_eq!(
            human(S::Finished, S::Cancelled, A::User),
            "a finished task can no longer be cancelled"
        );
        // An agent reaching for a cancel that is not its to make.
        assert_eq!(
            human(S::InProgress, S::Cancelled, A::Agent),
            "only the user or the orchestrator can cancel a task, not the agent"
        );
        // A finish from the wrong place.
        assert_eq!(
            human(S::Ready, S::Finished, A::Agent),
            "only a task in progress can be finished, from its last column (task is ready)"
        );
        // Anything else still names the move, in wire spelling.
        assert_eq!(
            human(S::InProgress, S::Finished, A::User),
            "the user may not move a task from in_progress to finished"
        );
    }

    /// No refusal may leak a Rust identifier: every status and actor a person
    /// sees is spelled the way the API spells it.
    #[test]
    fn human_messages_never_leak_pascal_case() {
        for from in S::ALL {
            for to in S::ALL {
                for actor in A::ALL {
                    if let Err(e) = check_transition(from, to, actor) {
                        let msg = e.human();
                        assert!(
                            !msg.chars().any(|c| c.is_ascii_uppercase()),
                            "{from:?} -> {to:?} by {actor:?} rendered as {msg:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn status_string_round_trip() {
        for s in S::ALL {
            assert_eq!(s.as_str().parse::<S>().unwrap(), s);
        }
        for a in A::ALL {
            assert_eq!(a.as_str().parse::<A>().unwrap(), a);
        }
    }
}

#[cfg(test)]
mod step_tests {
    use super::*;

    #[test]
    fn step_moves_only_reach_adjacent_columns() {
        for (from, to, len) in [(0, 1, 3), (1, 0, 3), (1, 2, 3), (2, 1, 3)] {
            assert!(check_step_move(from, to, len).is_ok());
        }
        for (from, to, len) in [(0, 2, 3), (1, 1, 3), (2, 3, 3), (3, 2, 3), (0, 0, 0)] {
            assert!(check_step_move(from, to, len).is_err());
        }
    }

    #[test]
    fn an_agent_finishes_work_or_fails_an_unfinished_task() {
        assert!(
            check_transition(TaskStatus::InProgress, TaskStatus::Finished, Actor::Agent).is_ok()
        );
        assert!(check_transition(TaskStatus::InProgress, TaskStatus::Failed, Actor::Agent).is_ok());
        assert!(check_transition(TaskStatus::Finished, TaskStatus::Failed, Actor::Agent).is_err());
        assert!(check_transition(TaskStatus::Ready, TaskStatus::Finished, Actor::Agent).is_err());
    }
}
