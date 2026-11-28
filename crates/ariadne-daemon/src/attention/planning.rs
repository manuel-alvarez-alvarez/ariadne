//! A completed planning turn can leave its question in console text.
use ariadne_api::attention::{
    AttentionCause, AttentionItemDto, AttentionProducer, AttentionSubjectDto, AttentionSubjectKind,
    AttentionTarget,
};
use ariadne_core::{GoalStatus, Seat, SessionStatus};
use ariadne_store::{Result, SessionFilter, Store};

pub(crate) async fn items(store: &Store) -> Result<Vec<AttentionItemDto>> {
    let mut items = Vec::new();
    for session in store
        .list_sessions(SessionFilter {
            status: Some(SessionStatus::Idle),
            ..Default::default()
        })
        .await?
    {
        if session.seat() != Some(Seat::Orchestrator) || session.status() != SessionStatus::Idle {
            continue;
        }
        let Some(goal_id) = &session.goal_id else {
            continue;
        };
        if store.get_goal(goal_id).await?.status() != GoalStatus::Planning {
            continue;
        }
        let events = store.list_session_events(&session.id).await?;
        let Some(start) = events
            .iter()
            .rposition(|event| event.kind == "user_prompt_submit")
        else {
            continue;
        };
        let turn = &events[start + 1..];
        if turn
            .iter()
            .any(|event| event.kind == crate::acp_questions::QUESTION_EVENT)
        {
            continue;
        }
        let Some(stop) = turn.iter().rev().find(|event| event.kind == "stop") else {
            continue;
        };
        let payload: serde_json::Value = serde_json::from_str(&stop.payload).unwrap_or_default();
        if payload["stop_reason"] == "cancelled" {
            continue;
        }
        items.push(AttentionItemDto {
            id: format!("planning_input:{}", session.id),
            occurrence: stop.id.clone(),
            producer: AttentionProducer::AgentRequest,
            reason: AttentionCause::Unknown,
            summary: "The planning orchestrator may have asked in text.".into(),
            required_action: "Answer in the orchestrator's console.".into(),
            since: stop.created_at.clone(),
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
