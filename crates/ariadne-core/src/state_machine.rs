//! Task state machine: statuses, actors, and the transition table.
//!
//! This is the single authority on which transitions are legal and who may
//! trigger them. The store validates every status change against it inside
//! the same transaction that records the audit row.

use serde::{Deserialize, Serialize};

/// Task lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(
    feature = "clap",
    derive(clap::ValueEnum),
    value(rename_all = "snake_case")
)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Created by the planner; waiting for dependencies to merge.
    Pending,
    /// All dependencies merged; waiting for an engineer session.
    Ready,
    /// Engineer session active in its worktree.
    InProgress,
    /// Engineer requested review; reviewer sessions active.
    UnderReview,
    /// At least one reviewer requested changes this round.
    ChangesRequested,
    /// Enough approvals collected; waiting for merge instruction.
    Approved,
    /// The task was told to merge into the base branch.
    Integrating,
    /// Merge verified on the base branch. Terminal.
    Merged,
    /// Cancelled by the user. Terminal.
    Cancelled,
    /// Unrecoverable failure (retry budget exhausted). Retryable by the user.
    Failed,
}

/// Who is attempting a transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Planner,
    Engineer,
    Reviewer,
    Integrator,
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
        S::UnderReview if from != S::InProgress => {
            format!("only an in-progress task can be sent for review (task is {f})")
        }
        S::Merged if from != S::Integrating => {
            format!("only a task that was told to merge can be marked merged (task is {f})")
        }
        _ => match actor {
            Some(a) => format!("the {} may not move a task from {f} to {t}", a.as_str()),
            None => format!("a task cannot move from {f} to {t}"),
        },
    }
}

impl TaskStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, TaskStatus::Merged | TaskStatus::Cancelled)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Ready => "ready",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::UnderReview => "under_review",
            TaskStatus::ChangesRequested => "changes_requested",
            TaskStatus::Approved => "approved",
            TaskStatus::Integrating => "integrating",
            TaskStatus::Merged => "merged",
            TaskStatus::Cancelled => "cancelled",
            TaskStatus::Failed => "failed",
        }
    }

    pub const ALL: [TaskStatus; 10] = [
        TaskStatus::Pending,
        TaskStatus::Ready,
        TaskStatus::InProgress,
        TaskStatus::UnderReview,
        TaskStatus::ChangesRequested,
        TaskStatus::Approved,
        TaskStatus::Integrating,
        TaskStatus::Merged,
        TaskStatus::Cancelled,
        TaskStatus::Failed,
    ];
}

impl std::str::FromStr for TaskStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        TaskStatus::ALL
            .into_iter()
            .find(|v| v.as_str() == s)
            .ok_or_else(|| format!("unknown task status: {s}"))
    }
}

impl Actor {
    pub fn as_str(&self) -> &'static str {
        match self {
            Actor::Planner => "planner",
            Actor::Engineer => "engineer",
            Actor::Reviewer => "reviewer",
            Actor::Integrator => "integrator",
            Actor::Daemon => "daemon",
            Actor::User => "user",
        }
    }
}

impl std::str::FromStr for Actor {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "planner" => Ok(Actor::Planner),
            "engineer" => Ok(Actor::Engineer),
            "reviewer" => Ok(Actor::Reviewer),
            "integrator" => Ok(Actor::Integrator),
            "daemon" => Ok(Actor::Daemon),
            "user" => Ok(Actor::User),
            other => Err(format!("unknown actor: {other}")),
        }
    }
}

/// Validate a transition. Returns `Ok(())` when `actor` may move a task from
/// `from` to `to`.
///
/// The table mirrors the plan:
///
/// | from                | to                 | actor                |
/// |---------------------|--------------------|----------------------|
/// | pending             | ready              | daemon               |
/// | ready               | pending            | planner, daemon      |
/// | ready               | in_progress        | daemon               |
/// | in_progress         | under_review       | engineer             |
/// | under_review        | changes_requested  | daemon               |
/// | under_review        | approved           | daemon               |
/// | changes_requested   | in_progress        | daemon               |
/// | approved            | integrating        | daemon               |
/// | integrating         | merged             | integrator           |
/// | integrating         | changes_requested  | integrator, daemon   |
/// | any non-terminal    | cancelled          | user                 |
/// | any non-terminal    | failed             | daemon               |
/// | failed              | ready              | user (retry)         |
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
        S::Failed if !from.is_terminal() && from != S::Failed => {
            return if actor == A::Daemon {
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
        (S::Ready, S::Pending) => &[A::Planner, A::Daemon],
        (S::Ready, S::InProgress) => &[A::Daemon],
        (S::InProgress, S::UnderReview) => &[A::Engineer],
        (S::UnderReview, S::ChangesRequested) => &[A::Daemon],
        (S::UnderReview, S::Approved) => &[A::Daemon],
        (S::ChangesRequested, S::InProgress) => &[A::Daemon],
        (S::Approved, S::Integrating) => &[A::Daemon],
        // Landing the change is the integrator's alone: the engineer's task
        // ends at the approval that hands it over.
        (S::Integrating, S::Merged) => &[A::Integrator],
        // Sending an integrating task back to the engineer: a conflict the
        // integrator will not resolve for it, or a comment on the pull
        // request.
        (S::Integrating, S::ChangesRequested) => &[A::Integrator, A::Daemon],
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

    const ACTORS: [Actor; 6] = [
        A::Planner,
        A::Engineer,
        A::Reviewer,
        A::Integrator,
        A::Daemon,
        A::User,
    ];

    /// The complete set of legal (from, to, actor) triples.
    const LEGAL: &[(S, S, A)] = &[
        (S::Pending, S::Ready, A::Daemon),
        (S::Ready, S::Pending, A::Planner),
        (S::Ready, S::Pending, A::Daemon),
        (S::Ready, S::InProgress, A::Daemon),
        (S::InProgress, S::UnderReview, A::Engineer),
        (S::UnderReview, S::ChangesRequested, A::Daemon),
        (S::UnderReview, S::Approved, A::Daemon),
        (S::ChangesRequested, S::InProgress, A::Daemon),
        (S::Approved, S::Integrating, A::Daemon),
        (S::Integrating, S::Merged, A::Integrator),
        (S::Integrating, S::ChangesRequested, A::Integrator),
        (S::Integrating, S::ChangesRequested, A::Daemon),
        (S::Failed, S::Ready, A::User),
    ];

    fn is_legal(from: S, to: S, actor: A) -> bool {
        if LEGAL.contains(&(from, to, actor)) {
            return true;
        }
        // Blanket cancel / fail rules.
        (to == S::Cancelled && actor == A::User && !from.is_terminal() && from != S::Cancelled)
            || (to == S::Failed && actor == A::Daemon && !from.is_terminal() && from != S::Failed)
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
        for terminal in [S::Merged, S::Cancelled] {
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

    #[test]
    fn error_distinguishes_forbidden_actor_from_illegal_edge() {
        // Legal edge, wrong actor.
        assert!(matches!(
            check_transition(S::Integrating, S::Merged, A::Reviewer),
            Err(TransitionError::Forbidden { .. })
        ));
        // Edge that exists for no actor.
        assert!(matches!(
            check_transition(S::Pending, S::Merged, A::Daemon),
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
        // `ariadne task cancel <merged>`: too late, and it says so.
        assert_eq!(
            human(S::Merged, S::Cancelled, A::User),
            "a merged task can no longer be cancelled"
        );
        // An agent reaching for the user's cancel.
        assert_eq!(
            human(S::InProgress, S::Cancelled, A::Planner),
            "only the user can cancel a task, not the planner"
        );
        // Agent-side verbs get the same treatment.
        assert_eq!(
            human(S::Pending, S::UnderReview, A::Engineer),
            "only an in-progress task can be sent for review (task is pending)"
        );
        assert_eq!(
            human(S::InProgress, S::Merged, A::Engineer),
            "only a task that was told to merge can be marked merged (task is in_progress)"
        );
        // Anything else still names the move, in wire spelling.
        assert_eq!(
            human(S::Integrating, S::Merged, A::Reviewer),
            "the reviewer may not move a task from integrating to merged"
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
