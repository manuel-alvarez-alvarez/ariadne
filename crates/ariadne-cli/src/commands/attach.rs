//! Attach/logs helpers: resolve an Ariadne id to a tmux session and exec.
//!
//! The id is a session, task or goal id — a task or goal one resolves to the
//! session of the wanted role (default engineer for tasks, planner for goals).
//! With no tmux alive for it, attach revives the most recent matching session
//! (`POST /v1/sessions/{id}/resume`) and attaches to the fresh tmux that
//! resumes the same agent conversation.

use anyhow::{Result, bail};

use ariadne_api::sessions::SessionDto;
use ariadne_client::{Client, ClientError};
use ariadne_core::Role;

/// Sessions matching the id, plus the role to attach to: task first (default
/// engineer), then goal (default planner).
///
/// With no sessions on either side the id itself decides the wording — a task
/// without sessions used to be reported as a missing *planner* session, and an
/// id naming nothing at all got the same message as a real task.
async fn candidates(
    client: &Client,
    id: &str,
    role: Option<Role>,
) -> Result<(Vec<SessionDto>, Role)> {
    for (query, default) in [("task", Role::Engineer), ("goal", Role::Planner)] {
        let sessions: Vec<SessionDto> = client
            .get_json(&format!("/v1/sessions?{query}={id}"))
            .await?;
        if !sessions.is_empty() {
            return Ok((sessions, role.unwrap_or(default)));
        }
    }
    for (kind, default) in [("tasks", Role::Engineer), ("goals", Role::Planner)] {
        if found::<serde_json::Value>(client, &format!("/v1/{kind}/{id}"))
            .await?
            .is_some()
        {
            return Ok((vec![], role.unwrap_or(default)));
        }
    }
    bail!("no such task, goal or session: {id}")
}

/// What a GET on `path` answers with, or nothing at all when it 404s.
async fn found<T: serde::de::DeserializeOwned>(
    client: &Client,
    path: &str,
) -> Result<Option<T>> {
    match client.get_json::<T>(path).await {
        Ok(value) => Ok(Some(value)),
        Err(ClientError::Api { status, .. }) if status == http::StatusCode::NOT_FOUND => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Whether the tmux session actually exists — the database may lag it by up
/// to the 15s liveness sweep.
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

/// A terminal task whose worktrees were removed — the normal end of a merged
/// task, since `delete_merged_worktrees` defaults to true — has no agents left
/// to attach to: fail with pointers to the history instead of a raw revive
/// conflict. With the policy off the worktree is kept and a revive is allowed.
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
             \x20 ariadne task thread {id}\n\
             \x20 ariadne session ls --all --task {id}   (then: ariadne session logs <session-id>)",
            status = task.status.as_str()
        );
    }
    Ok(())
}

/// Attach to one specific session: its own tmux when alive, else revive it.
/// The tmux itself decides — the persisted status can be stale either way, and
/// the daemon's resume treats tmux existence as authoritative too.
async fn attach_session(client: &Client, session: SessionDto) -> Result<()> {
    let session = if tmux_alive(&session.tmux_session) {
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
    // Which of the three it is decides everything below, so a short id that
    // names one of each is refused here rather than resolved to whichever
    // list happens to be probed first.
    let id = &crate::commands::resolve::attachable(client, id).await?;
    if let Some(session) = found::<SessionDto>(client, &format!("/v1/sessions/{id}")).await? {
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
