//! Posts of worker agents to the source orchestrator (039).
//!
//! The worker sends each one as a `source_post` frame on the event stream
//! that this daemon holds open on it. The follower puts it into the live
//! orchestrator session of the source goal, at once, or drops it.

use ariadne_api::messages::{RemotePostRequest, SourcePostDto};
use ariadne_api::tasks::TaskDto;
use ariadne_client::ClientError;
use ariadne_core::Seat;
use ariadne_store::SessionFilter;
use axum::http::Method;
use tracing::warn;

use super::Follower;
use crate::agents::prompts;
use crate::http::agent_sender;

impl Follower<'_> {
    pub(super) async fn on_post(&self, post: SourcePostDto) -> Result<(), ClientError> {
        if !self.current() {
            return Ok(());
        }
        let store = &self.ctx.store;
        let unreadable = |error: ariadne_store::StoreError| ClientError::Unreachable {
            endpoint: "the local store".into(),
            source: Box::new(error),
        };
        let Some(reference) = store
            .source_task_reference(&post.task_id)
            .await
            .map_err(unreadable)?
        else {
            warn!(host = self.host, task = %post.task_id, "a post names no task of this source");
            return Ok(());
        };
        // The worker that holds the stream, by its persistent ID, must own the
        // task. A host name may point to a replacement daemon, or share a
        // worker with an alias.
        let Some(protocol) = self
            .fenced(
                self.client
                    .get_json::<ariadne_api::tasks::RemoteTaskProtocol>(
                        "/v1/remote-tasks/protocol",
                    ),
            )
            .await
        else {
            return Ok(());
        };
        if protocol?.daemon_id != reference.worker_id {
            warn!(host = self.host, task = %post.task_id, "a post names a task of another worker");
            return Ok(());
        }
        // The title as the source goal knows it: the projection of 037 rule
        // 105 takes it from the cached worker task.
        let title = serde_json::from_str::<TaskDto>(&reference.cache)
            .map(|task| task.title)
            .unwrap_or_default();
        let orchestrator = store
            .list_sessions(SessionFilter {
                goal_id: Some(reference.goal_id.clone()),
                live_only: true,
                ..Default::default()
            })
            .await
            .map_err(unreadable)?
            .into_iter()
            .find(|s| s.seat() == Some(Seat::Orchestrator));
        let Some(orchestrator) = orchestrator else {
            warn!(host = self.host, task = %reference.id, "the orchestrator has no live session for a post");
            self.tell_dropped(
                &reference.id,
                &post,
                "The orchestrator has no live session.",
            )
            .await;
            return Ok(());
        };
        let (from, answer) = agent_sender(&post.column, &reference.id, &title);
        let text = prompts::post_briefing(&from, post.body.trim(), &answer);
        if let Err(error) = self.ctx.launcher.acp.send_post(&orchestrator.id, text) {
            warn!(host = self.host, task = %reference.id, %error, "cannot hand a post to the orchestrator");
        }
        Ok(())
    }

    /// Tell the worker agent of the sender that the source dropped its post.
    /// The source sends the notice once and holds none: a refusal of the
    /// worker drops it.
    async fn tell_dropped(&self, task_id: &str, post: &SourcePostDto, reason: &str) {
        let daemon_id = match self.ctx.store.daemon_id().await {
            Ok(id) => id,
            Err(error) => {
                warn!(host = self.host, %error, "cannot read the daemon id for a notice");
                return;
            }
        };
        let request = RemotePostRequest {
            source_daemon_id: daemon_id,
            to: post.column.clone(),
            body: post.body.clone(),
            dropped: Some(reason.to_string()),
        };
        let sent = self
            .fenced(self.client.send_no_content(
                Method::POST,
                &format!("/v1/remote-tasks/{task_id}/posts"),
                Some(&request),
            ))
            .await;
        if let Some(Err(error)) = sent {
            warn!(host = self.host, task = task_id, %error, "the worker refused a notice of a dropped post");
        }
    }
}
