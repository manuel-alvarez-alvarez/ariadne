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
/// plus the model ids recorded in recent session transcripts under
/// ~/.claude/projects (assistant messages carry `"model":"..."`).
fn claude_models() -> Vec<CompletionCandidate> {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<CompletionCandidate> = [
        ("fable", "alias for the latest Fable model"),
        ("opus", "alias for the latest Opus model"),
        ("sonnet", "alias for the latest Sonnet model"),
        ("haiku", "alias for the latest Haiku model"),
    ]
    .into_iter()
    .map(|(m, help)| {
        seen.insert(m.to_string());
        candidate(m, format!("claude_code: {help}"))
    })
    .collect();

    let Some(projects) = dirs::home_dir().map(|h| h.join(".claude").join("projects")) else {
        return out;
    };
    // Newest transcripts first.
    let mut transcripts: Vec<(std::time::SystemTime, std::path::PathBuf)> = Vec::new();
    if let Ok(dirs) = std::fs::read_dir(&projects) {
        for dir in dirs.flatten() {
            if let Ok(files) = std::fs::read_dir(dir.path()) {
                for file in files.flatten() {
                    let path = file.path();
                    if path.extension().is_some_and(|e| e == "jsonl")
                        && let Ok(meta) = file.metadata()
                        && let Ok(modified) = meta.modified()
                    {
                        transcripts.push((modified, path));
                    }
                }
            }
        }
    }
    transcripts.sort_unstable_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    for (_, path) in transcripts.into_iter().take(30) {
        for model in models_in_rollout_head(&path) {
            // Transcripts also contain "<synthetic>" placeholder entries.
            if model.starts_with("claude-") && seen.insert(model.clone()) {
                out.push(candidate(
                    &model,
                    "claude_code: used in recent sessions".into(),
                ));
            }
        }
    }
    out
}

/// Codex has no model-list command, but every session rollout under
/// $CODEX_HOME/sessions records the model it ran with — the configured
/// default plus models seen in recent sessions make an honest local catalog.
fn codex_models() -> Vec<CompletionCandidate> {
    let Some(home) = std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".codex")))
    else {
        return Vec::new();
    };

    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();

    if let Ok(raw) = std::fs::read_to_string(home.join("config.toml")) {
        for line in raw.lines() {
            if let Some(rest) = line.strip_prefix("model =") {
                let model = rest.trim().trim_matches('"');
                if !model.is_empty() && seen.insert(model.to_string()) {
                    out.push(candidate(model, "codex: configured default".into()));
                }
            }
        }
    }

    // Newest rollout files first; the model name appears within the first
    // few KB (session meta / turn context).
    let mut rollouts = Vec::new();
    collect_rollouts(&home.join("sessions"), 0, &mut rollouts);
    rollouts.sort_unstable_by(|a, b| b.cmp(a));
    for path in rollouts.into_iter().take(100) {
        for model in models_in_rollout_head(&path) {
            if seen.insert(model.clone()) {
                out.push(candidate(&model, "codex: used in recent sessions".into()));
            }
        }
    }
    out
}

/// Gather rollout-*.jsonl paths under sessions/YYYY/MM/DD (bounded depth).
fn collect_rollouts(dir: &std::path::Path, depth: u8, out: &mut Vec<std::path::PathBuf>) {
    if depth > 3 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rollouts(&path, depth + 1, out);
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("rollout-") && n.ends_with(".jsonl"))
        {
            out.push(path);
        }
    }
}

/// Extract `"model":"..."` values from the head of a rollout file. The
/// session-meta line carries the full instruction payload before the model
/// field, so the window must be generous (128KB, still trivial to read).
fn models_in_rollout_head(path: &std::path::Path) -> Vec<String> {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut buf = vec![0u8; 128 * 1024];
    let Ok(n) = file.read(&mut buf[..]) else {
        return Vec::new();
    };
    let head = String::from_utf8_lossy(&buf[..n]).into_owned();
    let mut out = Vec::new();
    let needle = "\"model\":\"";
    let mut rest = head.as_str();
    while let Some(i) = rest.find(needle) {
        rest = &rest[i + needle.len()..];
        if let Some(end) = rest.find('"') {
            let model = &rest[..end];
            if !model.is_empty() && model.len() < 64 {
                out.push(model.to_string());
            }
            rest = &rest[end..];
        } else {
            break;
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
