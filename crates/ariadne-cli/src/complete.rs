//! Dynamic shell-completion candidates.
//!
//! Invoked by the completion shim (`COMPLETE=zsh ariadne`) on TAB: each
//! function queries the daemon and returns live candidates with a
//! description column (title/status/role). Fail-safe: a dead daemon just
//! yields no candidates, never an error in the user's shell.

use std::ffi::OsStr;
use std::time::Duration;

use clap_complete::engine::{CompletionCandidate, PathCompleter, ValueCompleter};

use ariadne_client::Client;

/// Completion must be snappy: local unix socket, hard budget.
const BUDGET: Duration = Duration::from_millis(800);

/// Fetch a JSON list from the daemon, blocking (no runtime exists yet:
/// completion is handled before the CLI enters tokio).
fn fetch(path: &str) -> Vec<serde_json::Value> {
    match fetch_value(path) {
        Some(serde_json::Value::Array(items)) => items,
        _ => Vec::new(),
    }
}

/// One JSON document from the daemon, blocking, or nothing at all: a daemon
/// that is down or slow leaves the shell with no candidates, never an error.
fn fetch_value(path: &str) -> Option<serde_json::Value> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    rt.block_on(async {
        let client = Client::from_env();
        match tokio::time::timeout(BUDGET, client.get_json::<serde_json::Value>(path)).await {
            Ok(Ok(value)) => Some(value),
            other => {
                if std::env::var_os("ARIADNE_COMPLETE_DEBUG").is_some() {
                    eprintln!("complete: {path}: {other:?}");
                }
                None
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

/// Session, task and goal ids (top-level `ariadne attach`).
pub fn attach_ids() -> Vec<CompletionCandidate> {
    let mut out = task_candidates();
    out.extend(goal_candidates());
    out.extend(session_candidates());
    out
}

fn session_candidates() -> Vec<CompletionCandidate> {
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

/// Session ids (session subcommands).
pub fn session_ids() -> Vec<CompletionCandidate> {
    session_candidates()
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

/// Engineer profile names (`task create --engineer`).
pub fn engineer_profiles() -> Vec<CompletionCandidate> {
    profiles(Some("engineer"))
}

/// Reviewer profile names (`task create|update --reviewer`).
pub fn reviewer_profiles() -> Vec<CompletionCandidate> {
    profiles(Some("reviewer"))
}

/// Repository ids (repo subcommands).
pub fn repo_ids() -> Vec<CompletionCandidate> {
    fetch("/v1/repositories")
        .iter()
        .map(|r| {
            candidate(
                s(r, "id"),
                format!("{} [{}]", s(r, "path"), s(r, "base_branch")),
            )
        })
        .collect()
}

/// Repos of the goal being created in (`task create <goal> --repo`).
///
/// Only that goal's repos are candidates, so the id has to come off the
/// command line the same way `--model` reads `--agent` from it.
pub fn goal_repos() -> Vec<CompletionCandidate> {
    let Some(goal) = goal_on_the_line() else {
        return Vec::new();
    };
    let Some(goal) = fetch_value(&format!("/v1/goals/{goal}")) else {
        return Vec::new();
    };
    let Some(repos) = goal.get("repos").and_then(|r| r.as_array()) else {
        return Vec::new();
    };
    repos
        .iter()
        .map(|r| {
            candidate(
                s(r, "id"),
                format!("{} [{}]", s(r, "path"), s(r, "base_branch")),
            )
        })
        .collect()
}

/// The goal id typed on a `task create` line: its first ULID-shaped word,
/// which is how ids of every kind are spelled here. Flags and their values
/// cannot be told apart without the parser, and none of them look like this.
fn goal_on_the_line() -> Option<String> {
    let words: Vec<String> = std::env::args().collect();
    let i = words.iter().position(|w| w == "create")?;
    words[i + 1..]
        .iter()
        .find(|w| w.len() == 26 && w.chars().all(|c| c.is_ascii_alphanumeric()))
        .cloned()
}

/// Agent kinds for `profile create --agent`.
pub fn agent_kinds() -> Vec<CompletionCandidate> {
    ariadne_core::AgentKind::ALL
        .into_iter()
        .map(|kind| CompletionCandidate::new(kind.as_str()))
        .collect()
}

/// Prompt kinds for `profile prompt get|set|reset`, plus "system".
///
/// Every kind of every role: which ones a profile actually owns depends on the
/// profile named earlier on the line, and the command says so itself when the
/// two do not match.
pub fn prompt_kinds() -> Vec<CompletionCandidate> {
    let mut out = vec![
        CompletionCandidate::new("system").help(Some("the profile's own system prompt".into())),
    ];
    out.extend(
        ariadne_core::PromptKind::ALL
            .into_iter()
            .map(|kind| candidate(kind.as_str(), format!("{} profiles", kind.role().as_str()))),
    );
    out
}

/// `<kind>=` for `profile create|update --prompt`: only the kind half can be
/// completed — what follows the `=` is the caller's own prose.
pub fn prompt_assignment(current: &OsStr) -> Vec<CompletionCandidate> {
    let current = current.to_string_lossy();
    match current.contains('=') {
        true => Vec::new(),
        false => assignment_kinds(&current),
    }
}

/// `<kind>=<path>` for `profile create|update --prompt-file`: the kind, and
/// then the file it reads from, completed as a path.
pub fn prompt_file_assignment(current: &OsStr) -> Vec<CompletionCandidate> {
    let current = current.to_string_lossy();
    match current.split_once('=') {
        Some((kind, path)) => PathCompleter::file()
            .complete(OsStr::new(path))
            .into_iter()
            .map(|c| c.add_prefix(format!("{kind}=")))
            .collect(),
        None => assignment_kinds(&current),
    }
}

/// The `<kind>=` half of an assignment, filtered by what is typed so far:
/// unlike candidate lists, a completer's answers reach the shell as they are.
fn assignment_kinds(current: &str) -> Vec<CompletionCandidate> {
    prompt_kinds()
        .into_iter()
        .map(|c| {
            let value = format!("{}=", c.get_value().to_string_lossy());
            CompletionCandidate::new(value).help(c.get_help().cloned())
        })
        .filter(|c| c.get_value().to_string_lossy().starts_with(current))
        .collect()
}

/// Agent kinds plus "auto" for `profile update --agent`.
pub fn agent_kinds_or_auto() -> Vec<CompletionCandidate> {
    let mut out = agent_kinds();
    out.push(
        CompletionCandidate::new("auto").help(Some("first installed CLI at spawn time".into())),
    );
    out
}

/// Model candidates for `--model`, scoped to the agent in play: an explicit
/// `--agent` earlier on the line wins; otherwise, when updating an existing
/// profile, its stored agent kind; otherwise the union of all agents.
///
/// The catalog comes from the daemon (`GET /v1/models`, the list the UI
/// offers, opencode discovery included) when it answers within the budget;
/// a daemon that is down or slow leaves the compiled-in curated lists.
pub fn models() -> Vec<CompletionCandidate> {
    use ariadne_core::AgentKind;
    let hint = agent_hint();
    if let Some(from_daemon) = daemon_models(hint) {
        return from_daemon;
    }
    match hint {
        Some(AgentKind::ClaudeCode) => claude_models(),
        Some(AgentKind::Codex) => codex_models(),
        Some(AgentKind::Opencode) => opencode_models(),
        None => {
            let mut out = claude_models();
            out.extend(codex_models());
            out.extend(opencode_models());
            out
        }
    }
}

/// The daemon's model catalog, or nothing at all when it will not answer —
/// telling "no daemon" from "a daemon with no models" so only the former
/// falls back to the curated lists.
fn daemon_models(kind: Option<ariadne_core::AgentKind>) -> Option<Vec<CompletionCandidate>> {
    let path = match kind {
        Some(k) => format!("/v1/models?agent={}", k.as_str()),
        None => "/v1/models".to_string(),
    };
    let models = match fetch_value(&path)? {
        serde_json::Value::Array(models) => models,
        _ => return None,
    };
    Some(
        models
            .iter()
            .map(|m| {
                let agent = s(m, "agent_kind");
                let help = match m.get("description").and_then(|d| d.as_str()) {
                    Some(d) => format!("{agent} — {d}"),
                    None => agent.to_string(),
                };
                candidate(s(m, "id"), help)
            })
            .collect(),
    )
}

/// The completion request carries the full command line (after `--`):
/// scan it for context instead of guessing.
fn agent_hint() -> Option<ariadne_core::AgentKind> {
    let words: Vec<String> = std::env::args().collect();
    // Explicit --agent on the line.
    if let Some(i) = words.iter().position(|w| w == "--agent")
        && let Some(value) = words.get(i + 1)
        && let Ok(kind) = value.replace('-', "_").parse()
    {
        return Some(kind);
    }
    // `profile update <name>`: the profile's stored agent kind.
    let i = words.iter().position(|w| w == "update")?;
    let name = words.get(i + 1).filter(|w| !w.starts_with('-'))?;
    let profile = {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()?;
        rt.block_on(async {
            let client = Client::from_env();
            tokio::time::timeout(
                BUDGET,
                client.get_json::<serde_json::Value>(&format!("/v1/profiles/{name}")),
            )
            .await
            .ok()?
            .ok()
        })?
    };
    profile.get("agent_kind")?.as_str()?.parse().ok()
}

/// Claude Code model catalog (curated in ariadne-core, no discovery).
fn claude_models() -> Vec<CompletionCandidate> {
    curated_models(ariadne_core::AgentKind::ClaudeCode)
}

/// Codex model catalog (curated in ariadne-core, no discovery).
fn codex_models() -> Vec<CompletionCandidate> {
    curated_models(ariadne_core::AgentKind::Codex)
}

fn curated_models(kind: ariadne_core::AgentKind) -> Vec<CompletionCandidate> {
    ariadne_core::models::curated_models(kind)
        .iter()
        .map(|m| candidate(m.id, format!("{} — {}", kind.as_str(), m.description)))
        .collect()
}

/// OpenCode lists its models natively (`opencode models`, provider/model).
fn opencode_models() -> Vec<CompletionCandidate> {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return Vec::new();
    };
    let output = rt.block_on(async {
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            tokio::process::Command::new("opencode")
                .arg("models")
                .kill_on_drop(true)
                .output(),
        )
        .await
        .ok()?
        .ok()
    });
    let Some(output) = output else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && l.contains('/'))
        .map(|m| candidate(m, "opencode".into()))
        .collect()
}
