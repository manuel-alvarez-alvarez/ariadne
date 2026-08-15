//! Dynamic shell-completion candidates.
//!
//! Invoked by the completion shim (`COMPLETE=zsh ariadne`) on TAB: each
//! function queries the daemon and returns live candidates with a
//! description column (title/status/role). Fail-safe: a dead daemon just
//! yields no candidates, never an error in the user's shell.

use std::time::Duration;

use clap_complete::engine::CompletionCandidate;

use ariadne_client::Client;

/// Completion must be snappy: local unix socket, hard budget.
const BUDGET: Duration = Duration::from_millis(800);

/// Fetch a JSON list from the daemon, blocking (no runtime exists yet:
/// completion is handled before the CLI enters tokio).
fn fetch(path: &str) -> Vec<serde_json::Value> {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return Vec::new();
    };
    rt.block_on(async {
        let client = Client::from_env();
        match tokio::time::timeout(BUDGET, client.get_json::<serde_json::Value>(path)).await {
            Ok(Ok(serde_json::Value::Array(items))) => items,
            other => {
                if std::env::var_os("ARIADNE_COMPLETE_DEBUG").is_some() {
                    eprintln!("complete: {path}: {other:?}");
                }
                Vec::new()
            }
        }
    })
}

fn s<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("")
}

fn candidate(value: &str, help: String) -> CompletionCandidate {
    CompletionCandidate::new(value).help(Some(help.into()))
}

fn task_candidates() -> Vec<CompletionCandidate> {
    fetch("/v1/tasks")
        .iter()
        .map(|t| {
            candidate(
                s(t, "id"),
                format!("[{}] {}", s(t, "status"), s(t, "title")),
            )
        })
        .collect()
}

fn goal_candidates() -> Vec<CompletionCandidate> {
    fetch("/v1/goals")
        .iter()
        .map(|g| {
            candidate(
                s(g, "id"),
                format!("[{}] {}", s(g, "status"), s(g, "title")),
            )
        })
        .collect()
}

/// Task ids (task subcommands).
pub fn task_ids() -> Vec<CompletionCandidate> {
    task_candidates()
}

/// Goal ids (goal subcommands and --goal filters).
pub fn goal_ids() -> Vec<CompletionCandidate> {
    goal_candidates()
}

/// Task and goal ids (top-level `ariadne attach`).
pub fn attach_ids() -> Vec<CompletionCandidate> {
    let mut out = task_candidates();
    out.extend(goal_candidates());
    out
}

/// Session ids (session subcommands).
pub fn session_ids() -> Vec<CompletionCandidate> {
    fetch("/v1/sessions")
        .iter()
        .map(|x| {
            candidate(
                s(x, "id"),
                format!(
                    "[{}] {} {}",
                    s(x, "status"),
                    s(x, "role"),
                    s(x, "agent_kind")
                ),
            )
        })
        .collect()
}

fn profiles(role: Option<&str>) -> Vec<CompletionCandidate> {
    let path = match role {
        Some(r) => format!("/v1/profiles?role={r}"),
        None => "/v1/profiles".to_string(),
    };
    fetch(&path)
        .iter()
        .map(|p| {
            let agent = p
                .get("agent_kind")
                .and_then(|a| a.as_str())
                .unwrap_or("auto");
            candidate(s(p, "name"), format!("{} ({agent})", s(p, "role")))
        })
        .collect()
}

/// Profile names, any role (profile subcommands).
pub fn profile_names() -> Vec<CompletionCandidate> {
    profiles(None)
}

/// Planner profile names (`goal create --planner`).
pub fn planner_profiles() -> Vec<CompletionCandidate> {
    profiles(Some("planner"))
}

/// Roles accepted by `attach --role` and friends (static, but keeps the
/// value completable).
pub fn roles() -> Vec<CompletionCandidate> {
    ["planner", "engineer", "reviewer"]
        .into_iter()
        .map(CompletionCandidate::new)
        .collect()
}
