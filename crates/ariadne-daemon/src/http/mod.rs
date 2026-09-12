//! Axum application: routes, shared state, OpenAPI document.

pub use events::ingest_event;

mod caller;
mod catalog;
mod classify;
mod console;
pub(crate) mod convert;
mod doctor;
mod error;
pub(crate) mod events;
mod goals;
mod landing;
mod logs;
mod memories;
mod pins;
mod repositories;
mod sessions;
mod skills;
mod sse;
mod stream;
mod tasks;

use std::sync::Arc;
use std::time::Instant;

use axum::extract::State;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use chrono::{DateTime, SecondsFormat, Utc};
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use ariadne_api::stream::HeartbeatDto;
use ariadne_api::{HealthResponse, VersionResponse};
use ariadne_store::Store;

use crate::acp_discovery::AgentRegistry;
use crate::acp_sessions::OutsideSessions;
use catalog::{acp_agents, agents, models};

use crate::bus::EventBus;
use crate::launcher::Launcher;
use crate::log::LogBuffer;
use crate::scheduler::SchedEvent;

/// Shared handler state.
#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    /// Monotonic, so uptime is immune to the clock being set.
    pub started_at: Instant,
    /// The same moment as wall-clock time, which is the only one that can be
    /// written down: the heartbeat tells clients which daemon they are on.
    pub started_at_utc: DateTime<Utc>,
    pub launcher: Arc<Launcher>,
    /// Present once the scheduler is running; handlers poke it after writes.
    pub sched_tx: Option<mpsc::UnboundedSender<SchedEvent>>,
    /// Fan-out of domain events to `/v1/events/stream` subscribers.
    pub events: EventBus,
    /// Recent daemon log lines, served by `/v1/logs`.
    pub logs: LogBuffer,
    /// ACP commands and their cached discovery snapshot.
    pub agent_registry: AgentRegistry,
    /// The snapshot of every agent's stored sessions that
    /// `/v1/outside-sessions` pages.
    pub outside_sessions: OutsideSessions,
}

impl AppState {
    /// Who this daemon is, for the `heartbeat` control event.
    fn heartbeat(&self) -> HeartbeatDto {
        HeartbeatDto {
            version: env!("CARGO_PKG_VERSION").into(),
            started_at: self
                .started_at_utc
                .to_rfc3339_opts(SecondsFormat::Millis, true),
        }
    }

    /// Poke the scheduler, if one is running. Whether an event is worth
    /// acting on is its decision, not the handler's.
    fn wake(&self, event: SchedEvent) {
        if let Some(tx) = &self.sched_tx {
            let _ = tx.send(event);
        }
    }

    pub fn notify_scheduler(&self, task_id: &str) {
        self.wake(SchedEvent::TaskChanged(task_id.to_string()));
    }

    pub fn notify_scheduler_session(&self, session_id: &str) {
        self.wake(SchedEvent::SessionEvent(session_id.to_string()));
    }

    pub fn notify_scheduler_goal(&self, goal_id: &str) {
        self.wake(SchedEvent::GoalChanged(goal_id.to_string()));
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Ariadne Daemon API",
        description = "REST API of ariadned, the coding-agent orchestrator daemon.",
        version = env!("CARGO_PKG_VERSION"),
    ),
    paths(
        health,
        version,
        doctor::report,
        agents::list, agents::update, acp_agents::list, acp_agents::refresh,
        skills::create, skills::list, skills::get, skills::update, skills::delete,
        skills::reset_document,
        repositories::create, repositories::list, repositories::get,
        repositories::update, repositories::delete,
        memories::create, memories::list, memories::search, memories::delete,

        goals::create, goals::list, goals::get, goals::delete,
        goals::cancel, goals::complete, goals::finalize,
        tasks::create, tasks::list, tasks::get, tasks::update, tasks::adopt_author_session,
        tasks::transition, tasks::cancel, tasks::retry, tasks::list_transitions,
        landing::list_task_messages, landing::post_task_message,
        goals::list_goal_messages, goals::post_goal_message, landing::diff,
        landing::record_pull_request, landing::pick_winner,
        sessions::list, sessions::list_outside, sessions::get, sessions::kill, sessions::resume,
        console::snapshot, console::stream, console::input,
        events::list, stream::stream,
        models::list,
        models::set_enabled,
        logs::snapshot, logs::stream,
    ),
    components(schemas(
        ariadne_api::stream::DomainEvent, ariadne_api::stream::ResyncDto,
        ariadne_api::stream::HeartbeatDto,
        ariadne_api::events::AgentEventDto,
        ariadne_api::logs::LogLineDto, ariadne_api::logs::LogSnapshotResponse,
    )),
    tags(
        (name = "system", description = "Daemon health and metadata"),
        (name = "agents", description = "Per-agent launch configuration: the flags behind each registry command"),
        (name = "acp-agents", description = "The ACP agent registry: what's on PATH or configured, and what discovery found"),
        (name = "skills", description = "The documents an agent loads to do one kind of work"),
        (name = "repositories", description = "Git repositories registered with the daemon"),
        (name = "memories", description = "Searchable facts learned about one repository"),
        (name = "goals", description = "Goals and their plans"),
        (name = "tasks", description = "Tasks, transitions, and what their agents say"),
        (name = "sessions", description = "Agent sessions, and the console each one is driven through"),
        (name = "events", description = "Agent events the ACP runtime reports, and the live domain-event stream"),
        (name = "models", description = "The model catalog discovery found each registry agent offering"),
        (name = "logs", description = "The daemon's own process log"),
    )
)]
struct ApiDoc;

/// Build the daemon router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/version", get(version))
        .route("/v1/doctor", get(doctor::report))
        // agents
        .route("/v1/agents", get(agents::list))
        .route("/v1/agents/{id}", put(agents::update))
        .route("/v1/acp-agents", get(acp_agents::list))
        .route("/v1/acp-agents/refresh", post(acp_agents::refresh))
        // skills
        .route("/v1/skills", post(skills::create).get(skills::list))
        .route(
            "/v1/skills/{name}",
            get(skills::get).put(skills::update).delete(skills::delete),
        )
        .route(
            "/v1/skills/{name}/document/reset",
            post(skills::reset_document),
        )
        // repositories
        .route(
            "/v1/repositories",
            post(repositories::create).get(repositories::list),
        )
        .route(
            "/v1/repositories/{id}",
            get(repositories::get)
                .put(repositories::update)
                .delete(repositories::delete),
        )
        .route(
            "/v1/repositories/{repository_id}/memories",
            post(memories::create).get(memories::list),
        )
        .route(
            "/v1/repositories/{repository_id}/memories/search",
            get(memories::search),
        )
        .route(
            "/v1/repositories/{repository_id}/memories/{id}",
            axum::routing::delete(memories::delete),
        )
        // goals
        .route("/v1/goals", post(goals::create).get(goals::list))
        .route("/v1/goals/{id}", get(goals::get).delete(goals::delete))
        .route("/v1/goals/{id}/cancel", post(goals::cancel))
        .route("/v1/goals/{id}/finalize", post(goals::finalize))
        .route("/v1/goals/{id}/complete", post(goals::complete))
        .route(
            "/v1/goals/{id}/messages",
            get(goals::list_goal_messages).post(goals::post_goal_message),
        )
        .route("/v1/goals/{goal_id}/tasks", post(tasks::create))
        // tasks
        .route("/v1/tasks", get(tasks::list))
        .route("/v1/tasks/{id}", get(tasks::get).patch(tasks::update))
        .route(
            "/v1/tasks/{id}/transitions",
            post(tasks::transition).get(tasks::list_transitions),
        )
        .route("/v1/tasks/{id}/cancel", post(tasks::cancel))
        .route("/v1/tasks/{id}/retry", post(tasks::retry))
        .route(
            "/v1/tasks/{id}/author-session",
            post(tasks::adopt_author_session),
        )
        .route(
            "/v1/tasks/{id}/messages",
            get(landing::list_task_messages).post(landing::post_task_message),
        )
        .route("/v1/tasks/{id}/diff", get(landing::diff))
        .route("/v1/tasks/{id}/pick", post(landing::pick_winner))
        .route(
            "/v1/tasks/{id}/pull-request",
            post(landing::record_pull_request),
        )
        // sessions
        .route("/v1/sessions", get(sessions::list))
        .route("/v1/outside-sessions", get(sessions::list_outside))
        .route("/v1/sessions/{id}", get(sessions::get))
        .route("/v1/sessions/{id}/kill", post(sessions::kill))
        .route("/v1/sessions/{id}/resume", post(sessions::resume))
        .route("/v1/sessions/{id}/console", get(console::snapshot))
        .route("/v1/sessions/{id}/console/stream", get(console::stream))
        .route("/v1/sessions/{id}/console/input", post(console::input))
        // models
        .route("/v1/models", get(models::list))
        .route("/v1/models/enabled", put(models::set_enabled))
        // daemon logs
        .route("/v1/logs", get(logs::snapshot))
        .route("/v1/logs/stream", get(logs::stream))
        // events
        .route("/v1/events", get(events::list))
        .route("/v1/events/stream", get(stream::stream))
        // debug spawn (manual agent launch until the scheduler lands)
        .route("/internal/spawn", post(sessions::debug_spawn))
        // docs (SwaggerUi also serves the spec at /api-docs/openapi.json)
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
        // Wide open on purpose: the trust boundary is the unix socket / the
        // loopback bind (see auth.rs), not the browser origin. Without this a
        // webview (`tauri://localhost`, `http://localhost:*`) cannot call the
        // TCP listener at all.
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Daemon liveness probe.
#[utoipa::path(get, path = "/v1/health", tag = "system",
    responses((status = 200, description = "Daemon is healthy", body = HealthResponse)))]
async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        uptime_secs: state.started_at.elapsed().as_secs(),
    })
}

/// Daemon name and version.
#[utoipa::path(get, path = "/v1/version", tag = "system",
    responses((status = 200, description = "Daemon version", body = VersionResponse)))]
async fn version() -> Json<VersionResponse> {
    Json(VersionResponse {
        name: "ariadned".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}
