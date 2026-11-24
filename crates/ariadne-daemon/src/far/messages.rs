//! Relay task messages between a goal's daemon and a linked goal.

use ariadne_api::messages::MessageDto;
use ariadne_api::tasks::TaskDto;
use ariadne_client::ClientError;
use ariadne_core::{Actor, MessageKind};
use ariadne_store::{NewMessage, StoreError, Task};
use serde::{Deserialize, Serialize};

use super::{Follower, Unsent};

/// A message from the origin orchestrator. Staffed IDs differ between
/// daemons, so the receiving daemon resolves the column on its own task.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RelayMessage {
    pub(crate) id: String,
    pub(crate) origin_task_id: String,
    pub(crate) column: String,
    pub(crate) body: String,
}

impl Follower<'_> {
    pub(super) async fn send_messages(&self) -> Result<(), Unsent> {
        let store = &self.ctx.store;
        for message in store.pending_far_messages(self.host).await? {
            let task = store
                .get_task(message.task_id.as_deref().unwrap_or_default())
                .await?;
            let agent = if store.source_task_reference(&task.id).await?.is_some() {
                store
                    .source_task_agents(&task.id)
                    .await?
                    .into_iter()
                    .find(|a| Some(&a.id) == message.to_agent_id.as_ref())
                    .ok_or_else(|| {
                        Unsent::Refused("message recipient is no longer staffed".into())
                    })?
            } else {
                store
                    .get_task_agent(message.to_agent_id.as_deref().unwrap_or_default())
                    .await?
            };
            let Some(far_id) = task.far_task_id else {
                continue;
            };
            let request = RelayMessage {
                id: message.id.clone(),
                origin_task_id: task.id,
                column: agent.step,
                body: message.body,
            };
            self.fenced(
                self.client
                    .post_json::<(), _>(&format!("/v1/tasks/{far_id}/relay-message"), &request),
            )
            .await
            .ok_or(Unsent::Left)??;
            store.mark_message_relayed(&message.id).await?;
        }
        Ok(())
    }

    pub(super) async fn read_messages(
        &self,
        task: &Task,
        far: &TaskDto,
    ) -> Result<(), ClientError> {
        let path = format!("/v1/tasks/{}/messages", far.id);
        let Some(messages) = self
            .fenced(self.client.get_json::<Vec<MessageDto>>(&path))
            .await
        else {
            return Ok(());
        };
        for message in messages? {
            self.receive_message(task, far, message).await?;
        }
        Ok(())
    }

    pub(super) async fn on_message(&self, message: MessageDto) -> Result<(), ClientError> {
        if !from_far_agent(&message) || !self.current() {
            return Ok(());
        }
        let Some(far_id) = &message.task_id else {
            return Ok(());
        };
        let store = &self.ctx.store;
        let Some(task) = store
            .far_task_by_far_id(self.host, far_id)
            .await
            .map_err(local_store)?
        else {
            return Ok(());
        };
        if task.far_goal_id.as_deref() != Some(&message.goal_id) {
            return Ok(());
        }
        let Some(far) = self
            .fenced(
                self.client
                    .get_json::<TaskDto>(&format!("/v1/tasks/{far_id}")),
            )
            .await
        else {
            return Ok(());
        };
        self.receive_message(&task, &far?, message).await
    }

    async fn receive_message(
        &self,
        task: &Task,
        far: &TaskDto,
        message: MessageDto,
    ) -> Result<(), ClientError> {
        if !from_far_agent(&message)
            || message.goal_id != far.goal_id
            || message.task_id.as_deref() != Some(&far.id)
            || !self.current()
        {
            return Ok(());
        }
        let store = &self.ctx.store;
        let mut from_agent_id = None;
        if let Some(sender) = far
            .agents
            .iter()
            .find(|a| Some(&a.id) == message.from_agent_id.as_ref())
        {
            from_agent_id = store
                .list_task_agents(&task.id)
                .await
                .map_err(local_store)?
                .into_iter()
                .find(|agent| agent.step == sender.step)
                .map(|agent| agent.id);
        }
        if store
            .source_task_reference(&task.id)
            .await
            .map_err(local_store)?
            .is_some()
        {
            from_agent_id = message.from_agent_id.clone();
        }
        if !self.current() {
            return Ok(());
        }
        let added = store
            .receive_far_message(
                NewMessage {
                    goal_id: task.goal_id.clone(),
                    task_id: Some(task.id.clone()),
                    kind: MessageKind::Message,
                    from_actor: Actor::Agent,
                    from_agent_id,
                    from_session: None,
                    to_actor: Actor::Orchestrator,
                    to_agent_id: None,
                    body: message.body,
                },
                &message.id,
            )
            .await
            .map_err(local_store)?;
        if added.is_some() {
            self.ctx.wake(&task.id);
        }
        Ok(())
    }
}

fn from_far_agent(message: &MessageDto) -> bool {
    message.from_actor == Actor::Agent && message.to_actor == Actor::Orchestrator
}

pub(super) fn local_store(error: StoreError) -> ClientError {
    ClientError::Unreachable {
        endpoint: "the local store".into(),
        source: Box::new(error),
    }
}
