//! Attach/logs helpers: resolve an Ariadne id to a tmux session and exec.
//!
//! The id is a session, task or goal id: a session id attaches to that exact
//! session, a task or goal id to the session of the wanted role (default
//! engineer for tasks, planner for goals).
//!
//! When no tmux is alive for the id, attach revives the most recent matching
//! session through the daemon (`POST /v1/sessions/{id}/resume`): a fresh tmux
//! is created resuming the same agent conversation, and we attach to that.

use anyhow::{Result, bail};

use ariadne_api::sessions::SessionDto;
use ariadne_client::{Client, ClientError};
use ariadne_core::Role;

/// All sessions (any status) for a query string like `task=<id>` / `goal=<id>`.
async fn sessions_for(client: &Client, query: &str) -> Result<Vec<SessionDto>> {
    client
        .get_json(&format!("/v1/sessions?{query}"))
        .await
        .map_err(Into::into)
}

/// Sessions matching the id, plus the role to attach to: task first (default
/// engineer), then goal (default planner).
async fn candidates(
    client: &Client,
    id: &str,
    role: Option<Role>,
) -> Result<(Vec<SessionDto>, Role)> {
    let sessions = sessions_for(client, &format!("task={id}")).await?;
    if !sessions.is_empty() {
        return Ok((sessions, role.unwrap_or(Role::Engineer)));
    }
    let sessions = sessions_for(client, &format!("goal={id}")).await?;
    Ok((sessions, role.unwrap_or(Role::Planner)))
}

/// True when the tmux session actually exists (the DB may lag: the
/// liveness sweep marks dead sessions every 15s).
fn tmux_alive(name: &str) -> bool {
    std::process::Command::new("tmux")
        .args(["has-session", "-t", name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Find the live tmux session for a task or goal.
pub async fn resolve_tmux(client: &Client, id: &str, role: Option<Role>) -> Result<SessionDto> {
    let (sessions, wanted) = candidates(client, id, role).await?;
    sessions
        .into_iter()
        .find(|s| s.role == wanted && s.status.is_live() && tmux_alive(&s.tmux_session))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no live {} session found for {id} (is the agent running?)",
                wanted.as_str()
            )
        })
}

/// No live tmux: revive the most recent resumable session of the wanted role.
async fn revive(client: &Client, id: &str, role: Option<Role>) -> Result<SessionDto> {
    let (sessions, wanted) = candidates(client, id, role).await?;
    let target = sessions
        .into_iter()
        .rev() // ids are time-sortable: last = most recent
        .find(|s| s.role == wanted && s.internal_session_id.is_some())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no {} session (live or finished) found for {id} that can be resumed",
                wanted.as_str()
            )
        })?;
    eprintln!(
        "no live tmux for {id} — reviving session {} ({})",
        target.id,
        target.agent_kind.as_str()
    );
    client
        .post_empty(&format!("/v1/sessions/{}/resume", target.id))
        .await
        .map_err(Into::into)
}

/// Replace this process with `tmux attach`.
pub fn exec_tmux_attach(tmux_session: &str) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new("tmux")
        .args(["attach", "-t", tmux_session])
        .exec();
    // exec only returns on failure.
    bail!("failed to exec tmux attach -t {tmux_session}: {err}");
}

/// A terminal task whose worktrees were removed has no agents left to attach
/// to — fail with pointers to the history instead of a raw revive conflict.
/// (With the default `delete_merged_worktrees = false` policy the worktree is
/// kept, and reviving the agent to inspect the merged work is allowed.)
async fn ensure_task_not_finished(client: &Client, id: &str) -> Result<()> {
    use ariadne_api::tasks::TaskDto;
    use ariadne_core::TaskStatus;
    if let Ok(task) = client.get_json::<TaskDto>(&format!("/v1/tasks/{id}")).await
        && matches!(task.status, TaskStatus::Merged | TaskStatus::Cancelled)
        && task.worktree_path.is_none()
    {
        bail!(
            "task {id} is {status} — its agents and worktrees have been cleaned up.\n\
             Inspect what happened instead:\n\
             \x20 ariadne task history {id}\n\
             \x20 ariadne task reviews {id}\n\
             \x20 ariadne task messages {id}\n\
             \x20 ariadne session ls --all --task {id}   (then: ariadne session logs <session-id>)",
            status = task.status.as_str()
        );
    }
    Ok(())
}

/// The session with this id, or `None` when the id is not a session one (the
/// caller then tries it as a task or goal id).
async fn session_by_id(client: &Client, id: &str) -> Result<Option<SessionDto>> {
    match client
        .get_json::<SessionDto>(&format!("/v1/sessions/{id}"))
        .await
    {
        Ok(session) => Ok(Some(session)),
        Err(ClientError::Api { status, .. }) if status == http::StatusCode::NOT_FOUND => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Attach to one specific session: its own tmux when alive, else revive it.
/// A worktree that is gone surfaces the daemon's revive error as-is.
async fn attach_session(client: &Client, session: SessionDto) -> Result<()> {
    let session = if session.status.is_live() && tmux_alive(&session.tmux_session) {
        session
    } else {
        eprintln!(
            "no live tmux for {} — reviving it ({})",
            session.id,
            session.agent_kind.as_str()
        );
        client
            .post_empty(&format!("/v1/sessions/{}/resume", session.id))
            .await?
    };
    attach_to(&session)
}

/// Attach to a task or goal id: the live tmux of the wanted role, or the
/// most recent resumable session of that role revived.
pub async fn attach(client: &Client, id: &str, role: Option<Role>) -> Result<()> {
    let session = match resolve_tmux(client, id, role).await {
        Ok(session) => session,
        Err(_) => {
            ensure_task_not_finished(client, id).await?;
            revive(client, id, role).await?
        }
    };
    attach_to(&session)
}

/// `ariadne attach <id>`: session, task or goal id.
pub async fn attach_any(client: &Client, id: &str, role: Option<Role>) -> Result<()> {
    if let Some(session) = session_by_id(client, id).await? {
        if role.is_some() {
            bail!(
                "--role does not apply to a session id: {id} is already the {} session \
                 of that agent (pass the task or goal id to pick a role)",
                session.role.as_str()
            );
        }
        return attach_session(client, session).await;
    }
    attach(client, id, role).await
}

fn attach_to(session: &SessionDto) -> Result<()> {
    eprintln!(
        "attaching to {} ({} / {})",
        session.tmux_session,
        session.role.as_str(),
        session.agent_kind.as_str()
    );
    exec_tmux_attach(&session.tmux_session)
}
