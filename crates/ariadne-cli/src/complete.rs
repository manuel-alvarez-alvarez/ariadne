//! Dynamic shell-completion candidates.
//!
//! Invoked by the completion shim (`COMPLETE=zsh ariadne`) on TAB. Fail-safe
//! throughout: a daemon that is down or slow leaves the shell with no
//! candidates, never an error — and no runtime exists yet, so every lookup
//! blocks on one of its own.

use std::ffi::OsStr;
use std::time::Duration;

use clap_complete::engine::{CompletionCandidate, PathCompleter, ValueCompleter};

use ariadne_client::Client;
use ariadne_core::AgentKind;
use ariadne_core::models::ModelRef;

/// Completion must be snappy: local unix socket, hard budget.
const BUDGET: Duration = Duration::from_millis(800);

/// One JSON document from the daemon, or nothing at all.
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

/// A JSON list from the daemon; anything else is no candidates.
fn fetch(path: &str) -> Vec<serde_json::Value> {
    match fetch_value(path) {
        Some(serde_json::Value::Array(items)) => items,
        _ => Vec::new(),
    }
}

fn s<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("")
}

fn candidate(value: &str, help: String) -> CompletionCandidate {
    CompletionCandidate::new(value).help(Some(help.into()))
}

/// Every row of `path` as a candidate on its id, described by `help`.
fn by_id(path: &str, help: impl Fn(&serde_json::Value) -> String) -> Vec<CompletionCandidate> {
    fetch(path)
        .iter()
        .map(|row| candidate(s(row, "id"), help(row)))
        .collect()
}

/// Task ids (task subcommands).
pub fn task_ids() -> Vec<CompletionCandidate> {
    by_id("/v1/tasks", |t| {
        format!("[{}] {}", s(t, "status"), s(t, "title"))
    })
}

/// Goal ids (goal subcommands and --goal filters).
pub fn goal_ids() -> Vec<CompletionCandidate> {
    by_id("/v1/goals", |g| {
        format!("[{}] {}", s(g, "status"), s(g, "title"))
    })
}

/// Session ids (session subcommands).
pub fn session_ids() -> Vec<CompletionCandidate> {
    by_id("/v1/sessions", |x| {
        format!(
            "[{}] {} {}",
            s(x, "status"),
            s(x, "role"),
            s(x, "agent_kind")
        )
    })
}

/// Session, task and goal ids (top-level `ariadne attach`).
pub fn attach_ids() -> Vec<CompletionCandidate> {
    let mut out = task_ids();
    out.extend(goal_ids());
    out.extend(session_ids());
    out
}

fn profiles(role: Option<&str>) -> Vec<CompletionCandidate> {
    let path = match role {
        Some(r) => format!("/v1/profiles?role={r}"),
        None => "/v1/profiles".to_string(),
    };
    fetch(&path)
        .iter()
        .map(|p| {
            let model = p.get("model").and_then(|m| m.as_str()).unwrap_or("auto");
            candidate(s(p, "name"), format!("{} ({model})", s(p, "role")))
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

/// Whom a goal's planning thread can address (`goal msg --to`): its planner,
/// or the user.
pub fn goal_message_recipients() -> Vec<CompletionCandidate> {
    message_recipients(Some("planner"))
}

/// Whom a task's thread can address (`task msg --to`): anyone working on the
/// task — engineer, reviewers, planner — or the user.
pub fn task_message_recipients() -> Vec<CompletionCandidate> {
    message_recipients(None)
}

fn message_recipients(role: Option<&str>) -> Vec<CompletionCandidate> {
    let mut out = profiles(role);
    out.push(CompletionCandidate::new("user").help(Some("the human user".into())));
    out
}

/// Registered repository ids (repo subcommands, `goal create --repo`).
pub fn repo_ids() -> Vec<CompletionCandidate> {
    fetch("/v1/repositories").iter().map(repository).collect()
}

/// Repositories of the goal being created in (`task create <goal> --repo`).
///
/// Only that goal's repositories are candidates, so the id has to come off the
/// command line, where the goal was named as a positional.
pub fn goal_repositories() -> Vec<CompletionCandidate> {
    let Some(goal) = ulid_after("create") else {
        return Vec::new();
    };
    fetch_value(&format!("/v1/goals/{goal}"))
        .as_ref()
        .and_then(|g| g.get("repos")?.as_array())
        .map(|repos| repos.iter().map(repository).collect())
        .unwrap_or_default()
}

/// One repository as a candidate: the id, described by the checkout it stands
/// for and — when it has one — what it was registered as.
fn repository(r: &serde_json::Value) -> CompletionCandidate {
    let head = format!("{} [{}]", s(r, "path"), s(r, "base_branch"));
    let help = match r.get("description").and_then(|d| d.as_str()) {
        Some(d) if !d.trim().is_empty() => format!("{head} — {d}"),
        _ => head,
    };
    candidate(s(r, "id"), help)
}

/// The first ULID-shaped word after `verb` on the line being completed, which
/// is how ids of every kind are spelled here. Flags and their values cannot be
/// told apart without the parser, and none of them look like this.
fn ulid_after(verb: &str) -> Option<String> {
    let words: Vec<String> = std::env::args().collect();
    let i = words.iter().position(|w| w == verb)?;
    words[i + 1..]
        .iter()
        .find(|w| w.len() == 26 && w.chars().all(|c| c.is_ascii_alphanumeric()))
        .cloned()
}

/// The agent CLIs, for `ariadne agent update <kind>` — the one place left
/// where an agent CLI is named on its own, since what an agent *runs on* is
/// chosen as a whole model (`--model`).
pub fn agent_kinds() -> Vec<CompletionCandidate> {
    AgentKind::ALL
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
    out.extend(ariadne_core::PromptKind::ALL.into_iter().map(|kind| {
        let owners = kind
            .roles()
            .iter()
            .map(|role| role.as_str())
            .collect::<Vec<_>>()
            .join("/");
        candidate(kind.as_str(), format!("{owners} profiles"))
    }));
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

/// Model candidates for `--model`: everything an agent can be pinned to, in
/// the one spelling that pins it — `<agent_kind>[:<model>]`, the bare agent
/// CLI included, which is that CLI on its own default model.
///
/// The catalog comes from the daemon (`GET /v1/models`, the list the UI
/// offers, opencode discovery included) when it answers within the budget; a
/// daemon that is down or slow leaves the compiled-in curated lists, each
/// entry qualified here the way the daemon qualifies it, and opencode's own
/// `opencode models` for the one agent that lists them itself.
pub fn models() -> Vec<CompletionCandidate> {
    daemon_models().unwrap_or_else(|| {
        AgentKind::ALL
            .into_iter()
            .flat_map(|kind| {
                let mut out = vec![candidate(
                    kind.as_str(),
                    format!("{} on its own default model", kind.as_str()),
                )];
                out.extend(match kind {
                    AgentKind::Opencode => opencode_models(),
                    _ => curated_models(kind),
                });
                out
            })
            .collect()
    })
}

/// The same, plus the word an update writes to pin nothing at all: `task
/// update --model` and `profile update --model`.
pub fn models_or_default() -> Vec<CompletionCandidate> {
    let mut out = models();
    out.push(
        CompletionCandidate::new(crate::commands::DEFAULT)
            .help(Some("pin nothing: run on whatever the profile is on".into())),
    );
    out
}

/// The daemon's model catalog, or nothing at all when it will not answer —
/// telling "no daemon" from "a daemon with no models" so only the former
/// falls back to the curated lists.
fn daemon_models() -> Option<Vec<CompletionCandidate>> {
    let serde_json::Value::Array(models) = fetch_value("/v1/models")? else {
        return None;
    };
    Some(
        models
            .iter()
            .map(|m| {
                let help = match m.get("description").and_then(|d| d.as_str()) {
                    Some(d) => d.to_string(),
                    None => s(m, "agent_kind").to_string(),
                };
                candidate(s(m, "id"), help)
            })
            .collect(),
    )
}

/// A curated catalog from ariadne-core, for the agents that discover nothing:
/// each id under the CLI that runs it, which is how it is pinned.
fn curated_models(kind: AgentKind) -> Vec<CompletionCandidate> {
    ariadne_core::models::curated_models(kind)
        .iter()
        .map(|m| candidate(&qualified(kind, m.id), m.description.to_string()))
        .collect()
}

/// One model as it is pinned: the CLI that runs it, then the model.
fn qualified(kind: AgentKind, model: &str) -> String {
    ModelRef {
        agent_kind: kind,
        model: Some(model.to_string()),
    }
    .to_string()
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
            Duration::from_secs(3),
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
        .map(|m| candidate(&qualified(AgentKind::Opencode, m), "opencode".into()))
        .collect()
}
