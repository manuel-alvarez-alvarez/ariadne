//! The current workflow agent hands work to an adjacent column.

use super::caller::{CallCtx, call_ctx};
use super::convert::task_dto_of;
use super::error::{ApiError, ApiResult, Json};
use super::{AppState, channel};
use ariadne_api::tasks::{CompleteStepRequest, FailStepRequest, TaskDto};
use ariadne_core::workflow::StepGate;
use ariadne_core::{Actor, Seat, TaskStatus};
use ariadne_store::Task;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};

pub(super) async fn current_agent(state: &AppState, ctx: &CallCtx, task: &Task) -> ApiResult<()> {
    let refused = || ApiError::forbidden("only the current column's agent can make this call");
    let session = ctx.session.as_ref().ok_or_else(refused)?;
    if session.seat() != Some(Seat::Agent) || session.task_id.as_deref() != Some(&task.id) {
        return Err(refused());
    }
    let agent = state
        .store
        .get_task_agent(session.task_agent_id.as_deref().ok_or_else(refused)?)
        .await?;
    if task.step.as_deref() != Some(agent.step.as_str()) || agent.task_id != task.id {
        return Err(refused());
    }
    Ok(())
}

fn gate_failed(message: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::CONFLICT, "step_gate_failed", message)
}

#[utoipa::path(post, path = "/v1/tasks/{id}/step/complete", tag = "tasks",
    request_body = CompleteStepRequest,
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = TaskDto), (status = 403), (status = 409, description = "step_gate_failed")))]
pub(super) async fn complete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<CompleteStepRequest>,
) -> ApiResult<Json<TaskDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    let task = state.store.get_task(&id).await?;
    current_agent(&state, &ctx, &task).await?;
    working(&task, &req.reason)?;
    let steps = state.store.goal_steps(&task.goal_id).await?;
    let at = steps
        .iter()
        .position(|s| Some(&s.id) == task.step.as_ref())
        .ok_or_else(|| ApiError::conflict("the task has no current column"))?;
    let repo = state.store.get_repository(&task.repo_id).await?;
    let path = std::path::Path::new(&repo.path);
    let gate = steps[at]
        .gate
        .as_deref()
        .map(str::parse::<StepGate>)
        .transpose()
        .map_err(|_| gate_failed("the stored column has an invalid gate"))?;
    let git = &state.launcher.git;
    match gate {
        Some(StepGate::Committed) => {
            let worktree = task
                .worktree_path
                .as_deref()
                .ok_or_else(|| gate_failed("the task has no worktree"))?;
            if !git
                .committed_clean(
                    path,
                    std::path::Path::new(worktree),
                    &repo.base_branch,
                    &task.branch,
                )
                .await
                .map_err(|_| gate_failed("the commit gate could not inspect the task branch"))?
            {
                return Err(gate_failed(
                    "commit the task change and leave the worktree clean",
                ));
            }
        }
        Some(StepGate::Pushed) => {
            let remote = repo.forge.as_ref().map_or("origin", |f| f.remote.as_str());
            if !git
                .remote_has_branch_tip(path, remote, &task.branch)
                .await
                .map_err(|_| gate_failed("the push gate could not read the remote branch"))?
            {
                return Err(gate_failed("push the task branch tip to the remote"));
            }
        }
        Some(StepGate::Merged) => {
            let sha = req
                .merge_commit
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| gate_failed("provide the merge commit on the base branch"))?;
            if !git
                .is_ancestor(path, sha, &repo.base_branch)
                .await
                .unwrap_or(false)
                || !git
                    .is_ancestor(path, &task.branch, &repo.base_branch)
                    .await
                    .unwrap_or(false)
            {
                return Err(gate_failed(
                    "the merge commit and task branch must be on the base branch",
                ));
            }
        }
        Some(StepGate::RequestMerged) => {
            let url = task
                .pr_url
                .as_deref()
                .ok_or_else(|| gate_failed("open the task request before completing this step"))?;
            if channel::request_state(&state, &task, url)
                .await
                .map_err(|_| gate_failed("the request gate could not read the forge"))?
                != "merged"
            {
                return Err(gate_failed("the task request must be merged"));
            }
        }
        None => {}
    }
    let task = if let Some(next) = steps.get(at + 1) {
        state
            .store
            .move_step(&id, &next.id, Actor::Agent, &req.reason)
            .await?
    } else {
        state
            .store
            .end_step(
                &id,
                &steps[at].id,
                TaskStatus::Finished,
                &req.reason,
                req.merge_commit.as_deref(),
            )
            .await?
    };
    state.notify_scheduler(&id);
    Ok(Json(task_dto_of(&state.store, task).await?))
}

#[utoipa::path(post, path = "/v1/tasks/{id}/step/fail", tag = "tasks",
    request_body = FailStepRequest,
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = TaskDto), (status = 403), (status = 409)))]
pub(super) async fn fail(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<FailStepRequest>,
) -> ApiResult<Json<TaskDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    let task = state.store.get_task(&id).await?;
    current_agent(&state, &ctx, &task).await?;
    working(&task, &req.reason)?;
    let steps = state.store.goal_steps(&task.goal_id).await?;
    let at = steps
        .iter()
        .position(|s| Some(&s.id) == task.step.as_ref())
        .ok_or_else(|| ApiError::conflict("the task has no current column"))?;
    let task = if at == 0 {
        state
            .store
            .end_step(&id, &steps[at].id, TaskStatus::Failed, &req.reason, None)
            .await?
    } else {
        state
            .store
            .move_step(&id, &steps[at - 1].id, Actor::Agent, &req.reason)
            .await?
    };
    state.notify_scheduler(&id);
    Ok(Json(task_dto_of(&state.store, task).await?))
}

fn working(task: &Task, reason: &str) -> ApiResult<()> {
    if task.status() != TaskStatus::InProgress {
        return Err(ApiError::conflict("the task must be in progress"));
    }
    if reason.trim().is_empty() {
        return Err(ApiError::bad_request("a step call needs a reason"));
    }
    Ok(())
}
