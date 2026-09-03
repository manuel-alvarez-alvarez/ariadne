//! Dynamic shell-completion candidates.
//!
//! Invoked by the completion shim (`COMPLETE=zsh ariadne`) on TAB. Fail-safe
//! throughout: a daemon that is down or slow leaves the shell with no
//! candidates, never an error — and no runtime exists yet, so the lookups
//! block on one of their own.
//!
//! Candidates are verb-aware: what `task retry` offers is not what `task
//! cancel` offers, because the one thing a completion must not do is propose
//! an id the command is about to refuse. Where the daemon filters by status
//! itself the query says so; where it takes one status and the verb wants
//! several, the narrowing happens here.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::OnceLock;
use std::time::Duration;

use clap_complete::engine::CompletionCandidate;
use serde_json::Value;

use ariadne_client::{Client, endpoint};
use ariadne_core::models::ModelRef;
use ariadne_core::{AgentKind, SessionStatus, TaskStatus};

/// Completion must be snappy: local unix socket, hard budget for the whole
/// invocation however many endpoints it reads.
const BUDGET: Duration = Duration::from_millis(800);

/// The exception, for the model catalog alone. `GET /v1/models` asks the
/// agent CLIs what they can run, which takes seconds rather than
/// milliseconds — and it is paid at most once per [`MODELS_TTL`], because
/// what comes back is written to disk.
const CATALOG_BUDGET: Duration = Duration::from_secs(5);

/// How long a written model catalog answers `--model` on its own. Models move
/// when an agent CLI is upgraded or a provider key appears, which is rare
/// next to how often TAB is pressed, so a quarter of an hour of staleness
/// buys every press but the first an instant answer.
const MODELS_TTL: Duration = Duration::from_secs(900);

/// The one runtime this process gets. Completion functions are called by
/// clap, several of them on a single invocation for a single argument, and
/// each was building — and dropping — a runtime of its own.
fn runtime() -> Option<&'static tokio::runtime::Runtime> {
    static RUNTIME: OnceLock<Option<tokio::runtime::Runtime>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .ok()
        })
        .as_ref()
}

/// One round of daemon lookups, under the completion budget. `None` is "ask
/// the shell to show nothing": no runtime, or an answer that did not arrive
/// in time.
fn round<T>(work: impl Future<Output = T>) -> Option<T> {
    round_within(BUDGET, work)
}

fn round_within<T>(budget: Duration, work: impl Future<Output = T>) -> Option<T> {
    let rt = runtime()?;
    rt.block_on(async {
        match tokio::time::timeout(budget, work).await {
            Ok(value) => Some(value),
            Err(_) => {
                debug("timed out", &budget);
                None
            }
        }
    })
}

fn debug(what: &str, detail: &impl std::fmt::Debug) {
    if std::env::var_os("ARIADNE_COMPLETE_DEBUG").is_some() {
        eprintln!("complete: {what}: {detail:?}");
    }
}

/// One JSON document from the daemon, or nothing at all.
async fn get(client: &Client, path: &str) -> Option<Value> {
    match client.get_json::<Value>(path).await {
        Ok(value) => Some(value),
        Err(e) => {
            debug(path, &e);
            None
        }
    }
}

/// One JSON document from the daemon, fetched on its own round.
fn fetch_value(path: &str) -> Option<Value> {
    round(async { get(&Client::from_env(), path).await }).flatten()
}

/// A JSON list from the daemon; anything else is no candidates.
fn fetch(path: &str) -> Vec<Value> {
    rows(fetch_value(path))
}

fn rows(value: Option<Value>) -> Vec<Value> {
    match value {
        Some(Value::Array(items)) => items,
        _ => Vec::new(),
    }
}

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("")
}

/// The status a row is in, in the daemon's own spelling. A row whose status
/// does not parse is left out of every filter rather than guessed at.
fn status<T: FromStr>(row: &Value) -> Option<T> {
    s(row, "status").parse().ok()
}

fn candidate(value: &str, help: String) -> CompletionCandidate {
    CompletionCandidate::new(value).help(Some(help.into()))
}

/// Rows as candidates on their ids, newest first: ids are ULIDs and the
/// daemon lists them oldest first, so newest-first is that list reversed.
fn ids(rows: &[Value], help: impl Fn(&Value) -> String) -> Vec<CompletionCandidate> {
    rows.iter()
        .rev()
        .map(|row| candidate(s(row, "id"), help(row)))
        .collect()
}

/// Every row of `path` that `keep` accepts, newest first, on its id.
fn by_id(
    path: &str,
    keep: fn(&Value) -> bool,
    help: impl Fn(&Value) -> String,
) -> Vec<CompletionCandidate> {
    let rows: Vec<Value> = fetch(path).into_iter().filter(keep).collect();
    ids(&rows, help)
}

fn anything(_: &Value) -> bool {
    true
}

// ---- tasks ---------------------------------------------------------------

fn task_help(t: &Value) -> String {
    format!("[{}] {}", s(t, "status"), s(t, "title"))
}

/// Task ids, newest first (task subcommands and `--depends-on`).
pub fn task_ids() -> Vec<CompletionCandidate> {
    by_id("/v1/tasks", anything, task_help)
}

/// What `task retry` can act on: the failed ones, the only status it takes.
/// `GET /v1/tasks` filters by one status, and this is one.
pub fn retryable_task_ids() -> Vec<CompletionCandidate> {
    by_id("/v1/tasks?status=failed", anything, task_help)
}

/// What `task cancel` can act on: everything that has not already merged or
/// been cancelled. Seven statuses against a query that takes one, so the
/// narrowing is here.
pub fn cancellable_task_ids() -> Vec<CompletionCandidate> {
    by_id("/v1/tasks", task_is_open, task_help)
}

fn task_is_open(row: &Value) -> bool {
    status::<TaskStatus>(row).is_some_and(|s| !s.is_terminal())
}

// ---- goals ---------------------------------------------------------------

fn goal_help(g: &Value) -> String {
    format!("[{}] {}", s(g, "status"), s(g, "title"))
}

/// Goal ids, newest first (goal subcommands and `--goal` filters).
pub fn goal_ids() -> Vec<CompletionCandidate> {
    by_id("/v1/goals", anything, goal_help)
}

/// What `goal cancel` can act on: a goal still under way. `GET /v1/goals`
/// takes as many statuses as we care to name, so the daemon does the filtering.
pub fn cancellable_goal_ids() -> Vec<CompletionCandidate> {
    by_id("/v1/goals?status=planning,active", anything, goal_help)
}

/// What `goal rm` can act on: a finished goal, the only kind it will delete.
pub fn deletable_goal_ids() -> Vec<CompletionCandidate> {
    by_id("/v1/goals?status=completed,cancelled", anything, goal_help)
}

// ---- sessions ------------------------------------------------------------

fn session_help(x: &Value) -> String {
    format!(
        "[{}] {} {}",
        s(x, "status"),
        s(x, "role"),
        s(x, "agent_kind")
    )
}

/// Session ids, newest first (`session inspect`, `session logs`).
pub fn session_ids() -> Vec<CompletionCandidate> {
    by_id("/v1/sessions", anything, session_help)
}

/// What `session kill` can act on: a session with a tmux process to kill.
/// Three statuses are live against a query that takes one, so the narrowing
/// is here.
pub fn live_session_ids() -> Vec<CompletionCandidate> {
    by_id("/v1/sessions", session_is_live, session_help)
}

/// What `session resume` can act on: a session that has ended, which is what
/// it revives.
pub fn ended_session_ids() -> Vec<CompletionCandidate> {
    by_id("/v1/sessions", session_has_ended, session_help)
}

fn session_is_live(row: &Value) -> bool {
    status::<SessionStatus>(row).is_some_and(|s| s.is_live())
}

fn session_has_ended(row: &Value) -> bool {
    status::<SessionStatus>(row).is_some_and(|s| !s.is_live())
}

/// Session, task and goal ids (top-level `ariadne attach`), live sessions
/// first: attaching wants a pane that exists, and the rest of the list is
/// there because a task or goal id attaches to the session of its role.
///
/// The three lists are read together on one round rather than one after
/// another, so the budget covers the lot.
pub fn attach_ids() -> Vec<CompletionCandidate> {
    let Some((sessions, tasks, goals)) = round(async {
        let client = Client::from_env();
        tokio::join!(
            get(&client, "/v1/sessions"),
            get(&client, "/v1/tasks"),
            get(&client, "/v1/goals"),
        )
    }) else {
        return Vec::new();
    };
    attach_order(rows(sessions), rows(tasks), rows(goals))
}

/// Live sessions, then tasks and goals, then the sessions that have ended —
/// each list newest first.
fn attach_order(
    sessions: Vec<Value>,
    tasks: Vec<Value>,
    goals: Vec<Value>,
) -> Vec<CompletionCandidate> {
    let (live, ended): (Vec<Value>, Vec<Value>) = sessions.into_iter().partition(session_is_live);
    let mut out = ids(&live, session_help);
    out.extend(ids(&tasks, task_help));
    out.extend(ids(&goals, goal_help));
    out.extend(ids(&ended, session_help));
    out
}

// ---- profiles, repositories ----------------------------------------------

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

/// Registered repository ids (repo subcommands, `goal create --repo`).
pub fn repo_ids() -> Vec<CompletionCandidate> {
    fetch("/v1/repositories").iter().map(repository).collect()
}

/// Repositories of the goal being created in (`task create <goal> --repo`).
///
/// Only that goal's repositories are candidates, so the id has to come off the
/// command line, where the goal was named as a positional.
pub fn goal_repositories() -> Vec<CompletionCandidate> {
    let words: Vec<String> = std::env::args().collect();
    let Some(goal) = goal_on_the_line(&words) else {
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
fn repository(r: &Value) -> CompletionCandidate {
    let head = format!("{} [{}]", s(r, "path"), s(r, "base_branch"));
    let help = match r.get("description").and_then(|d| d.as_str()) {
        Some(d) if !d.trim().is_empty() => format!("{head} — {d}"),
        _ => head,
    };
    candidate(s(r, "id"), help)
}

/// The goal id on a `task create` line, in whatever order its flags came.
///
/// Ids are ULIDs and no flag looks like one, but a flag's *value* can — a
/// title with an id quoted in it, a `--depends-on` task — so a word some flag
/// is eating is never the goal. What is left is the one positional `task
/// create` has, wherever on the line it ended up.
fn goal_on_the_line(words: &[String]) -> Option<String> {
    let start = words.iter().position(|w| w == "create")? + 1;
    let mut eaten = false;
    for word in &words[start..] {
        if let Some(takes_the_next) = flag(word) {
            eaten = takes_the_next;
            continue;
        }
        if !eaten && is_ulid(word) {
            return Some(word.clone());
        }
        eaten = false;
    }
    None
}

/// Whether `word` is a flag and, if it is, whether its value is the *next*
/// word rather than carried inside it: `--repo x` and `-d x` eat what follows,
/// `--repo=x` and `-dx` have already eaten theirs.
///
/// A cluster of short flags ending in one that takes a value (`-yd x`) would
/// need the parser to unpick, and reads here as a short flag carrying its own
/// value. `task create` has exactly one short flag and it takes a value, so
/// there is no cluster on the line this reads.
fn flag(word: &str) -> Option<bool> {
    if let Some(long) = word.strip_prefix("--") {
        // A bare `--` is the end-of-flags separator, not a flag.
        return (!long.is_empty()).then_some(!long.contains('='));
    }
    let short = word.strip_prefix('-')?;
    // A bare `-` is a value like any other.
    (!short.is_empty()).then_some(short.len() == 1)
}

/// How every id in Ariadne is spelled: a 26-character ULID.
fn is_ulid(word: &str) -> bool {
    word.len() == 26 && word.chars().all(|c| c.is_ascii_alphanumeric())
}

// ---- agents, prompts -----------------------------------------------------

/// The agent CLIs, for `ariadne agent update <kind>` — the one place left
/// where an agent CLI is named on its own, since what an agent *runs on* is
/// chosen as a whole model (`--model`).
pub fn agent_kinds() -> Vec<CompletionCandidate> {
    AgentKind::ALL
        .into_iter()
        .map(|kind| CompletionCandidate::new(kind.as_str()))
        .collect()
}

/// The prompt kind of `profile prompt get|set|reset`: `system`, the one
/// prompt a profile owns.
pub fn prompt_kinds() -> Vec<CompletionCandidate> {
    vec![CompletionCandidate::new("system").help(Some("the profile's own system prompt".into()))]
}

// ---- models --------------------------------------------------------------

/// Model candidates for `--model`: everything an agent can be pinned to, in
/// the one spelling that pins it — `<agent_kind>[:<model>]`, the bare agent
/// CLI included, which is that CLI on its own default model.
///
/// The catalog is the daemon's (`GET /v1/models`, the list the UI offers,
/// opencode discovery included), kept on disk so that pressing TAB again
/// costs nothing and so that a daemon which is down still completes with what
/// it last said. Only a machine that has never reached one falls back to the
/// compiled-in curated lists.
pub fn models() -> Vec<CompletionCandidate> {
    match model_catalog() {
        Some(catalog) => catalog.iter().map(model_candidate).collect(),
        None => curated_catalog(),
    }
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

/// Effort candidates for `--effort`: every effort the catalog knows, once
/// each, cheapest first, described by what it buys.
///
/// Which of them a given model takes is the model's own business, and clap
/// cannot see the `--model` sitting on the same line — so the union is what
/// there is to offer, and the daemon refuses one that does not belong to the
/// model it is written beside.
///
/// Each entry lists its own efforts cheapest → deepest, and the lists agree
/// wherever they overlap, so they are merged rather than concatenated: what
/// comes out reads from cheapest to deepest across every agent CLI.
pub fn efforts() -> Vec<CompletionCandidate> {
    let (known, entries) = match model_catalog() {
        Some(catalog) => (
            catalog.iter().map(catalog_efforts).collect(),
            catalog_effort_entries(&catalog),
        ),
        None => (curated_efforts(), curated_effort_entries()),
    };
    merged(known)
        .into_iter()
        .map(|id| match effort_help(&id, &entries) {
            Some(help) => candidate(&id, help),
            None => CompletionCandidate::new(id),
        })
        .collect()
}

/// One effort as one agent CLI describes it: which CLI, the effort's own id,
/// and what it buys where that CLI has written one.
struct EffortEntry {
    kind: String,
    id: String,
    description: Option<String>,
}

/// Every effort of every model in a fetched catalog, in the order the
/// catalog lists them — the order [`effort_help`] reads "the first entry
/// that lists it" in.
fn catalog_effort_entries(catalog: &[Value]) -> Vec<EffortEntry> {
    catalog
        .iter()
        .flat_map(|m| {
            let kind = s(m, "agent_kind").to_string();
            m.get("efforts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(move |e| EffortEntry {
                    kind: kind.clone(),
                    id: s(e, "id").to_string(),
                    description: e
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                })
        })
        .collect()
}

/// The same, from the compiled-in ladders, for a machine that has never
/// reached a daemon.
fn curated_effort_entries() -> Vec<EffortEntry> {
    AgentKind::ALL
        .into_iter()
        .flat_map(|kind| {
            ariadne_core::models::known_efforts(kind)
                .iter()
                .map(move |effort| EffortEntry {
                    kind: kind.as_str().to_string(),
                    id: (*effort).to_string(),
                    description: ariadne_core::models::effort_description(kind, effort)
                        .map(str::to_string),
                })
        })
        .collect()
}

/// What `--effort <id>` is described as: the description every agent CLI
/// that takes it agrees on, when they agree — an effort means the same thing
/// on every model of one agent CLI, but not necessarily between two of them —
/// or, where two CLIs write different words for it, each named beside its
/// own: `low — claude_code: …; codex: …`.
///
/// `None` where nothing that lists this effort has written a description for
/// it at all.
fn effort_help(id: &str, entries: &[EffortEntry]) -> Option<String> {
    let mut described: Vec<(&str, &str)> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for e in entries {
        if e.id != id || seen.contains(&e.kind.as_str()) {
            continue;
        }
        seen.push(&e.kind);
        if let Some(d) = &e.description {
            described.push((&e.kind, d.as_str()));
        }
    }
    match described.as_slice() {
        [] => None,
        [(_, d)] => Some((*d).to_string()),
        many if many.windows(2).all(|w| w[0].1 == w[1].1) => Some(many[0].1.to_string()),
        many => Some(
            many.iter()
                .map(|(kind, d)| format!("{kind}: {d}"))
                .collect::<Vec<_>>()
                .join("; "),
        ),
    }
}

/// The same, plus the word an update writes to run the model at whatever its
/// agent CLI runs it at: `task update --effort` and `profile update --effort`.
pub fn efforts_or_default() -> Vec<CompletionCandidate> {
    let mut out = efforts();
    out.push(
        CompletionCandidate::new(crate::commands::DEFAULT).help(Some(
            "pin no effort: whatever the agent CLI reasons it at".into(),
        )),
    );
    out
}

/// The efforts one catalog entry lists, in the order it lists them — the id
/// of each, which is the word `--effort` takes; what the entry says about it
/// is for a picker with room to show it.
fn catalog_efforts(m: &Value) -> Vec<String> {
    m.get("efforts")
        .and_then(Value::as_array)
        .map(|efforts| {
            efforts
                .iter()
                .filter_map(|e| e.get("id").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// What a machine that has never reached a daemon offers: every effort each
/// agent CLI accepts, which is as much as this side knows on its own.
fn curated_efforts() -> Vec<Vec<String>> {
    AgentKind::ALL
        .into_iter()
        .map(|kind| {
            ariadne_core::models::known_efforts(kind)
                .iter()
                .map(|effort| (*effort).to_string())
                .collect()
        })
        .collect()
}

/// Several cheapest-first lists as one, each effort once and in an order that
/// keeps every list's own.
///
/// An effort nothing has offered yet is held back until the next one that has
/// been, and goes in just before it — which is what puts codex's `minimal` at
/// the head of a list that already starts at `low`. A run with nothing after
/// it is deeper than everything so far, and goes at the end; so does a list
/// that shares no effort at all with what is there, since nothing says where
/// else it would sit.
fn merged(lists: Vec<Vec<String>>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for list in lists {
        // The run of efforts read since the last one already offered, waiting
        // for the one that says where they belong.
        let mut pending: Vec<String> = Vec::new();
        // Where this list has read up to, in `out`'s own positions.
        let mut at = 0;
        for effort in list {
            match out.iter().position(|known| *known == effort) {
                Some(seen) if seen >= at => {
                    at = seen + pending.len() + 1;
                    out.splice(seen..seen, pending.drain(..));
                }
                Some(_) => {}
                None if pending.contains(&effort) => {}
                None => pending.push(effort),
            }
        }
        out.extend(pending);
    }
    out
}

/// The catalog: from disk while what is there is recent, then from the
/// daemon, then from disk at any age — telling "no daemon" from "a daemon
/// with no models" so only the former reaches back for a stale answer.
fn model_catalog() -> Option<Vec<Value>> {
    if let Some(fresh) = cached_models(Some(MODELS_TTL)) {
        return Some(fresh);
    }
    let fetched = round_within(CATALOG_BUDGET, async {
        get(&Client::from_env(), "/v1/models").await
    })
    .flatten();
    match fetched {
        Some(Value::Array(models)) => {
            store_models(&models);
            Some(models)
        }
        _ => cached_models(None),
    }
}

/// Where the catalog is kept: under the ariadne home, which is what the
/// daemon it came from is addressed by.
fn models_cache() -> Option<PathBuf> {
    Some(endpoint::home(None)?.join("cache").join("models.json"))
}

/// The catalog as last written, when it is no older than `max_age` — `None`
/// for "any age", which is what completes while the daemon is down.
fn cached_models(max_age: Option<Duration>) -> Option<Vec<Value>> {
    let path = models_cache()?;
    if let Some(ttl) = max_age {
        let age = std::fs::metadata(&path)
            .ok()?
            .modified()
            .ok()?
            .elapsed()
            .ok()?;
        if age > ttl {
            return None;
        }
    }
    match serde_json::from_slice(&std::fs::read(&path).ok()?).ok()? {
        Value::Array(models) if current_shape(&models) => Some(models),
        _ => None,
    }
}

/// Whether a written catalog is one this version can read: efforts used to be
/// bare words and are now objects, and a cache left by an older CLI is worth
/// no more than no cache at all — it is refetched rather than half-read.
fn current_shape(models: &[Value]) -> bool {
    models.iter().all(|m| match m.get("efforts") {
        Some(Value::Array(efforts)) => efforts.iter().all(Value::is_object),
        Some(_) => false,
        None => true,
    })
}

/// Write the catalog back, best-effort: a completion that cannot write its
/// cache is still a completion. Through a temporary file of its own, so a TAB
/// pressed mid-write reads the previous catalog rather than half of this one.
fn store_models(models: &[Value]) {
    let Some(path) = models_cache() else { return };
    let Some(dir) = path.parent().map(Path::to_path_buf) else {
        return;
    };
    let tmp = dir.join(format!("models.json.{}", std::process::id()));
    let written = std::fs::create_dir_all(&dir)
        .and_then(|()| serde_json::to_vec(models).map_err(std::io::Error::other))
        .and_then(|bytes| std::fs::write(&tmp, bytes))
        .and_then(|()| std::fs::rename(&tmp, &path));
    if let Err(e) = written {
        debug("caching the model catalog", &e);
        let _ = std::fs::remove_file(&tmp);
    }
}

fn model_candidate(m: &Value) -> CompletionCandidate {
    let tier = s(m, "tier");
    let tier = if tier.is_empty() { "unknown" } else { tier };
    let cost = m.get("cost").and_then(Value::as_u64).map(|n| n as u8);
    let speed = m.get("speed").and_then(Value::as_u64).map(|n| n as u8);
    let description = match m.get("description").and_then(|d| d.as_str()) {
        Some(d) => d,
        None => s(m, "agent_kind"),
    };
    candidate(s(m, "id"), model_help(tier, cost, speed, description))
}

/// A `--model` candidate's help: the three figures a picker sizes a task
/// from, then the sentence they summarize — `frontier · cost 5/5 · speed
/// 2/5 — deepest reasoning there is`.
fn model_help(tier: &str, cost: Option<u8>, speed: Option<u8>, description: &str) -> String {
    format!(
        "{tier} · cost {} · speed {} — {description}",
        band(cost),
        band(speed)
    )
}

/// A cost or speed band as `--model` help spells it: `3/5`, or `-` where
/// nothing has ranked it.
fn band(n: Option<u8>) -> String {
    match n {
        Some(n) => format!("{n}/5"),
        None => "-".to_string(),
    }
}

/// The compiled-in catalog, for a machine that has never reached a daemon:
/// each agent CLI, and the models ariadne-core knows it can be pinned to,
/// qualified here the way the daemon qualifies them. What an agent discovers
/// for itself is not in here — that is the daemon's job, and asking
/// `opencode` to list its own models on a TAB cost seconds every time the
/// daemon was down.
fn curated_catalog() -> Vec<CompletionCandidate> {
    AgentKind::ALL
        .into_iter()
        .flat_map(|kind| {
            // The bare CLI carries no bands of its own — it is every model of
            // that CLI at once, not one of them — so it gets the same
            // "unknown · cost - · speed -" a fetched catalog gives an entry
            // nothing has ranked, rather than the description on its own.
            let mut out = vec![candidate(
                kind.as_str(),
                model_help(
                    "unknown",
                    None,
                    None,
                    &format!("{} on its own default model", kind.as_str()),
                ),
            )];
            out.extend(curated_models(kind));
            out
        })
        .collect()
}

fn curated_models(kind: AgentKind) -> Vec<CompletionCandidate> {
    ariadne_core::models::curated_models(kind)
        .iter()
        .map(|m| {
            candidate(
                &qualified(kind, m.id),
                model_help(m.tier.as_str(), m.cost, m.speed, m.description),
            )
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;

    fn task(id: &str, status: &str) -> Value {
        json!({"id": id, "status": status, "title": "a task"})
    }

    fn session(id: &str, status: &str) -> Value {
        json!({"id": id, "status": status, "role": "engineer", "agent_kind": "codex"})
    }

    fn goal(id: &str, status: &str) -> Value {
        json!({"id": id, "status": status, "title": "a goal"})
    }

    fn kept(rows: &[Value], keep: fn(&Value) -> bool) -> Vec<&str> {
        rows.iter()
            .filter(|r| keep(r))
            .map(|r| s(r, "id"))
            .collect()
    }

    fn offered(candidates: &[CompletionCandidate]) -> Vec<String> {
        candidates
            .iter()
            .map(|c| c.get_value().to_string_lossy().into_owned())
            .collect()
    }

    fn words(line: &[&str]) -> Vec<String> {
        line.iter().map(|w| (*w).to_string()).collect()
    }

    /// `task cancel` refuses a task that has already ended, so completion
    /// never proposes one — and offers everything else, mid-flight or not.
    #[test]
    fn cancelling_a_task_offers_only_the_ones_still_going() {
        let rows = [
            task("01PENDING", "pending"),
            task("01WORKING", "in_progress"),
            task("01FAILED", "failed"),
            task("01MERGED", "merged"),
            task("01CANCELLED", "cancelled"),
            task("01NONSENSE", "integrating"),
        ];
        assert_eq!(
            kept(&rows, task_is_open),
            ["01PENDING", "01WORKING", "01FAILED"],
            "a merged or cancelled task is done with, and a status we cannot \
             read is not guessed at"
        );
    }

    /// The two halves of a session's life, and each verb gets its own:
    /// `session kill` has a tmux process to kill, `session resume` has one to
    /// bring back.
    #[test]
    fn killing_and_resuming_a_session_split_the_list_between_them() {
        let rows = [
            session("01STARTING", "starting"),
            session("01RUNNING", "running"),
            session("01IDLE", "idle"),
            session("01EXITED", "exited"),
            session("01FAILED", "failed"),
            session("01NONSENSE", "sleeping"),
        ];
        assert_eq!(
            kept(&rows, session_is_live),
            ["01STARTING", "01RUNNING", "01IDLE"]
        );
        assert_eq!(kept(&rows, session_has_ended), ["01EXITED", "01FAILED"]);
    }

    /// Every model lists its efforts cheapest → deepest and the lists agree
    /// wherever they overlap, so the union reads the same way: an effort one
    /// list has and another has not lands where its own list puts it, never
    /// appended after the rest.
    #[test]
    fn the_efforts_of_the_catalog_merge_into_one_cheapest_first_list() {
        assert_eq!(
            merged(vec![
                words(&["low", "medium", "high", "xhigh", "max"]),
                words(&["minimal", "low", "medium", "high", "xhigh", "max", "ultra"]),
                words(&["low", "medium", "high", "max"]),
                Vec::new(),
            ]),
            words(&["minimal", "low", "medium", "high", "xhigh", "max", "ultra"]),
            "each effort once, and `minimal` before the `low` it is cheaper than"
        );
        // A model with no effort control contributes nothing, and one whose
        // efforts are its own variant names contributes those.
        assert_eq!(
            merged(vec![
                Vec::new(),
                words(&["low", "high"]),
                words(&["gpt-5-codex-low", "gpt-5-codex-high"]),
            ]),
            words(&["low", "high", "gpt-5-codex-low", "gpt-5-codex-high"])
        );
        assert_eq!(merged(Vec::new()), Vec::<String>::new());
    }

    /// What a machine that has never reached a daemon offers: the union of
    /// what each agent CLI accepts, in the same cheapest-first order.
    #[test]
    fn the_curated_efforts_are_what_every_cli_accepts() {
        assert_eq!(
            merged(curated_efforts()),
            words(&["minimal", "low", "medium", "high", "xhigh", "max", "ultra"])
        );
    }

    /// The catalog's own answer is read as it comes, and an entry that lists
    /// no efforts — a model with no effort control, or a daemon too old to
    /// say — contributes nothing rather than breaking the list.
    #[test]
    fn an_entry_offers_the_efforts_it_lists_and_no_others() {
        assert_eq!(
            catalog_efforts(&json!({"id": "codex:gpt-5.6-sol", "efforts": [
                {"id": "low", "description": "lighter reasoning", "default": false},
                {"id": "high", "description": "greater depth", "default": true},
            ]})),
            words(&["low", "high"])
        );
        assert_eq!(
            catalog_efforts(&json!({"id": "claude_code:claude-haiku-4-5", "efforts": []})),
            Vec::<String>::new()
        );
        assert_eq!(
            catalog_efforts(&json!({"id": "claude_code"})),
            Vec::<String>::new()
        );
    }

    /// A `--model` candidate's help leads with the three figures a picker
    /// sizes a task from, then the description — bands and all, or dashes
    /// where nothing has ranked them.
    #[test]
    fn a_models_help_leads_with_its_bands_then_its_description() {
        assert_eq!(
            model_help("frontier", Some(5), Some(2), "deepest reasoning there is"),
            "frontier · cost 5/5 · speed 2/5 — deepest reasoning there is"
        );
        assert_eq!(
            model_help("unknown", None, None, "codex"),
            "unknown · cost - · speed - — codex"
        );
    }

    /// A machine that has never reached a daemon offers the compiled-in
    /// catalog, and its bare-CLI entries carry the same "unknown · cost - ·
    /// speed -" bands a fetched catalog gives an entry nothing has ranked,
    /// rather than the description on its own.
    #[test]
    fn the_curated_fallback_bands_its_bare_cli_entries_too() {
        let claude_code = curated_catalog()
            .into_iter()
            .find(|c| c.get_value().to_string_lossy() == "claude_code")
            .expect("claude_code is offered on its own");
        assert_eq!(
            claude_code.get_help().expect("help").to_string(),
            "unknown · cost - · speed - — claude_code on its own default model"
        );
    }

    /// An effort every entry that lists it agrees about is described once;
    /// one that two agent CLIs describe differently is described per CLI,
    /// each named beside its own words; one nothing has described at all
    /// carries no help.
    #[test]
    fn an_effort_is_described_once_when_every_cli_agrees_and_per_cli_when_they_do_not() {
        let entries = catalog_effort_entries(
            json!([
                {"agent_kind": "claude_code", "efforts": [
                    {"id": "high", "description": "greater depth"},
                    {"id": "low", "description": "lighter reasoning"},
                ]},
                {"agent_kind": "codex", "efforts": [
                    {"id": "high", "description": "more thinking time"},
                    {"id": "low", "description": "lighter reasoning"},
                    {"id": "minimal"},
                ]},
            ])
            .as_array()
            .expect("an array"),
        );
        assert_eq!(
            effort_help("low", &entries).as_deref(),
            Some("lighter reasoning"),
            "the two CLIs agree, so it is said once"
        );
        assert_eq!(
            effort_help("high", &entries).as_deref(),
            Some("claude_code: greater depth; codex: more thinking time"),
            "and where they do not, each is named beside its own"
        );
        assert_eq!(
            effort_help("minimal", &entries),
            None,
            "nothing here has written what minimal buys"
        );
    }

    /// A catalog written by an older CLI spelled its efforts as bare words.
    /// It is not read at all — the daemon is asked again — rather than
    /// completing an entry with no efforts on it.
    #[test]
    fn a_catalog_of_the_old_shape_is_no_catalog() {
        let current = json!([
            {"id": "codex:gpt-5.6-sol", "efforts": [{"id": "low", "default": true}]},
            {"id": "codex", "efforts": []},
            {"id": "claude_code"},
        ]);
        assert!(current_shape(current.as_array().expect("an array")));
        for stale in [
            json!([{"id": "codex:gpt-5.6-sol", "efforts": ["low", "high"]}]),
            json!([{"id": "codex:gpt-5.6-sol", "efforts": "low"}]),
        ] {
            assert!(
                !current_shape(stale.as_array().expect("an array")),
                "{stale}"
            );
        }
        assert!(current_shape(&[]), "a daemon that serves no models");
    }

    /// Ids are ULIDs, so the daemon's oldest-first list read backwards is
    /// newest-first — which is the one a person is completing nine times in
    /// ten.
    #[test]
    fn candidates_come_out_newest_first() {
        let rows = [task("01OLD", "merged"), task("01NEW", "failed")];
        assert_eq!(offered(&ids(&rows, task_help)), ["01NEW", "01OLD"]);
        assert_eq!(
            ids(&rows, task_help)[0].get_help().unwrap().to_string(),
            "[failed] a task",
            "the id is described by what it is, as it always was"
        );
    }

    /// `ariadne attach` takes a session, task or goal id, and only a live
    /// session has a pane waiting: those come first, and the ones that have
    /// ended come last.
    #[test]
    fn attaching_offers_live_sessions_first_and_ended_ones_last() {
        let sessions = vec![
            session("01EXITED", "exited"),
            session("01RUNNING", "running"),
        ];
        let tasks = vec![task("01TASK", "in_progress")];
        let goals = vec![goal("01GOAL", "active")];
        assert_eq!(
            offered(&attach_order(sessions, tasks, goals)),
            ["01RUNNING", "01TASK", "01GOAL", "01EXITED"]
        );
    }

    /// The goal of a `task create` is a positional, and completion has only
    /// the raw line to find it on: whatever the flag order, and whatever a
    /// flag is carrying.
    #[test]
    fn the_goal_is_found_wherever_task_create_put_it() {
        let goal = "01J9ZQ4T7K3M8N2P5R6S7V8W9X";
        assert_eq!(goal.len(), 26, "the fixture is ULID-shaped");
        let other = "01AAAAAAAAAAAAAAAAAAAAAAAA";

        let found = |line: &[&str]| goal_on_the_line(&words(line));
        assert_eq!(
            found(&["ariadne", "task", "create", goal, "--repo"]),
            Some(goal.to_string()),
        );
        assert_eq!(
            found(&["ariadne", "task", "create", "--title", "t", goal, "--repo"]),
            Some(goal.to_string()),
            "flags may come first",
        );
        assert_eq!(
            found(&["ariadne", "task", "create", "--title", other, goal]),
            Some(goal.to_string()),
            "an id quoted inside a title is a value, not the goal",
        );
        assert_eq!(
            found(&["ariadne", "task", "create", "--depends-on", other, goal]),
            Some(goal.to_string()),
            "and neither is the task another flag names",
        );
        assert_eq!(
            found(&[
                "ariadne",
                "task",
                "create",
                &format!("--title={other}"),
                goal
            ]),
            Some(goal.to_string()),
            "a flag that carries its own value eats nothing after it",
        );
        assert_eq!(
            found(&["ariadne", "task", "create", "-d", other, goal]),
            Some(goal.to_string()),
            "short flags take a value too",
        );
        assert_eq!(
            found(&["ariadne", "task", "create", "-done thing", goal]),
            Some(goal.to_string()),
            "and a short flag may carry that value attached, eating nothing",
        );
        assert_eq!(
            found(&["ariadne", "task", "create", &format!("-d{other}"), goal]),
            Some(goal.to_string()),
            "even when what it carries is itself id-shaped",
        );
        assert_eq!(
            found(&["ariadne", "task", "create", "--", goal]),
            Some(goal.to_string()),
            "the end-of-flags separator is not a flag and eats nothing",
        );
        assert_eq!(
            found(&["ariadne", "task", "create", "--repo"]),
            None,
            "nothing to complete against until a goal is on the line",
        );
        assert_eq!(
            found(&["ariadne", "task", "ls"]),
            None,
            "and no goal is looked for on a line that creates nothing",
        );
    }
}
