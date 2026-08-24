//! Call context: who is making this request?
//!
//! Agent-originated requests (via the MCP server) carry `X-Ariadne-Session`;
//! everything else (CLI, curl, web) is the user. The unix socket is the trust
//! boundary — the header is context, not cryptographic auth.

use axum::http::HeaderMap;

use ariadne_api::SESSION_HEADER;
use ariadne_core::{Actor, AuthorRole, Role};
use ariadne_store::{AgentSession, Store};

use super::error::{ApiError, ApiResult};

/// Resolved identity of the caller.
pub struct CallCtx {
    pub actor: Actor,
    pub author_role: AuthorRole,
    /// Present when the call came from an agent session.
    pub session: Option<AgentSession>,
}

impl CallCtx {
    pub fn user() -> Self {
        Self {
            actor: Actor::User,
            author_role: AuthorRole::User,
            session: None,
        }
    }
}

pub async fn call_ctx(store: &Store, headers: &HeaderMap) -> ApiResult<CallCtx> {
    let Some(raw) = headers.get(SESSION_HEADER) else {
        return Ok(CallCtx::user());
    };
    let session_id = raw
        .to_str()
        .map_err(|_| ApiError::bad_request("invalid X-Ariadne-Session header"))?;
    let session = store
        .get_session(session_id)
        .await
        .map_err(|_| ApiError::forbidden(format!("unknown agent session: {session_id}")))?;
    let (actor, author_role) = match session.role() {
        Role::Planner => (Actor::Planner, AuthorRole::Planner),
        Role::Engineer => (Actor::Engineer, AuthorRole::Engineer),
        Role::Reviewer => (Actor::Reviewer, AuthorRole::Reviewer),
    };
    Ok(CallCtx {
        actor,
        author_role,
        session: Some(session),
    })
}

/// Ensure an agent session is scoped to the given task (users pass freely).
pub fn ensure_task_scope(ctx: &CallCtx, task_id: &str) -> ApiResult<()> {
    if let Some(session) = &ctx.session
        && session.task_id.as_deref() != Some(task_id)
        && session.role() != Role::Planner
    {
        return Err(ApiError::forbidden(format!(
            "session {} is not assigned to task {task_id}",
            session.id
        )));
    }
    Ok(())
}
