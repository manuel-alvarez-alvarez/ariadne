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

/// Claude Code has no model-list command; complete its documented aliases
/// and current model ids.
fn claude_models() -> Vec<CompletionCandidate> {
    [
        ("fable", "alias for the latest Fable model"),
        ("opus", "alias for the latest Opus model"),
        ("sonnet", "alias for the latest Sonnet model"),
        ("haiku", "alias for the latest Haiku model"),
        ("claude-fable-5", "Fable 5"),
        ("claude-opus-5", "Opus 5"),
        ("claude-sonnet-5", "Sonnet 5"),
        ("claude-haiku-4-5-20251001", "Haiku 4.5"),
    ]
    .into_iter()
    .map(|(m, help)| candidate(m, format!("claude_code: {help}")))
    .collect()
}

/// Codex has no model-list command either; the configured default in
/// $CODEX_HOME/config.toml is the one authoritative local source.
fn codex_models() -> Vec<CompletionCandidate> {
    let mut out = Vec::new();
    let home = std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".codex")));
    if let Some(config) = home.map(|h| h.join("config.toml"))
        && let Ok(raw) = std::fs::read_to_string(config)
    {
        for line in raw.lines() {
            if let Some(rest) = line.strip_prefix("model =") {
                let model = rest.trim().trim_matches('"');
                if !model.is_empty() {
                    out.push(candidate(model, "codex: configured default".into()));
                }
            }
        }
    }
    out
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
