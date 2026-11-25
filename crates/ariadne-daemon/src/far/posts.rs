//! Posts of worker agents to the source orchestrator (039).
//!
//! The worker sends each one as a `source_post` frame on the event stream
//! that this daemon holds open on it. The follower puts it into the live
//! orchestrator session of the source goal, at once, or drops it.

use ariadne_api::messages::SourcePostDto;
use ariadne_api::tasks::TaskDto;
use ariadne_client::ClientError;
use ariadne_core::Seat;
use ariadne_store::SessionFilter;
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
        // Two host names of one worker share the stream of the worker, so the
        // post may come through the alias. Then the worker itself, by its
        // persistent ID, must own the task.
        if reference.host != self.host {
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
            return Ok(());
        };
        let (from, answer) = agent_sender(&post.column, &reference.id, &title);
        let text = prompts::post_briefing(&from, post.body.trim(), &answer);
        if let Err(error) = self.ctx.launcher.acp.send_post(&orchestrator.id, text) {
            warn!(host = self.host, task = %reference.id, %error, "cannot hand a post to the orchestrator");
        }
        Ok(())
    }
}
