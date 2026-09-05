//! Task state machine: statuses, actors, and the transition table.
//!
//! This is the single authority on which transitions are legal and who may
//! trigger them. The store validates every status change against it inside
//! the same transaction that records the audit row.
//!
//! The actors are *seats*, not identities: an agent is generic, and what it
//! knows how to do comes from the skills it loads. A seat says only where an
//! agent sits — the orchestrator of a goal, or the author or a reviewer of
//! one task — which is the whole of what this table needs to know about it.

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
    /// Every dependency has finished; waiting for an author session.
    Ready,
    /// Author session active in its worktree.
    InProgress,
    /// The author requested review; reviewer sessions active.
    UnderReview,
    /// At least one reviewer requested changes this round.
    ChangesRequested,
    /// Enough approvals collected; the author is finishing it.
    Approved,
    /// The work is done and whatever it produced is where it belongs: a
    /// change landed on the base branch, a request published, a report filed,
    /// a release out. Terminal.
    ///
    /// Landing is one way to reach this and not the definition of it — a task
    /// with no change to land finishes all the same, and `merge_commit` and
    /// `pr_url` record which way it went.
    Finished,
    /// Cancelled by the user. Terminal.
    Cancelled,
    /// Unrecoverable failure (retry budget exhausted). Retryable by the user.
    Failed,
}

/// Who is attempting a transition: a seat, the daemon, or the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Orchestrator,
    Author,
    Reviewer,
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
            Some(a) => format!("only the user can cancel a task, not the {}", a.as_str()),
            None => format!("a {f} task can no longer be cancelled"),
        },
        S::Ready if from != S::Failed => format!("only failed tasks can be retried (task is {f})"),
        S::UnderReview if !matches!(from, S::InProgress | S::Approved) => {
            format!("only an in-progress or approved task can be sent for review (task is {f})")
        }
        S::Finished if from != S::Approved => {
            format!("only an approved task can be finished (task is {f})")
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
    UnderReview = "under_review",
    ChangesRequested = "changes_requested",
    Approved = "approved",
    Finished = "finished",
    Cancelled = "cancelled",
    Failed = "failed",
]}

wire_enum! { Actor, "actor", [
    Orchestrator = "orchestrator",
    Author = "author",
    Reviewer = "reviewer",
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
        S::Cancelled if !from.is_terminal() && from != S::Cancelled => {
            return if actor == A::User {
                Ok(())
            } else {
                Err(TransitionError::Forbidden { from, to, actor })
            };
        }
        // The daemon fails a task it cannot keep running, and the author
        // fails the one it owns: a task that cannot be done as written is the
        // author's own finding, and `fail_task` is where it says so.
        S::Failed if !from.is_terminal() && from != S::Failed => {
            return if matches!(actor, A::Daemon | A::Author) {
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
        (S::InProgress, S::UnderReview) => &[A::Author],
        (S::UnderReview, S::ChangesRequested) => &[A::Daemon],
        (S::UnderReview, S::Approved) => &[A::Daemon],
        (S::ChangesRequested, S::InProgress) => &[A::Daemon],
        // Ending the task is the author's own: an approved task is one it is
        // finishing, and `finish_task` is the only way out of it.
        (S::Approved, S::Finished) => &[A::Author],
        // And back to the reviewers when the people on a published request
        // ask for changes: that revision is reviewed like any other round.
        (S::Approved, S::UnderReview) => &[A::Author],
        (S::Failed, S::Ready) => &[A::User],
        _ => return Err(TransitionError::IllegalTransition { from, to }),
    };

    if allowed_actors.contains(&actor) {
        Ok(())
    } else {
        Err(TransitionError::Forbidden { from, to, actor })
    }
}

#[cfg(test)]
mod tests {
    use super::Actor as A;
    use super::TaskStatus as S;
    use super::*;

    const ACTORS: [Actor; 5] = [A::Orchestrator, A::Author, A::Reviewer, A::Daemon, A::User];

    /// The complete set of legal (from, to, actor) triples.
    const LEGAL: &[(S, S, A)] = &[
        (S::Pending, S::Ready, A::Daemon),
        (S::Ready, S::Pending, A::Orchestrator),
        (S::Ready, S::Pending, A::Daemon),
        (S::Ready, S::InProgress, A::Daemon),
        (S::InProgress, S::UnderReview, A::Author),
        (S::UnderReview, S::ChangesRequested, A::Daemon),
        (S::UnderReview, S::Approved, A::Daemon),
        (S::ChangesRequested, S::InProgress, A::Daemon),
        (S::Approved, S::Finished, A::Author),
        (S::Approved, S::UnderReview, A::Author),
        (S::Failed, S::Ready, A::User),
    ];

    fn is_legal(from: S, to: S, actor: A) -> bool {
        if LEGAL.contains(&(from, to, actor)) {
            return true;
        }
        // Blanket cancel / fail rules.
        (to == S::Cancelled && actor == A::User && !from.is_terminal() && from != S::Cancelled)
            || (to == S::Failed
                && matches!(actor, A::Daemon | A::Author)
                && !from.is_terminal()
                && from != S::Failed)
    }

    /// Exhaustively check every (from, to, actor) combination against the
    /// reference predicate: nothing extra is allowed, nothing legal rejected.
    #[test]
    fn exhaustive_transition_table() {
        for from in S::ALL {
            for to in S::ALL {
                for actor in ACTORS {
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
                for actor in ACTORS {
                    assert!(
                        check_transition(terminal, to, actor).is_err(),
                        "{terminal:?} must be terminal, but {to:?} by {actor:?} passed"
                    );
                }
            }
        }
    }

    /// Failing a task is the daemon's and the author's, and nobody else's:
    /// the daemon fails one whose agent it cannot keep running, and the
    /// author fails the one it owns when the task cannot be done as written.
    /// That is the whole of what an author says about it, so the edge exists
    /// from wherever its work had got to.
    #[test]
    fn the_author_may_fail_the_task_it_owns() {
        for from in [
            S::Ready,
            S::InProgress,
            S::UnderReview,
            S::ChangesRequested,
            S::Approved,
        ] {
            assert!(check_transition(from, S::Failed, A::Author).is_ok());
        }
        for actor in [A::Orchestrator, A::Reviewer, A::User] {
            assert!(check_transition(S::InProgress, S::Failed, actor).is_err());
        }
    }

    #[test]
    fn error_distinguishes_forbidden_actor_from_illegal_edge() {
        // Legal edge, wrong actor.
        assert!(matches!(
            check_transition(S::Approved, S::Finished, A::Reviewer),
            Err(TransitionError::Forbidden { .. })
        ));
        // Edge that exists for no actor.
        assert!(matches!(
            check_transition(S::Pending, S::Finished, A::Daemon),
            Err(TransitionError::IllegalTransition { .. })
        ));
    }

    /// The three refusals a user provokes from the CLI/UI by name.
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
        // An agent reaching for the user's cancel.
        assert_eq!(
            human(S::InProgress, S::Cancelled, A::Orchestrator),
            "only the user can cancel a task, not the orchestrator"
        );
        // Agent-side verbs get the same treatment.
        assert_eq!(
            human(S::Pending, S::UnderReview, A::Author),
            "only an in-progress or approved task can be sent for review (task is pending)"
        );
        assert_eq!(
            human(S::InProgress, S::Finished, A::Author),
            "only an approved task can be finished (task is in_progress)"
        );
        // Anything else still names the move, in wire spelling.
        assert_eq!(
            human(S::Approved, S::Finished, A::Reviewer),
            "the reviewer may not move a task from approved to finished"
        );
    }

    /// No refusal may leak a Rust identifier: every status and actor a person
    /// sees is spelled the way the API spells it.
    #[test]
    fn human_messages_never_leak_pascal_case() {
        for from in S::ALL {
            for to in S::ALL {
                for actor in ACTORS {
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
    }
}
