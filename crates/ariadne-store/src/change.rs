//! Change notifications: every mutating [`Store`](crate::Store) method
//! announces the row it just committed here.
//!
//! Emission lives in the repository layer on purpose: the HTTP handlers, the
//! scheduler and the launcher all write through these methods, so no state
//! change can reach the database unannounced.

use crate::{
    AgentEvent, AgentSession, Goal, Message, Profile, Repository, Review, Task, TaskTransition,
};

/// A committed write, carrying the row as it now stands.
#[derive(Debug, Clone)]
pub enum Change {
    GoalCreated(Goal),
    GoalUpdated(Goal),
    GoalDeleted(String),
    TaskCreated(Task),
    /// Any task write; `transition` is set when it was a status change.
    TaskUpdated {
        task: Task,
        transition: Option<TaskTransition>,
    },
    MessageCreated(Message),
    ReviewCreated(Review),
    SessionCreated(AgentSession),
    SessionUpdated(AgentSession),
    AgentEventCreated(AgentEvent),
    ProfileCreated(Profile),
    ProfileUpdated(Profile),
    ProfileDeleted(String),
    RepositoryCreated(Repository),
    RepositoryUpdated(Repository),
    RepositoryDeleted(String),
}
