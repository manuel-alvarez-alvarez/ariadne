//! Attach/logs helpers: resolve an Ariadne id to a tmux session and exec.

use anyhow::{Result, bail};

use ariadne_api::sessions::SessionDto;
use ariadne_client::Client;
use ariadne_core::Role;

/// Live sessions for a query string like `task=<id>` / `goal=<id>`.
async fn live_sessions(client: &Client, query: &str) -> Result<Vec<SessionDto>> {
    let sessions: Vec<SessionDto> = client.get_json(&format!("/v1/sessions?{query}")).await?;
    Ok(sessions
        .into_iter()
        .filter(|s| s.status.is_live())
        .collect())
}

/// Find the tmux session for a task (default engineer) or goal (planner).
pub async fn resolve_tmux(client: &Client, id: &str, role: Option<Role>) -> Result<SessionDto> {
    // Try as task first, then as goal.
    let mut candidates = live_sessions(client, &format!("task={id}")).await?;
    let wanted = if candidates.is_empty() {
        candidates = live_sessions(client, &format!("goal={id}")).await?;
        role.unwrap_or(Role::Planner)
    } else {
        role.unwrap_or(Role::Engineer)
    };
    candidates
        .into_iter()
        .find(|s| s.role == wanted)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no live {} session found for {id} (is the agent running?)",
                wanted.as_str()
            )
        })
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

pub async fn attach(client: &Client, id: &str, role: Option<Role>) -> Result<()> {
    let session = resolve_tmux(client, id, role).await?;
    eprintln!(
        "attaching to {} ({} / {})",
        session.tmux_session,
        session.role.as_str(),
        session.agent_kind.as_str()
    );
    exec_tmux_attach(&session.tmux_session)
}

pub fn parse_role(s: &str) -> Result<Role, String> {
    s.parse()
}
