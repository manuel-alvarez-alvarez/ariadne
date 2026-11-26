//! The session producer: a live session's own flag for the user.
//!
//! Pending human permission prompts, input, waits and errors.

use ariadne_api::attention::{
    AttentionCause, AttentionItemDto, AttentionProducer, AttentionSubjectDto, AttentionSubjectKind,
    AttentionTarget,
};
use ariadne_core::AttentionReason;
use ariadne_store::SessionFilter;
use ariadne_store::{Result, Store};

pub(crate) async fn items(store: &Store) -> Result<Vec<AttentionItemDto>> {
    let mut items = Vec::new();
    for session in store
        .list_sessions(SessionFilter {
            attention_only: true,
            ..Default::default()
        })
        .await?
    {
        let Some(reason) = session.attention_reason() else {
            continue;
        };
        if !session.status().is_live() || !super::work_is_active_checked(store, &session).await? {
            continue;
        }
        let summary = match reason {
            AttentionReason::WaitingPermission => "The agent needs permission to continue.",
            AttentionReason::WaitingInput => "The agent needs input to continue.",
            AttentionReason::WaitingUser if session.pull_request_id.is_none() => {
                "The agent is waiting for you."
            }
            AttentionReason::AgentError => "The agent reported an error.",
            _ => continue,
        };
        let since = session
            .attention_since
            .clone()
            .unwrap_or_else(|| session.created_at.clone());
        items.push(AttentionItemDto {
            occurrence: String::new(),
            id: format!("agent_request:{}:{}", reason.as_str(), session.id),
            producer: AttentionProducer::AgentRequest,
            reason: AttentionCause::Unknown,
            summary: summary.into(),
            required_action: "Open the agent console to respond.".into(),
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
