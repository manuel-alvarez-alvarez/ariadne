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
    /// Engineer instructed to merge into the base branch.
    Merging,
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
    Daemon,
    User,
}

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
            TaskStatus::Merging => "merging",
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
        TaskStatus::Merging,
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
/// | from                | to                 | actor            |
/// |---------------------|--------------------|------------------|
/// | pending             | ready              | daemon           |
/// | ready               | pending            | planner, daemon  |
/// | ready               | in_progress        | daemon           |
/// | in_progress         | under_review       | engineer         |
/// | under_review        | changes_requested  | daemon           |
/// | under_review        | approved           | daemon           |
/// | changes_requested   | in_progress        | daemon           |
/// | approved            | merging            | daemon           |
/// | merging             | merged             | engineer         |
/// | any non-terminal    | cancelled          | user             |
/// | any non-terminal    | failed             | daemon           |
/// | failed              | ready              | user (retry)     |
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
        (S::Approved, S::Merging) => &[A::Daemon],
        (S::Merging, S::Merged) => &[A::Engineer],
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

    const ACTORS: [Actor; 5] = [A::Planner, A::Engineer, A::Reviewer, A::Daemon, A::User];

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
        (S::Approved, S::Merging, A::Daemon),
        (S::Merging, S::Merged, A::Engineer),
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
            check_transition(S::Merging, S::Merged, A::Reviewer),
            Err(TransitionError::Forbidden { .. })
        ));
        // Edge that exists for no actor.
        assert!(matches!(
            check_transition(S::Pending, S::Merged, A::Daemon),
            Err(TransitionError::IllegalTransition { .. })
        ));
    }

    #[test]
    fn status_string_round_trip() {
        for s in S::ALL {
            assert_eq!(s.as_str().parse::<S>().unwrap(), s);
        }
    }
}
