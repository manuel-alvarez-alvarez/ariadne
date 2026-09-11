//! Sessions an ACP agent stores itself, asked over the protocol: the ones
//! Ariadne did not start, which a task can adopt.

use std::collections::HashSet;

use anyhow::Result;

use ariadne_api::sessions::OutsideSessionDto;
use ariadne_core::models::agent_of;
use ariadne_store::{SessionFilter, Store};

use crate::acp_discovery::AgentRegistry;

/// The stored sessions of every registry agent that can list them, minus the
/// ones already bound to an Ariadne session row.
///
/// The `model` column of a session carries `<agent>:<model>`, so the agent an
/// already-adopted session belongs to is read back off it rather than kept
/// anywhere else.
pub async fn discover(registry: &AgentRegistry, store: &Store) -> Result<Vec<OutsideSessionDto>> {
    let known: HashSet<(String, String)> = store
        .list_sessions(SessionFilter::default())
        .await?
        .into_iter()
        .filter_map(|session| {
            let agent_id = agent_of(&session.model).to_string();
            session
                .internal_session_id
                .map(|internal| (agent_id, internal))
        })
        .collect();
    let mut sessions = registry.stored_sessions().await;
    sessions.retain(|session| {
        !known.contains(&(
            session.agent_id.clone(),
            session.internal_session_id.clone(),
        ))
    });
    Ok(sessions)
}
