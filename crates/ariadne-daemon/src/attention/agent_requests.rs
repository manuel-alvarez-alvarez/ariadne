//! The agent-request producer: a session's own question to the user.
//!
//! Explicit questions and pending human permission prompts.

use ariadne_api::attention::{
    AttentionCause, AttentionItemDto, AttentionProducer, AttentionSubjectDto, AttentionSubjectKind,
    AttentionTarget,
};
use ariadne_core::AttentionReason;
use ariadne_store::SessionFilter;
use ariadne_store::{Result, Store};

pub(crate) async fn items(store: &Store) -> Result<Vec<AttentionItemDto>> {
    let mut items = Vec::new();
    for request in store.pending_agent_requests().await? {
        let session = store.get_session(&request.session_id).await?;
        if !super::work_is_active_checked(store, &session).await? {
            continue;
        }
        items.push(AttentionItemDto {
            id: format!("agent_request:{}", request.id),
            producer: AttentionProducer::AgentRequest,
            reason: AttentionCause::Unknown,
            summary: request.summary,
            required_action: "Answer the agent in its console.".into(),
            since: request.created_at,
            affected: vec![AttentionSubjectDto {
                kind: AttentionSubjectKind::Session,
                id: session.id.clone(),
                label: session.model.clone(),
            }],
            target: AttentionTarget::Console {
                session_id: session.id,
            },
        });
    }
    for session in store
        .list_sessions(SessionFilter {
            attention_only: true,
            ..Default::default()
        })
        .await?
    {
        if session.attention_reason() != Some(AttentionReason::WaitingPermission)
            || !super::work_is_active_checked(store, &session).await?
        {
            continue;
        }
        let since = session
            .attention_since
            .clone()
            .unwrap_or_else(|| session.created_at.clone());
        items.push(AttentionItemDto {
            id: format!("agent_request:permission:{}", session.id),
            producer: AttentionProducer::AgentRequest,
            reason: AttentionCause::Unknown,
            summary: "The agent needs permission to continue.".into(),
            required_action: "Answer the permission request in the agent console.".into(),
            since,
            affected: vec![AttentionSubjectDto {
                kind: AttentionSubjectKind::Session,
                id: session.id.clone(),
                label: session.model.clone(),
            }],
            target: AttentionTarget::Console {
                session_id: session.id,
            },
        });
    }
    Ok(items)
}
