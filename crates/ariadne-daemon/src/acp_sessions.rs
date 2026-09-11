//! Sessions an ACP agent stores itself: the ACP counterpart of
//! [`crate::outside_sessions`], asked over the protocol rather than read off
//! a CLI's own transcript store.

use std::collections::HashSet;

use anyhow::Result;

use ariadne_api::sessions::OutsideSessionDto;
use ariadne_core::AgentKind;
use ariadne_store::{SessionFilter, Store};

use crate::acp_discovery::AgentRegistry;

/// The stored sessions of every registry agent that can list them, minus the
/// ones already bound to an Ariadne session row.
///
/// The `model` column of an ACP session carries `<agent id>:<model>`, so the
/// agent id an already-adopted session belongs to is read back off it rather
/// than kept anywhere else.
pub async fn discover(registry: &AgentRegistry, store: &Store) -> Result<Vec<OutsideSessionDto>> {
    let known: HashSet<(String, String)> = store
        .list_sessions(SessionFilter::default())
        .await?
        .into_iter()
        .filter(|session| session.agent_kind() == AgentKind::Acp)
        .filter_map(|session| {
            let agent_id = session
                .model
                .split_once(':')
                .map(|(id, _)| id.to_string())?;
            session
                .internal_session_id
                .map(|internal| (agent_id, internal))
        })
        .collect();
    let mut sessions = registry.stored_sessions().await;
    sessions.retain(|session| {
        !known.contains(&(
            session.agent_id.clone().unwrap_or_default(),
            session.internal_session_id.clone(),
        ))
    });
    Ok(sessions)
}
