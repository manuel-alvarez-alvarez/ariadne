//! Axum application: routes, shared state, OpenAPI document.

mod catalog;
mod classify;
pub(crate) mod convert;
mod doctor;
mod error;
mod events;
mod goals;
mod landing;
mod logs;
mod pane;
mod profiles;
mod recipients;
mod repositories;
mod session_logs;
mod sessions;
mod sse;
mod stream;
mod tasks;

use std::sync::Arc;
use std::time::Instant;

use axum::extract::State;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use ariadne_api::{HealthResponse, VersionResponse};
use ariadne_store::Store;

use catalog::{agents, models};

use crate::bus::EventBus;
use crate::launcher::Launcher;
use crate::log::LogBuffer;
use crate::scheduler::SchedEvent;

/// Shared handler state.
#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub started_at: Instant,
    pub launcher: Arc<Launcher>,
    /// Present once the scheduler is running; handlers poke it after writes.
    pub sched_tx: Option<mpsc::UnboundedSender<SchedEvent>>,
    /// Fan-out of domain events to `/v1/events/stream` subscribers.
    pub events: EventBus,
    /// Recent daemon log lines, served by `/v1/logs`.
    pub logs: LogBuffer,
}

impl AppState {
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

    /// A message was posted: whoever it addresses is woken with it.
    pub fn notify_scheduler_message(&self, message_id: &str) {
        self.wake(SchedEvent::MessagePosted(message_id.to_string()));
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
        agents::list, agents::update,
        profiles::create, profiles::list, profiles::get, profiles::update, profiles::delete,
        profiles::list_prompts, profiles::update_prompt, profiles::reset_prompt,
        profiles::reset_system_prompt,
        repositories::create, repositories::list, repositories::get,
        repositories::update, repositories::delete,
        goals::create, goals::list, goals::get, goals::delete,
        goals::cancel, goals::submit, goals::finalize,
        goals::list_messages, goals::post_message,
        tasks::create, tasks::list, tasks::get, tasks::update,
        tasks::transition, tasks::cancel, tasks::retry, tasks::list_transitions,
        tasks::list_messages, tasks::post_message,
        landing::list_reviews, landing::post_review, landing::diff,
        landing::record_pull_request,
        sessions::list, sessions::get, sessions::kill, sessions::resume,
        sessions::input, sessions::resize, sessions::logs,
        session_logs::logs_stream,
        events::list, stream::stream,
        models::list,
        logs::snapshot, logs::stream,
    ),
    components(schemas(
        ariadne_api::stream::DomainEvent, ariadne_api::stream::ResyncDto,
        ariadne_api::sessions::SessionLogChunk, ariadne_api::sessions::SessionLogEnd,
        ariadne_api::sessions::SessionPaneSize,
        ariadne_api::logs::LogLineDto, ariadne_api::logs::LogSnapshotResponse,
    )),
    tags(
        (name = "system", description = "Daemon health and metadata"),
        (name = "agents", description = "Per-agent-CLI launch configuration"),
        (name = "profiles", description = "Agent profiles (role + system prompt + agent CLI)"),
        (name = "repositories", description = "Git repositories registered with the daemon"),
        (name = "goals", description = "Goals and their planning threads"),
        (name = "tasks", description = "Tasks, transitions, conversations, reviews"),
        (name = "sessions", description = "Agent sessions (tmux-hosted)"),
        (name = "events", description = "Raw agent events from hooks, and the live domain-event stream"),
        (name = "models", description = "Model catalogs per agent CLI"),
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
        .route("/v1/agents/{kind}", put(agents::update))
        // profiles
        .route("/v1/profiles", post(profiles::create).get(profiles::list))
        .route(
            "/v1/profiles/{id}",
            get(profiles::get)
                .put(profiles::update)
                .delete(profiles::delete),
        )
        .route("/v1/profiles/{id}/prompts", get(profiles::list_prompts))
        .route(
            "/v1/profiles/{id}/prompts/{kind}",
            put(profiles::update_prompt),
        )
        .route(
            "/v1/profiles/{id}/prompts/{kind}/reset",
            post(profiles::reset_prompt),
        )
        .route(
            "/v1/profiles/{id}/system-prompt/reset",
            post(profiles::reset_system_prompt),
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
        // goals
        .route("/v1/goals", post(goals::create).get(goals::list))
        .route("/v1/goals/{id}", get(goals::get).delete(goals::delete))
        .route("/v1/goals/{id}/cancel", post(goals::cancel))
        .route("/v1/goals/{id}/submit", post(goals::submit))
        .route("/v1/goals/{id}/finalize", post(goals::finalize))
        .route(
            "/v1/goals/{id}/messages",
            get(goals::list_messages).post(goals::post_message),
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
            "/v1/tasks/{id}/messages",
            get(tasks::list_messages).post(tasks::post_message),
        )
        .route(
            "/v1/tasks/{id}/reviews",
            get(landing::list_reviews).post(landing::post_review),
        )
        .route("/v1/tasks/{id}/diff", get(landing::diff))
        .route(
            "/v1/tasks/{id}/pull-request",
            post(landing::record_pull_request),
        )
        // sessions
        .route("/v1/sessions", get(sessions::list))
        .route("/v1/sessions/{id}", get(sessions::get))
        .route("/v1/sessions/{id}/kill", post(sessions::kill))
        .route("/v1/sessions/{id}/resume", post(sessions::resume))
        .route("/v1/sessions/{id}/input", post(sessions::input))
        .route("/v1/sessions/{id}/resize", post(sessions::resize))
        .route("/v1/sessions/{id}/logs", get(sessions::logs))
        .route(
            "/v1/sessions/{id}/logs/stream",
            get(session_logs::logs_stream),
        )
        // models
        .route("/v1/models", get(models::list))
        // daemon logs
        .route("/v1/logs", get(logs::snapshot))
        .route("/v1/logs/stream", get(logs::stream))
        // events
        .route("/v1/events", get(events::list))
        .route("/v1/events/stream", get(stream::stream))
        .route("/internal/agent-events", post(events::ingest))
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
