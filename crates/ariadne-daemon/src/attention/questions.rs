//! The question producer: one item for each question an agent asks with ACP
//! `elicitation/create` (021) and waits on. The item goes with the question.

use ariadne_api::attention::{
    AttentionCause, AttentionItemDto, AttentionProducer, AttentionSubjectDto, AttentionSubjectKind,
    AttentionTarget,
};
use ariadne_store::{Result, Store};

use crate::launcher::Launcher;

/// The longest summary an item takes from the agent's message.
const SUMMARY_MAX_CHARS: usize = 200;

pub(crate) async fn items(store: &Store, launcher: &Launcher) -> Result<Vec<AttentionItemDto>> {
    let mut items = Vec::new();
    for question in launcher.acp.all_open_questions().await {
        let session = store.get_session(&question.session_id).await?;
        if !session.status().is_live() || !super::work_is_active_checked(store, &session).await? {
            continue;
        }
        let message = question.message.lines().next().unwrap_or_default().trim();
        let summary = match message.chars().count() {
            0 => "The agent asks you a question.".to_string(),
            n if n > SUMMARY_MAX_CHARS => {
                format!(
                    "{}…",
                    message.chars().take(SUMMARY_MAX_CHARS).collect::<String>()
                )
            }
            _ => message.to_string(),
        };
        items.push(AttentionItemDto {
            occurrence: question.id.clone(),
            id: format!("question:{}", question.id),
            producer: AttentionProducer::AgentRequest,
            reason: AttentionCause::Unknown,
            summary,
            required_action: "Answer the question in the agent's console.".into(),
            since: question.created_at,
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
