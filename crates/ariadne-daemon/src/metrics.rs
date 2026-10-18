//! The daemon's metric catalog (035), recorded through the global
//! OpenTelemetry meter that [`crate::telemetry::init`] sets.
//!
//! The daemon is the one recorder: the store commits state and records no
//! metric. A task's endings and the time it spent in each status are read
//! off the store's change hook in [`crate::bus`], because a task has many
//! writers and its `TaskUpdated` change carries each transition exactly once.
//! A goal's ending is recorded at its two call sites instead, since its
//! `GoalUpdated` change carries no transition to tell an ending from a later
//! edit of an ended goal.
//!
//! An instrument is built on each record, not cached: one built before the
//! meter provider is set stays a no-op for good, and the SDK returns the same
//! stream for the same name anyway. A histogram's buckets are the provider's
//! [`view`], not the instrument's.
//!
//! No attribute carries a goal, task, session or launch id, or a path.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, Meter};
use opentelemetry_sdk::metrics::{Aggregation, Instrument, InstrumentKind, Stream};

use ariadne_core::{GoalStatus, TaskStatus, TokenUsage};
use ariadne_store::{AgentSession, Goal, SessionFilter, Store, Task, TaskFilter, TaskTransition};

const METER: &str = "ariadned";

/// Buckets for work that runs minutes to hours: lead times, lifetimes,
/// waits on a person.
const LONG_SECONDS: &[f64] = &[
    1.0, 10.0, 60.0, 300.0, 900.0, 1_800.0, 3_600.0, 7_200.0, 14_400.0, 28_800.0, 86_400.0,
];
/// Buckets for a request or a pass: milliseconds to seconds.
const SHORT_SECONDS: &[f64] = &[
    0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
];
/// Buckets for an agent's launch or turn: a second to tens of minutes.
const AGENT_SECONDS: &[f64] = &[
    0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1_800.0, 3_600.0,
];

/// The label of an absent value: a session with no seat, a task with no
/// column, a flag that is not raised.
const NONE: &str = "none";

fn meter() -> Meter {
    opentelemetry::global::meter(METER)
}

fn counter(name: &'static str) -> Counter<u64> {
    meter().u64_counter(name).build()
}

fn seconds(name: &'static str) -> Histogram<f64> {
    meter().f64_histogram(name).with_unit("s").build()
}

/// The buckets of each histogram of the catalog, fit to its range.
fn buckets(name: &str) -> Option<&'static [f64]> {
    Some(match name {
        "ariadne.task.lead_time"
        | "ariadne.task.status_time"
        | "ariadne.goal.lead_time"
        | "ariadne.session.lifetime"
        | "ariadne.permission.wait"
        | "ariadne.attention.wait" => LONG_SECONDS,
        "http.server.request.duration"
        | "ariadne.scheduler.tick.duration"
        | "ariadne.ai_permissions.latency" => SHORT_SECONDS,
        "ariadne.acp.launch.duration" | "ariadne.forge.poll.duration" | "ariadne.turn.duration" => {
            AGENT_SECONDS
        }
        _ => return None,
    })
}

/// The View that gives each histogram of the catalog its buckets, for the
/// meter provider (`crate::telemetry::init`). Every other instrument keeps
/// the SDK's default stream.
pub fn view(instrument: &Instrument) -> Option<Stream> {
    if instrument.kind() != InstrumentKind::Histogram {
        return None;
    }
    let boundaries = buckets(instrument.name())?.to_vec();
    Stream::builder()
        .with_aggregation(Aggregation::ExplicitBucketHistogram {
            boundaries,
            record_min_max: true,
        })
        .build()
        .ok()
}

fn kv(key: &'static str, value: impl Into<String>) -> KeyValue {
    KeyValue::new(key, value.into())
}

/// Seconds from `from` to `to`, both RFC 3339; zero where either is bad or
/// `to` comes first.
fn between(from: &str, to: &str) -> f64 {
    match (
        DateTime::parse_from_rfc3339(from),
        DateTime::parse_from_rfc3339(to),
    ) {
        (Ok(from), Ok(to)) => (to - from).num_milliseconds().max(0) as f64 / 1_000.0,
        _ => 0.0,
    }
}

/// A repository by its name: the last part of its checkout, never the path.
pub(crate) async fn repo_name(store: &Store, repo_id: &str) -> String {
    store
        .get_repository(repo_id)
        .await
        .ok()
        .and_then(|repo| {
            Path::new(&repo.path)
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| NONE.into())
}

/// The repository of a session: its task's, else its goal's where the goal
/// works in one repository alone.
async fn session_repo(store: &Store, session: &AgentSession) -> String {
    if let Some(task_id) = &session.task_id
        && let Ok(task) = store.get_task(task_id).await
    {
        return repo_name(store, &task.repo_id).await;
    }
    if let Some(goal_id) = &session.goal_id
        && let Ok(repos) = store.list_goal_repositories(goal_id).await
        && let [one] = repos.as_slice()
    {
        return repo_name(store, &one.id).await;
    }
    NONE.into()
}

fn seat(session: &AgentSession) -> String {
    session.seat.clone().unwrap_or_else(|| NONE.into())
}

// ---- Group 1: what the work did -------------------------------------------

/// The launch of each session whose end has been counted.
fn ended_launches() -> &'static Mutex<HashMap<String, String>> {
    static ENDED: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    ENDED.get_or_init(Mutex::default)
}

/// Count a run's end once, off the committed row the change hook carries:
/// the first row of a launch that is no longer live. A repeated end, a kill
/// and the end its agent reports after it, and every later write of the
/// ended row all carry the same launch, and count nothing more. A row that
/// ended before this daemon started is not one it saw end.
fn record_run_end(session: &AgentSession) {
    let Some(ended) = session.ended_at.as_deref() else {
        return;
    };
    if session.status().is_live()
        || !DateTime::parse_from_rfc3339(ended).is_ok_and(|at| at >= *watching_since())
    {
        return;
    }
    let launch = session.launch_id.clone().unwrap_or_default();
    if ended_launches()
        .lock()
        .expect("ended launches lock")
        .insert(session.id.clone(), launch.clone())
        .as_ref()
        == Some(&launch)
    {
        return;
    }
    let attrs = [
        kv("seat", seat(session)),
        kv("model", session.model.clone()),
        kv("status", session.status.clone()),
        kv(
            "attention_reason",
            session
                .attention_reason
                .clone()
                .unwrap_or_else(|| NONE.into()),
        ),
    ];
    counter("ariadne.sessions.ended").add(1, &attrs);
    let lifetime = between(
        session
            .launched_at
            .as_deref()
            .unwrap_or(&session.created_at),
        ended,
    );
    seconds("ariadne.session.lifetime").record(lifetime, &attrs[..2]);
}

/// Record a model switch, off the session that leaves.
pub fn record_switch(
    session: &AgentSession,
    to_model: &str,
    reason: &str,
    automatic: bool,
    same_agent: bool,
) {
    counter("ariadne.switches").add(
        1,
        &[
            kv("reason", reason),
            KeyValue::new("automatic", automatic),
            KeyValue::new("same_agent", same_agent),
            kv("from_model", session.model.clone()),
            kv("to_model", to_model),
        ],
    );
}

/// Record one stored message, by its kind and the seats at its two ends.
pub fn record_message(kind: &str, from_seat: &str, to_seat: &str) {
    counter("ariadne.messages").add(
        1,
        &[
            kv("kind", kind),
            kv("from_seat", from_seat),
            kv("to_seat", to_seat),
        ],
    );
}

/// Record a permission answer and how long the agent waited for it.
pub fn record_permission(decided_by: &str, answer: &str, tool_name: &str, waited: f64) {
    counter("ariadne.permissions").add(
        1,
        &[
            kv("decided_by", decided_by),
            kv("answer", answer),
            kv("tool_name", tool_name),
        ],
    );
    seconds("ariadne.permission.wait").record(waited, &[kv("decided_by", decided_by)]);
}

/// Record what one usage write moved, by kind: the step each counter rose,
/// as the write's own transaction measured it. A counter that fell moved by
/// zero, so the counter never goes down.
pub async fn record_tokens(store: &Store, session_id: &str, moved: TokenUsage) {
    let deltas = [
        ("input", moved.input_tokens),
        ("cached_input", moved.cached_input_tokens),
        ("output", moved.output_tokens),
    ];
    if deltas.iter().all(|(_, delta)| *delta == 0) {
        return;
    }
    let Ok(session) = store.get_session(session_id).await else {
        return;
    };
    let repo = session_repo(store, &session).await;
    let tokens = counter("ariadne.tokens");
    for (kind, delta) in deltas {
        if delta > 0 {
            tokens.add(
                delta,
                &[
                    kv("kind", kind),
                    kv("model", session.model.clone()),
                    kv("seat", seat(&session)),
                    kv("repo", repo.clone()),
                ],
            );
        }
    }
}

/// Record one task transition, from the change hook: the seconds the task
/// spent in the status it leaves, and, where it ends, the ending.
pub async fn record_task_transition(store: &Store, task: &Task, transition: &TaskTransition) {
    // The status left began at the transition before this one, or at the
    // task's creation where this is its first.
    let entered = store
        .list_task_transitions(&task.id)
        .await
        .ok()
        .and_then(|all| {
            all.iter()
                .take_while(|t| t.id != transition.id)
                .last()
                .map(|t| t.created_at.clone())
        })
        .unwrap_or_else(|| task.created_at.clone());
    seconds("ariadne.task.status_time").record(
        between(&entered, &transition.created_at),
        &[kv("status", transition.from_status.clone())],
    );
    if !matches!(
        transition.to_status.as_str(),
        "finished" | "cancelled" | "failed"
    ) {
        return;
    }
    let step = task.step.clone().unwrap_or_else(|| NONE.into());
    let model = match &task.step {
        Some(step) => store
            .list_task_agents(&task.id)
            .await
            .ok()
            .and_then(|agents| agents.into_iter().find(|agent| agent.step == *step))
            .map(|agent| agent.model),
        None => None,
    };
    let workflow = store
        .get_goal(&task.goal_id)
        .await
        .map(|goal| goal.workflow)
        .unwrap_or_else(|_| NONE.into());
    let repo = repo_name(store, &task.repo_id).await;
    counter("ariadne.tasks.ended").add(
        1,
        &[
            kv("status", transition.to_status.clone()),
            kv("step", step.clone()),
            KeyValue::new("landed", task.merge_commit.is_some()),
            kv("workflow", workflow.clone()),
            kv("repo", repo.clone()),
            kv("model", model.unwrap_or_else(|| NONE.into())),
        ],
    );
    seconds("ariadne.task.lead_time").record(
        between(&task.created_at, &transition.created_at),
        &[
            kv("status", transition.to_status.clone()),
            kv("step", step),
            kv("workflow", workflow),
            kv("repo", repo),
        ],
    );
}

/// Record a goal that has just ended, at the call that ended it.
pub async fn record_goal_ended(store: &Store, goal: &Goal) {
    if !matches!(goal.status(), GoalStatus::Completed | GoalStatus::Cancelled) {
        return;
    }
    let repo = match store.list_goal_repositories(&goal.id).await.as_deref() {
        Ok([one]) => repo_name(store, &one.id).await,
        _ => NONE.into(),
    };
    counter("ariadne.goals.ended").add(
        1,
        &[
            kv("status", goal.status.clone()),
            kv("workflow", goal.workflow.clone()),
            kv("repo", repo),
        ],
    );
    seconds("ariadne.goal.lead_time").record(
        between(&goal.created_at, &goal.updated_at),
        &[
            kv("status", goal.status.clone()),
            kv("workflow", goal.workflow.clone()),
        ],
    );
}

/// The attention flag each session was last seen with, as its reason and
/// when it was raised.
type Flag = Option<(String, String)>;

fn flags() -> &'static Mutex<HashMap<String, Flag>> {
    static FLAGS: OnceLock<Mutex<HashMap<String, Flag>>> = OnceLock::new();
    FLAGS.get_or_init(Mutex::default)
}

/// When this process began to watch flags: a flag first seen on a session
/// with a later `attention_since` was raised under this daemon.
fn watching_since() -> &'static DateTime<Utc> {
    static SINCE: OnceLock<DateTime<Utc>> = OnceLock::new();
    SINCE.get_or_init(Utc::now)
}

/// Record a raise and a clear of a session's attention flag, from the change
/// hook: the store sends a session's row after every write of its flag.
pub fn record_session_update(session: &AgentSession) {
    record_run_end(session);
    let now = Utc::now();
    let flag = session
        .attention_reason
        .clone()
        .zip(session.attention_since.clone());
    let before = flags()
        .lock()
        .expect("attention flags lock")
        .insert(session.id.clone(), flag.clone());
    let before = match before {
        Some(before) => before,
        // Not seen yet: a flag raised before this daemon started is not one
        // it saw raised, and has no clear of its own to time yet either.
        None => {
            if let Some((reason, since)) = &flag
                && DateTime::parse_from_rfc3339(since).is_ok_and(|since| since >= *watching_since())
            {
                counter("ariadne.attention.raised").add(1, &[kv("reason", reason.clone())]);
            }
            return;
        }
    };
    if before == flag {
        return;
    }
    if let Some((reason, since)) = before {
        counter("ariadne.attention.cleared").add(1, &[kv("reason", reason.clone())]);
        seconds("ariadne.attention.wait").record(
            between(
                &since,
                &now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            ),
            &[kv("reason", reason)],
        );
    }
    if let Some((reason, _)) = flag {
        counter("ariadne.attention.raised").add(1, &[kv("reason", reason)]);
    }
}

// ---- Group 4: agent activity, and the ACP launch ---------------------------

/// The clocks of each launch: when its agent was spawned, until the session
/// is ready, and when its running turn was prompted.
#[derive(Default)]
struct Clocks {
    spawned: Option<Instant>,
    prompted: Option<Instant>,
}

fn clocks() -> &'static Mutex<HashMap<String, Clocks>> {
    static CLOCKS: OnceLock<Mutex<HashMap<String, Clocks>>> = OnceLock::new();
    CLOCKS.get_or_init(Mutex::default)
}

fn record_launch(outcome: &'static str, took: Option<Duration>) {
    counter("ariadne.acp.launches").add(1, &[kv("outcome", outcome)]);
    if let Some(took) = took {
        seconds("ariadne.acp.launch.duration")
            .record(took.as_secs_f64(), &[kv("outcome", outcome)]);
    }
}

/// An agent that could not be started at all.
pub fn record_launch_failed_to_spawn() {
    record_launch("spawn_failed", None);
}

/// An agent process is up: its launch's clock starts.
pub fn record_launch_spawned(launch_id: &str) {
    clocks()
        .lock()
        .expect("launch clocks lock")
        .entry(launch_id.to_string())
        .or_default()
        .spawned = Some(Instant::now());
}

/// Record agent activity off each stored ACP event: the launch's end of
/// setup (`session_start`) or its death before it (`session_end`), a turn
/// (`user_prompt_submit` to `stop`), and a tool call (`pre_tool_use`). Only
/// the tool's name is read from the payload.
pub async fn record_agent_activity(
    store: &Store,
    session_id: &str,
    launch_id: &str,
    kind: &str,
    payload: &serde_json::Value,
) {
    let (launched, turn) = {
        let mut clocks = clocks().lock().expect("launch clocks lock");
        match kind {
            "session_start" => (
                clocks
                    .get_mut(launch_id)
                    .and_then(|c| c.spawned.take())
                    .map(|at| ("ready", at.elapsed())),
                None,
            ),
            "session_end" => (
                clocks
                    .remove(launch_id)
                    .and_then(|c| c.spawned)
                    .map(|at| ("failed", at.elapsed())),
                None,
            ),
            "user_prompt_submit" => {
                clocks.entry(launch_id.to_string()).or_default().prompted = Some(Instant::now());
                return;
            }
            "stop" => (
                None,
                clocks
                    .get_mut(launch_id)
                    .and_then(|c| c.prompted.take())
                    .map(|at| at.elapsed()),
            ),
            "pre_tool_use" => (None, None),
            _ => return,
        }
    };
    if let Some((outcome, took)) = launched {
        record_launch(outcome, Some(took));
    }
    if !matches!(kind, "stop" | "pre_tool_use") {
        return;
    }
    let Ok(session) = store.get_session(session_id).await else {
        return;
    };
    let attrs = [
        kv("seat", seat(&session)),
        kv("model", session.model.clone()),
    ];
    if kind == "stop" {
        counter("ariadne.turns").add(1, &attrs);
        if let Some(took) = turn {
            seconds("ariadne.turn.duration").record(took.as_secs_f64(), &attrs);
        }
    } else {
        let name = payload
            .get("tool_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        counter("ariadne.tool_calls").add(1, &[kv("tool_name", name), attrs[1].clone()]);
    }
}

/// Record one answer of the AI permission model, and how long it took.
pub fn record_ai_permission(verdict: &str, took: f64) {
    counter("ariadne.ai_permissions.decisions").add(1, &[kv("verdict", verdict)]);
    seconds("ariadne.ai_permissions.latency").record(took, &[]);
}

// ---- Group 3: daemon health ------------------------------------------------

/// Record an answered request under its matched route, never its path.
pub fn record_http_request(method: &str, route: &str, status: u16, took: f64) {
    seconds("http.server.request.duration").record(
        took,
        &[
            kv("http.request.method", method),
            kv("http.route", route),
            KeyValue::new("http.response.status_code", i64::from(status)),
        ],
    );
}

/// Record one periodic scheduler reconciliation.
pub fn record_scheduler_tick(took: f64) {
    seconds("ariadne.scheduler.tick.duration").record(took, &[]);
}

/// Record one forge fetch of a repository's requests.
pub fn record_forge_poll(repo: &str, outcome: &'static str, took: f64) {
    seconds("ariadne.forge.poll.duration")
        .record(took, &[kv("repo", repo), kv("outcome", outcome)]);
}

/// Record one webhook delivery, by what the ingress made of it.
pub fn record_webhook(outcome: &'static str) {
    counter("ariadne.forge.webhooks").add(1, &[kv("outcome", outcome)]);
}

/// Record a follower of `/v1/events/stream` that fell behind.
pub fn record_events_resync() {
    counter("ariadne.events.stream.resyncs").add(1, &[]);
}

/// Record the rows one retention sweep deleted, per table.
pub fn record_telemetry_deleted(table: &'static str, rows: u64) {
    if rows > 0 {
        counter("ariadne.telemetry.deleted").add(rows, &[kv("table", table)]);
    }
}

fn stream_clients() -> &'static AtomicI64 {
    static CLIENTS: AtomicI64 = AtomicI64::new(0);
    &CLIENTS
}

/// One follower of `/v1/events/stream`, counted while it is held.
pub struct EventStreamClient(());

impl EventStreamClient {
    pub fn open() -> Self {
        stream_clients().fetch_add(1, Ordering::Relaxed);
        Self(())
    }
}

impl Drop for EventStreamClient {
    fn drop(&mut self) {
        stream_clients().fetch_sub(1, Ordering::Relaxed);
    }
}

// ---- Group 2: live state, and the observable gauges ------------------------

/// One observation of a gauge: its attributes and its value.
type Observed = (Vec<KeyValue>, u64);

/// The live-state gauges, each read off the store on collection.
#[derive(Clone, Copy)]
enum Live {
    Tasks,
    Goals,
    Sessions,
    Attention,
}

fn tally(keys: impl IntoIterator<Item = Vec<KeyValue>>) -> Vec<Observed> {
    let mut counts: Vec<Observed> = Vec::new();
    for key in keys {
        match counts.iter_mut().find(|(attrs, _)| *attrs == key) {
            Some((_, n)) => *n += 1,
            None => counts.push((key, 1)),
        }
    }
    counts
}

async fn read_live(store: &Store, gauge: Live) -> ariadne_store::Result<Vec<Observed>> {
    Ok(match gauge {
        Live::Tasks => tally(
            store
                .list_tasks(TaskFilter::default())
                .await?
                .iter()
                .filter(|t| {
                    !matches!(
                        t.status(),
                        TaskStatus::Finished | TaskStatus::Cancelled | TaskStatus::Failed
                    )
                })
                .map(|t| vec![kv("status", t.status.clone())]),
        ),
        Live::Goals => tally(
            store
                .list_goals(&[])
                .await?
                .iter()
                .filter(|g| !matches!(g.status(), GoalStatus::Completed | GoalStatus::Cancelled))
                .map(|g| vec![kv("status", g.status.clone())]),
        ),
        Live::Sessions => tally(
            store
                .list_sessions(SessionFilter {
                    live_only: true,
                    ..SessionFilter::default()
                })
                .await?
                .iter()
                .map(|s| vec![kv("status", s.status.clone()), kv("seat", seat(s))]),
        ),
        Live::Attention => tally(
            store
                .list_sessions(SessionFilter {
                    attention_only: true,
                    ..SessionFilter::default()
                })
                .await?
                .iter()
                .filter_map(|s| s.attention_reason.clone())
                .map(|reason| vec![kv("reason", reason)]),
        ),
    })
}

/// How long one collection waits on a store read before it reports nothing
/// for that gauge.
const COLLECT_READ: Duration = Duration::from_secs(2);

/// The runtime a gauge callback reads the store on. A callback runs on the
/// metric reader's own thread, and a flush can block the very thread a
/// daemon task would answer on, so the read runs on a runtime of its own,
/// through a read-only store of its own: a pool returns each connection on
/// the runtime that dropped it, so the daemon's pools stay on the daemon's
/// runtime. Kept for the life of the process: a runtime must not be dropped
/// inside another one.
struct Collector {
    runtime: tokio::runtime::Runtime,
    path: PathBuf,
    store: tokio::sync::OnceCell<Store>,
}

fn collector() -> &'static OnceLock<Option<Collector>> {
    static COLLECTOR: OnceLock<Option<Collector>> = OnceLock::new();
    &COLLECTOR
}

impl Collector {
    fn start(path: PathBuf) -> Option<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("metrics-collect")
            .enable_all()
            .build()
            .inspect_err(|error| tracing::warn!(%error, "cannot start the metrics collector"))
            .ok()?;
        Some(Self {
            runtime,
            path,
            store: tokio::sync::OnceCell::new(),
        })
    }

    async fn read(&self, gauge: Live) -> ariadne_store::Result<Vec<Observed>> {
        let store = self
            .store
            .get_or_try_init(|| Store::open_read_only(&self.path))
            .await?;
        read_live(store, gauge).await
    }
}

/// Every attribute set each live-state gauge has reported: one whose count
/// has fallen to nothing reports zero, rather than keeping its last count.
fn seen_of(gauge: Live) -> &'static Mutex<Vec<Vec<KeyValue>>> {
    static SEEN: OnceLock<[Mutex<Vec<Vec<KeyValue>>>; 4]> = OnceLock::new();
    &SEEN.get_or_init(Default::default)[gauge as usize]
}

/// Read one live-state gauge now, with a zero for each attribute set it
/// reported before and holds no more. None where the read failed: then the
/// gauge observes nothing, rather than a zero no read proved.
fn collect_live(gauge: Live) -> Option<Vec<Observed>> {
    let read = read_now(gauge);
    with_zeros(&mut seen_of(gauge).lock().expect("seen gauges lock"), read)
}

/// Add a zero for each attribute set in `seen` that a successful `read`
/// does not hold, and remember the sets it does. A failed read changes
/// nothing and answers None.
fn with_zeros(seen: &mut Vec<Vec<KeyValue>>, read: Option<Vec<Observed>>) -> Option<Vec<Observed>> {
    let mut observed = read?;
    for attrs in seen.iter() {
        if !observed.iter().any(|(held, _)| held == attrs) {
            observed.push((attrs.clone(), 0));
        }
    }
    for (attrs, _) in &observed {
        if !seen.contains(attrs) {
            seen.push(attrs.clone());
        }
    }
    Some(observed)
}

/// Read one live-state gauge now, from the reader's thread. None where it
/// could not be read: on a runtime's own thread, with no collector, on a
/// store error, or past [`COLLECT_READ`].
fn read_now(gauge: Live) -> Option<Vec<Observed>> {
    // Never block a runtime's own thread.
    if tokio::runtime::Handle::try_current().is_ok() {
        return None;
    }
    let collector = collector().get().and_then(Option::as_ref)?;
    match collector
        .runtime
        .block_on(async { tokio::time::timeout(COLLECT_READ, collector.read(gauge)).await })
    {
        Ok(Ok(observed)) => Some(observed),
        Ok(Err(error)) => {
            tracing::warn!(%error, "could not read a live-state metric");
            None
        }
        Err(_) => {
            tracing::warn!("reading a live-state metric timed out");
            None
        }
    }
}

/// Process CPU time in seconds, user and system together.
fn cpu_seconds() -> f64 {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::ProcessCPUTime);
    t.tv_sec as f64 + t.tv_nsec as f64 / 1e9
}

/// Register the observable gauges. `files` are the two databases,
/// `ariadne.db` first: the live-state gauges read it on each collection, and
/// the size of both is read then too.
pub fn start(files: [PathBuf; 2]) {
    let _ = watching_since();
    let _ = collector().get_or_init(|| Collector::start(files[0].clone()));
    let meter = meter();
    for (name, gauge) in [
        ("ariadne.tasks.active", Live::Tasks),
        ("ariadne.goals.active", Live::Goals),
        ("ariadne.sessions.live", Live::Sessions),
        ("ariadne.attention.pending", Live::Attention),
    ] {
        // Built for the callback it carries: the provider holds it from here.
        let _ = meter
            .u64_observable_gauge(name)
            .with_callback(move |observer| {
                for (attrs, value) in collect_live(gauge).unwrap_or_default() {
                    observer.observe(value, &attrs);
                }
            })
            .build();
    }
    let _ = meter
        .i64_observable_gauge("ariadne.events.stream.clients")
        .with_callback(|observer| observer.observe(stream_clients().load(Ordering::Relaxed), &[]))
        .build();
    let _ = meter
        .u64_observable_gauge("ariadne.db.size")
        .with_unit("By")
        .with_callback(move |observer| {
            for file in &files {
                if let (Ok(meta), Some(name)) = (
                    std::fs::metadata(file),
                    file.file_name().and_then(|n| n.to_str()),
                ) {
                    observer.observe(meta.len(), &[kv("file", name)]);
                }
            }
        })
        .build();
    let _ = meter
        .f64_observable_counter("process.cpu.time")
        .with_unit("s")
        .with_callback(|observer| {
            observer.observe(cpu_seconds(), &[]);
        })
        .build();
    let _ = meter
        .u64_observable_gauge("process.memory.usage")
        .with_unit("By")
        .with_callback(|observer| {
            if let Some(usage) = memory_stats::memory_stats() {
                observer.observe(usage.physical_mem as u64, &[]);
            }
        })
        .build();
}

#[cfg(test)]
mod tests {
    use super::{KeyValue, with_zeros};

    /// A failed read observes nothing and forgets nothing, and only a
    /// successful read that no longer holds an attribute set reports it zero.
    #[test]
    fn a_failed_read_reports_no_zero_and_an_empty_read_does() {
        let running = || vec![KeyValue::new("status", "running")];
        let mut seen = Vec::new();
        assert_eq!(
            with_zeros(&mut seen, Some(vec![(running(), 2)])),
            Some(vec![(running(), 2)])
        );
        assert_eq!(with_zeros(&mut seen, None), None);
        assert_eq!(seen, vec![running()], "a failed read keeps what was seen");
        assert_eq!(
            with_zeros(&mut seen, Some(Vec::new())),
            Some(vec![(running(), 0)])
        );
    }
}
