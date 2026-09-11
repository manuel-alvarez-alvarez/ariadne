//! Attach/logs helpers: resolve an Ariadne id to a session's console.
//!
//! The id is a session, task or goal id — a task or goal one resolves to the
//! session of the wanted seat (default author for tasks, orchestrator for
//! goals). With no live session for it, attach revives the most recent
//! matching session (`POST /v1/sessions/{id}/resume`) and attaches to the
//! fresh agent that resumes the same conversation.

use anyhow::{Result, bail};

use ariadne_api::sessions::SessionDto;
use ariadne_client::{Client, ClientError};
use ariadne_core::Seat;
use ariadne_core::models::agent_of;

use crate::output::{style, view};

/// A hint about what this command is doing on the caller's behalf — reviving
/// a session, attaching to one — never the payload itself, so it is dimmed
/// the way every other line that is context rather than content is.
fn hint(message: &str) -> String {
    style::paint(view().color, style::META, message)
}

/// Sessions matching the id, plus the seat to attach to: task first (default
/// author), then goal (default orchestrator).
///
/// With no sessions on either side the id itself decides the wording — a task
/// without sessions used to be reported as a missing *orchestrator* session,
/// and an id naming nothing at all got the same message as a real task.
async fn candidates(
    client: &Client,
    id: &str,
    seat: Option<Seat>,
) -> Result<(Vec<SessionDto>, Seat)> {
    for (query, default) in [("task", Seat::Author), ("goal", Seat::Orchestrator)] {
        let sessions: Vec<SessionDto> = client
            .get_json(&format!("/v1/sessions?{query}={id}"))
            .await?;
        if !sessions.is_empty() {
            return Ok((sessions, seat.unwrap_or(default)));
        }
    }
    for (kind, default) in [("tasks", Seat::Author), ("goals", Seat::Orchestrator)] {
        if found::<serde_json::Value>(client, &format!("/v1/{kind}/{id}"))
            .await?
            .is_some()
        {
            return Ok((vec![], seat.unwrap_or(default)));
        }
    }
    bail!("no such task, goal or session: {id}")
}

/// What a GET on `path` answers with, or nothing at all when it 404s.
async fn found<T: serde::de::DeserializeOwned>(client: &Client, path: &str) -> Result<Option<T>> {
    match client.get_json::<T>(path).await {
        Ok(value) => Ok(Some(value)),
        Err(ClientError::Api { status, .. }) if status == http::StatusCode::NOT_FOUND => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Find the live session for a task or goal seat. Its persisted status is
/// the liveness check: the daemon's runtime owns the agent, and the liveness
/// sweep keeps the row honest.
pub async fn resolve_live(client: &Client, id: &str, seat: Option<Seat>) -> Result<SessionDto> {
    let (sessions, wanted) = candidates(client, id, seat).await?;
    sessions
        .into_iter()
        .find(|s| s.seat == wanted && s.status.is_live())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no live {} session found for {id} (is the agent running?)",
                wanted.as_str()
            )
        })
}

/// No live session: revive the most recent resumable session of the wanted
/// seat.
async fn revive(client: &Client, id: &str, seat: Option<Seat>) -> Result<SessionDto> {
    let (sessions, wanted) = candidates(client, id, seat).await?;
    let target = sessions
        .into_iter()
        .rev() // ids are time-sortable: last = most recent
        .find(|s| s.seat == wanted && s.internal_session_id.is_some())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no {} session (live or finished) found for {id} that can be resumed",
                wanted.as_str()
            )
        })?;
    eprintln!(
        "{}",
        hint(&format!(
            "no live session for {id} — reviving session {} ({})",
            target.id,
            agent_of(&target.model)
        ))
    );
    client
        .post_empty(&format!("/v1/sessions/{}/resume", target.id))
        .await
        .map_err(Into::into)
}

/// A terminal task whose worktrees were removed — the normal end of a merged
/// task, since `delete_merged_worktrees` defaults to true — has no agents left
/// to attach to: fail with pointers to the history instead of a raw revive
/// conflict. With the policy off the worktree is kept and a revive is allowed.
async fn ensure_task_not_finished(client: &Client, id: &str) -> Result<()> {
    use ariadne_api::tasks::TaskDto;
    use ariadne_core::TaskStatus;
    if let Ok(task) = client.get_json::<TaskDto>(&format!("/v1/tasks/{id}")).await
        && matches!(task.status, TaskStatus::Finished | TaskStatus::Cancelled)
        && task.worktree_path.is_none()
    {
        return Err(crate::error::Failure::conflict(format!(
            "task {id} is {status} — its agents and worktrees have been cleaned up",
            status = task.status.as_str()
        ))
        .hint(format!(
            "inspect with: ariadne task history {id}; ariadne task messages {id}; ariadne \
             session ls --all --task {id} (then: ariadne session logs <session-id>)"
        ))
        .err());
    }
    Ok(())
}

/// Attach to one specific session: its console when it is live, else revive
/// it first.
async fn attach_session(client: &Client, session: SessionDto) -> Result<()> {
    let session = if session.status.is_live() {
        session
    } else {
        eprintln!(
            "{}",
            hint(&format!(
                "session {} has ended — reviving it ({})",
                session.id,
                agent_of(&session.model)
            ))
        );
        client
            .post_empty(&format!("/v1/sessions/{}/resume", session.id))
            .await?
    };
    attach_to(client, &session).await
}

/// Attach to a task or goal id: the live session of the wanted seat, or the
/// most recent resumable session of that seat revived.
pub async fn attach(client: &Client, id: &str, seat: Option<Seat>) -> Result<()> {
    let session = match resolve_live(client, id, seat).await {
        Ok(session) => session,
        Err(_) => {
            ensure_task_not_finished(client, id).await?;
            revive(client, id, seat).await?
        }
    };
    attach_to(client, &session).await
}

/// `ariadne attach <id>`: session, task or goal id.
pub async fn attach_any(client: &Client, id: &str, seat: Option<Seat>) -> Result<()> {
    // Which of the three it is decides everything below, so a short id that
    // names one of each is refused here rather than resolved to whichever
    // list happens to be probed first.
    let id = &crate::commands::resolve::attachable(client, id).await?;
    if let Some(session) = found::<SessionDto>(client, &format!("/v1/sessions/{id}")).await? {
        if seat.is_some() {
            bail!(
                "--seat does not apply to a session id: {id} is already the {} session of that agent \
                 (pass the task or goal id to pick a seat)",
                session.seat.as_str()
            );
        }
        return attach_session(client, session).await;
    }
    attach(client, id, seat).await
}

async fn attach_to(client: &Client, session: &SessionDto) -> Result<()> {
    eprintln!(
        "{}",
        hint(&format!(
            "attaching to the console ({} / {})",
            session.seat.as_str(),
            agent_of(&session.model)
        ))
    );
    crate::commands::console::attach(client, &session.id).await
}
