//! Post DTOs: what one agent posts to the live session of another (039).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Body of `POST /v1/tasks/{id}/posts`: text for the live session of another
/// agent, with no stored row. The sender is the session header's.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PostToSessionRequest {
    /// `orchestrator`, or the column of a staffed agent of the task.
    pub to: String,
    pub body: String,
}

/// Body of `POST /v1/remote-tasks/{id}/posts`: a post of the source
/// orchestrator to a column of a worker-protocol task on this worker.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RemotePostRequest {
    /// The persistent daemon ID of the source that sends the post.
    pub source_daemon_id: String,
    /// The column of a staffed agent of the task.
    pub to: String,
    /// The body of the post, or of the dropped post when `dropped` is set.
    pub body: String,
    /// Set when this is a notice that the source dropped the post in `body`
    /// of a worker agent to its orchestrator: the reason. The worker puts a
    /// notice into the session, not a post, and asks for no answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped: Option<String>,
}

/// The `source_post` frame of `GET /v1/events/stream?source=…`: a post of a
/// worker agent to the orchestrator of its source goal. The worker sends it
/// on one open stream of that source, live, and never again.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SourcePostDto {
    pub id: String,
    pub task_id: String,
    /// The column of the sender.
    pub column: String,
    pub body: String,
}
