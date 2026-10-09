//! Change notifications: every mutating [`Store`](crate::Store) method
//! announces the row it just committed here.
//!
//! Emission lives in the repository layer on purpose: the HTTP handlers, the
//! scheduler and the launcher all write through these methods, so no state
//! change can reach the database unannounced.

use crate::{
    AgentEvent, AgentSession, Goal, LearnedPermission, Message, Repository, Skill, Task,
    TaskTransition, Workflow,
};

/// A committed write, carrying the row as it now stands.
#[derive(Debug, Clone)]
pub enum Change {
    /// What Ariadne keeps of a repository's requests moved: a request it
    /// started or stopped working on, or a flag of one it works on. Carries
    /// the repository id; the requests themselves are the forge's.
    PullRequestsChanged(String),
    GoalCreated(Goal),
    GoalUpdated(Goal),
    GoalDeleted(String),
    TaskCreated(Task),
    /// Any task write; `transition` is set when it was a status change.
    TaskUpdated {
        task: Task,
        transition: Option<TaskTransition>,
    },
    MessageSent(Message),
    SessionCreated(AgentSession),
    SessionUpdated(AgentSession),
    AgentEventCreated(AgentEvent),
    SkillCreated(Skill),
    SkillUpdated(Skill),
    SkillDeleted(String),
    WorkflowCreated(Workflow),
    WorkflowUpdated(Workflow),
    WorkflowDeleted(String),
    RepositoryCreated(Repository),
    RepositoryUpdated(Repository),
    RepositoryDeleted(String),
    LearnedPermissionCreated(LearnedPermission),
    LearnedPermissionUpdated(LearnedPermission),
    LearnedPermissionDeleted(LearnedPermission),
}
