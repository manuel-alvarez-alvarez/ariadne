//! Text from one agent put straight into the session of another (spec 039).
//!
//! A post is a prompt in a live session and nothing else: it writes no row,
//! answers no pending permission or question, and clears no attention.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};

use ariadne_api::messages::PostToSessionRequest;
use ariadne_core::{PromptKind, Seat};
use ariadne_store::{AgentSession, SessionFilter, Task};

use super::AppState;
use super::caller::{call_ctx, ensure_task_scope};
use super::error::{ApiError, ApiResult, Json};
use crate::agents::prompts;

/// Put the body into the live session of the orchestrator or of the agent
/// of one column, on one task of this host.
///
/// Each refusal is a bad request with the sentence the caller reads.
#[utoipa::path(post, path = "/v1/tasks/{id}/posts", tag = "tasks",
    request_body = PostToSessionRequest,
    params(("id" = String, Path, description = "task id")),
    responses((status = 204), (status = 400), (status = 403)))]
pub(super) async fn post_to_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<PostToSessionRequest>,
) -> ApiResult<StatusCode> {
    let ctx = call_ctx(&state.store, &headers).await?;
    let Some(sender) = &ctx.session else {
        return Err(ApiError::bad_request(
            "a post is from an agent session, and this call has none",
        ));
    };
    if !matches!(sender.seat(), Some(Seat::Orchestrator | Seat::Agent)) {
        return Err(ApiError::bad_request(
            "only the orchestrator and the agent of a column post to a session",
        ));
    }
    let body = req.body.trim();
    if body.is_empty() {
        return Err(ApiError::bad_request("a post needs a body"));
    }
    let task = state.store.get_task(&id).await?;
    if sender.goal_id.as_deref() != Some(task.goal_id.as_str()) {
        return Err(ApiError::bad_request(format!(
            "task {id} is a task of another goal, so you cannot post to it"
        )));
    }
    ensure_task_scope(&ctx, &id)
        .map_err(|_| ApiError::bad_request(format!("task {id} is not your own task")))?;
    if task.is_far() {
        return Err(ApiError::bad_request(format!(
            "task {id} runs on another host, and a post stays on one host"
        )));
    }
    let agents = state.store.list_task_agents(&id).await?;
    let roll_call = || {
        let columns: Vec<&str> = agents.iter().map(|a| a.step.as_str()).collect();
        format!("`orchestrator`, or one of: {}", columns.join(", "))
    };
    let recipient = if req.to.trim().eq_ignore_ascii_case("orchestrator") {
        live_session(
            &state,
            SessionFilter {
                goal_id: Some(task.goal_id.clone()),
                live_only: true,
                ..Default::default()
            },
            |s| s.seat() == Some(Seat::Orchestrator),
        )
        .await?
        .ok_or_else(|| ApiError::bad_request("the orchestrator has no live session"))?
    } else {
        let Some(agent) = agents
            .iter()
            .find(|a| a.step.eq_ignore_ascii_case(req.to.trim()))
        else {
            return Err(ApiError::bad_request(format!(
                "task {id} staffs no column {}. Say {}",
                req.to.trim(),
                roll_call()
            )));
        };
        if task.step.as_deref() != Some(agent.step.as_str()) {
            return Err(ApiError::bad_request(format!(
                "the {} column is not the current column of task {id}. Say {}",
                agent.step,
                current_address(&task)
            )));
        }
        live_session(
            &state,
            SessionFilter {
                task_id: Some(id.clone()),
                live_only: true,
                ..Default::default()
            },
            |s| s.task_agent_id.as_deref() == Some(agent.id.as_str()),
        )
        .await?
        .ok_or_else(|| {
            ApiError::bad_request(format!("the {} agent has no live session", agent.step))
        })?
    };
    let (from, answer) = match sender.seat() {
        Some(Seat::Orchestrator) => (
            "your orchestrator".to_string(),
            "to `orchestrator`".to_string(),
        ),
        _ => {
            let column = match &sender.task_agent_id {
                Some(agent_id) => agents
                    .iter()
                    .find(|a| &a.id == agent_id)
                    .map(|a| a.step.clone()),
                None => None,
            }
            .ok_or_else(|| ApiError::bad_request("your session is staffed on no column"))?;
            (
                format!(r#"the {column} agent of task {id} "{}""#, task.title),
                format!("to `{column}` and task `{id}`"),
            )
        }
    };
    let text = prompts::post_briefing(&from, body, &answer);
    state
        .launcher
        .acp
        .send_prompt_of_kind(&recipient.id, text, PromptKind::IncomingMessage)
        .map_err(|_| ApiError::bad_request("the recipient has no live session to take the post"))?;
    Ok(StatusCode::NO_CONTENT)
}

fn current_address(task: &Task) -> String {
    match task.step.as_deref() {
        Some(step) => format!("`{step}`, the current column, or `orchestrator`"),
        None => "`orchestrator`: the task has no current column".to_string(),
    }
}

async fn live_session(
    state: &AppState,
    filter: SessionFilter,
    pick: impl Fn(&AgentSession) -> bool,
) -> ApiResult<Option<AgentSession>> {
    Ok(state
        .store
        .list_sessions(filter)
        .await?
        .into_iter()
        .find(pick))
}
