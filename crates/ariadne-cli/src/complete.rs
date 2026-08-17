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
pub fn models() -> Vec<CompletionCandidate> {
    use ariadne_core::AgentKind;
    match agent_hint() {
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

/// Claude Code model catalog (curated, no discovery).
fn claude_models() -> Vec<CompletionCandidate> {
    [
        ("claude-fable-5", "Frontier: highest capability; intricate multi-step agentic workflows and full SDLC loops"),
        ("claude-mythos-5", "Frontier: specialized high-end reasoning within secure configurations"),
        ("claude-opus-5", "Opus tier: massive contextual analysis, legal boilerplate logic, deep math/science"),
        ("claude-opus-4-7", "Opus tier: pinned version for multi-file architecture refactoring and complex bugs"),
        ("claude-sonnet-4-8", "Sonnet tier: production sweet spot; high speed with near-Opus engineering capability"),
        ("claude-sonnet-4-6", "Sonnet tier: legacy production staple; quick diagnostics, lower task latency"),
        ("claude-haiku-4-5", "Haiku tier: ultra-fast; inline completions, text touch-ups, shell command generation"),
    ]
    .into_iter()
    .map(|(m, help)| candidate(m, format!("claude_code — {help}")))
    .collect()
}

/// Codex model catalog (curated, no discovery).
fn codex_models() -> Vec<CompletionCandidate> {
    [
        (
            "gpt-5.6-sol",
            "Frontier reasoning: multi-step agentic loops, codebase-wide architecture planning",
        ),
        (
            "gpt-5.5-sol",
            "Frontier reasoning: deep logic reasoning, long-horizon bug resolution",
        ),
        (
            "gpt-5.3-codex",
            "Codex developer: default enterprise LTS; optimal for active engineering contexts",
        ),
        (
            "gpt-5.2-codex",
            "Codex developer: native context compaction; single/multi-file refactoring",
        ),
        (
            "codex-mini-latest",
            "Low latency: ultra-fast inline edits, quick explanation cards, shell tasks",
        ),
        (
            "o4-mini",
            "Diagnostic: low-latency diagnostic tracking and fast debugging runs",
        ),
        (
            "o3",
            "Diagnostic: classic reasoning checkups and quick inline completion blocks",
        ),
        ("gpt-5.6-terra", "Balanced: high-volume execution layer"),
        (
            "gpt-5.6-luna",
            "Balanced: cost-effective subagent processing queues",
        ),
    ]
    .into_iter()
    .map(|(m, help)| candidate(m, format!("codex — {help}")))
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
