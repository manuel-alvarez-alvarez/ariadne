//! Task endpoints: CRUD, transitions, messages, reviews.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};

use ariadne_api::Page;
use ariadne_api::messages::{CreateMessageRequest, MessageDto};
use ariadne_api::reviews::{CreateReviewRequest, ReviewDto};
use ariadne_api::tasks::{
    CreateTaskRequest, RecordPullRequestRequest, ReturnToEngineerRequest, TaskDto, TaskListQuery,
    TaskTransitionDto, TransitionRequest, UpdateTaskRequest,
};
use ariadne_core::{Actor, AttentionReason, AuthorRole, ReviewVerdict, Role, TaskStatus};
use ariadne_store::{
    NewMessage, NewReview, NewTask, Recipient, Store, Task, TaskFilter, TaskUpdate,
};

use super::AppState;
use super::auth::{CallCtx, call_ctx, ensure_task_scope};
use super::convert::{message_dto_of, message_dtos, review_dto, task_dto, transition_dto};
use super::error::{ApiError, ApiResult};
use super::recipients;
use crate::forge::{self, Forge};

async fn to_dto(store: &Store, task: Task) -> ApiResult<TaskDto> {
    let reviewers = store.list_task_reviewer_pins(&task.id).await?;
    let deps = store.list_task_dependencies(&task.id).await?;
    Ok(task_dto(task, reviewers, deps))
}

/// Resolve a list of profile ids-or-names, checking each has `role`.
async fn resolve_profiles(store: &Store, specs: &[String], role: Role) -> ApiResult<Vec<String>> {
    let mut ids = Vec::with_capacity(specs.len());
    for spec in specs {
        let p = store.resolve_profile(spec).await?;
        if p.role() != role {
            return Err(ApiError::bad_request(format!(
                "profile {} has role {}, expected {}",
                p.name,
                p.role,
                role.as_str()
            )));
        }
        ids.push(p.id);
    }
    Ok(ids)
}

/// Resolve one profile id-or-name, checking it has `role`: the shape every
/// single-profile assignment takes.
async fn resolve_profile(store: &Store, spec: &str, role: Role) -> ApiResult<String> {
    let specs = [spec.to_string()];
    Ok(resolve_profiles(store, &specs, role).await?.remove(0))
}

/// Create a task in a goal (planner via MCP, or the user).
#[utoipa::path(post, path = "/v1/goals/{goal_id}/tasks", tag = "tasks",
    request_body = CreateTaskRequest,
    params(("goal_id" = String, Path, description = "goal id")),
    responses((status = 201, body = TaskDto), (status = 400), (status = 409)))]
pub async fn create(
    State(state): State<AppState>,
    Path(goal_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<CreateTaskRequest>,
) -> ApiResult<(StatusCode, Json<TaskDto>)> {
    let ctx = call_ctx(&state.store, &headers).await?;
    if !matches!(ctx.actor, Actor::Planner | Actor::User) {
        return Err(ApiError::forbidden(
            "only the planner or the user may create tasks",
        ));
    }
    if let Some(session) = &ctx.session
        && session.goal_id != goal_id
    {
        return Err(ApiError::forbidden("session belongs to a different goal"));
    }

    let goal = state.store.get_goal(&goal_id).await?;
    let repos = state.store.list_goal_repositories(&goal.id).await?;
    let repo_id = match &req.repo_id {
        Some(id) => {
            if !repos.iter().any(|r| &r.id == id) {
                return Err(ApiError::bad_request(format!(
                    "repo {id} does not belong to goal {goal_id}"
                )));
            }
            id.clone()
        }
        None if repos.len() == 1 => repos[0].id.clone(),
        None => {
            return Err(ApiError::bad_request(
                "goal has multiple repos; specify repo_id",
            ));
        }
    };

    let engineer = resolve_profile(&state.store, &req.engineer_profile, Role::Engineer).await?;
    let reviewers = resolve_profiles(&state.store, &req.reviewer_profiles, Role::Reviewer).await?;
    let integrator =
        resolve_profile(&state.store, &req.integrator_profile, Role::Integrator).await?;

    let task = state
        .store
        .create_task(NewTask {
            goal_id: goal.id,
            repo_id,
            title: req.title,
            description: req.description,
            engineer_profile_id: engineer,
            integrator_profile_id: integrator,
            reviewer_profile_ids: reviewers,
            depends_on: req.depends_on,
        })
        .await?;
    let dto = to_dto(&state.store, task).await?;
    Ok((StatusCode::CREATED, Json(dto)))
}

/// List tasks.
#[utoipa::path(get, path = "/v1/tasks", tag = "tasks",
    params(TaskListQuery),
    responses((status = 200, body = [TaskDto])))]
pub async fn list(
    State(state): State<AppState>,
    Query(q): Query<TaskListQuery>,
) -> ApiResult<Json<Vec<TaskDto>>> {
    let tasks = state
        .store
        .list_tasks(TaskFilter {
            goal_id: q.goal,
            status: q.status,
        })
        .await?;
    let mut out = Vec::with_capacity(tasks.len());
    for task in tasks {
        out.push(to_dto(&state.store, task).await?);
    }
    Ok(Json(out))
}

/// Inspect a task.
#[utoipa::path(get, path = "/v1/tasks/{id}", tag = "tasks",
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = TaskDto), (status = 404)))]
pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<TaskDto>> {
    let task = state.store.get_task(&id).await?;
    Ok(Json(to_dto(&state.store, task).await?))
}

/// Edit a pending/ready task (planner or user).
#[utoipa::path(patch, path = "/v1/tasks/{id}", tag = "tasks",
    request_body = UpdateTaskRequest,
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = TaskDto), (status = 404), (status = 409)))]
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<UpdateTaskRequest>,
) -> ApiResult<Json<TaskDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    if !matches!(ctx.actor, Actor::Planner | Actor::User) {
        return Err(ApiError::forbidden(
            "only the planner or the user may edit tasks",
        ));
    }
    let reviewer_profile_ids = match &req.reviewer_profiles {
        Some(specs) => Some(resolve_profiles(&state.store, specs, Role::Reviewer).await?),
        None => None,
    };
    let integrator_profile_id = match &req.integrator_profile {
        Some(spec) => Some(resolve_profile(&state.store, spec, Role::Integrator).await?),
        None => None,
    };
    let task = state
        .store
        .update_task(
            &id,
            TaskUpdate {
                title: req.title,
                description: req.description,
                reviewer_profile_ids,
                integrator_profile_id,
            },
        )
        .await?;
    if let Some(deps) = req.depends_on {
        state.store.set_task_dependencies(&id, &deps).await?;
    }
    let task = state.store.get_task(&task.id).await?;
    Ok(Json(to_dto(&state.store, task).await?))
}

/// Request a status transition. The actor is derived from the call context.
#[utoipa::path(post, path = "/v1/tasks/{id}/transitions", tag = "tasks",
    request_body = TransitionRequest,
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = TaskDto), (status = 404), (status = 409)))]
pub async fn transition(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<TransitionRequest>,
) -> ApiResult<Json<TaskDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_task_scope(&ctx, &id)?;
    let task = apply_transition(&state, &ctx, &id, req).await?;
    Ok(Json(to_dto(&state.store, task).await?))
}

pub(crate) async fn apply_transition(
    state: &AppState,
    ctx: &CallCtx,
    task_id: &str,
    req: TransitionRequest,
) -> ApiResult<Task> {
    // `merged` is never taken on faith.
    if req.to == TaskStatus::Merged {
        let task = state.store.get_task(task_id).await?;
        let repo = state.store.get_repository(&task.repo_id).await?;
        verify_merged(state, &task, &repo, req.merge_commit.as_deref()).await?;
    }
    let task = state
        .store
        .transition_task(
            task_id,
            req.to,
            ctx.actor,
            req.reason.as_deref(),
            req.merge_commit.as_deref(),
        )
        .await?;
    state.notify_scheduler(task_id).await;
    Ok(task)
}

/// Prove the task really was landed, the way it was landed.
///
/// Locally that is the branch being an ancestor of the base: rebase, squash
/// and fast-forward leave the branch tip *as* the base tip. A pull or merge
/// request leaves no such thing — a squash or rebase merge on either forge
/// writes a commit nobody's branch points at — so what is checked there is
/// the forge's own answer, plus the local base having caught up with it: a
/// task whose branch merged on a forge is only finished here once the
/// checkout says so too.
async fn verify_merged(
    state: &AppState,
    task: &Task,
    repo: &ariadne_store::Repository,
    reported: Option<&str>,
) -> ApiResult<()> {
    let repo_path = std::path::PathBuf::from(&repo.path);
    let Some(watched) = forge::watched_pull_request(task) else {
        let merged = state
            .launcher
            .git
            .is_ancestor(&repo_path, &task.branch, &repo.base_branch)
            .await
            .map_err(|e| ApiError::conflict(e.to_string()))?;
        if !merged {
            return Err(ApiError::conflict(format!(
                "merge not verified: {} is not an ancestor of {} in {}",
                task.branch, repo.base_branch, repo.path
            )));
        }
        return Ok(());
    };

    // Whichever forge it is on, the same two facts: whether it was merged,
    // and the commit the merge landed as.
    let landing = match watched.forge {
        Forge::GitHub => state
            .launcher
            .gh()
            .pr_view(&repo_path, &watched)
            .await
            .map(|pr| pr.landing()),
        Forge::GitLab => state
            .launcher
            .glab()
            .mr_view(&repo_path, &watched)
            .await
            .map(|mr| mr.landing()),
    }
    .map_err(|e| ApiError::conflict(format!("merge not verified: {e:#}")))?;

    let label = watched.label();
    if !landing.merged {
        return Err(ApiError::conflict(format!(
            "merge not verified: {label} is {}, not merged",
            landing.state
        )));
    }
    // Merged there, but the task is landed here: both the sha being reported
    // and the commit the forge says the merge landed as have to be on the
    // local base branch, which is what says the checkout has caught up with
    // the remote. Everything the integrator is told to do — fetch,
    // fast-forward, report `git rev-parse <base>` — makes both true at once.
    let mut contained = Vec::new();
    contained.extend(reported.map(str::to_string));
    contained.extend(landing.commits);
    for commit in contained {
        let caught_up = state
            .launcher
            .git
            .is_ancestor(&repo_path, &commit, &repo.base_branch)
            .await
            .map_err(|e| ApiError::conflict(e.to_string()))?;
        if !caught_up {
            return Err(ApiError::conflict(format!(
                "merge not verified: {label} landed as {commit}, which {} in {} \
                 does not contain yet — fetch the remote and fast-forward it first",
                repo.base_branch, repo.path
            )));
        }
    }
    Ok(())
}

/// Record the pull request an integrator opened for a task.
///
/// The daemon watches what it is told here, and only here: the URL travels as
/// a tool call rather than as a sentence in the conversation, so that a
/// pull request is either being watched or was never reported — never
/// half-known from a message somebody has to parse.
#[utoipa::path(post, path = "/v1/tasks/{id}/pull-request", tag = "tasks",
    request_body = RecordPullRequestRequest,
    params(("id" = String, Path, description = "task id")),
    responses(
        (status = 200, body = TaskDto),
        (status = 400, description = "not a pull request URL"),
        (status = 403, description = "not an integrator session"),
        (status = 409, description = "the task is not being integrated")
    ))]
pub async fn record_pull_request(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<RecordPullRequestRequest>,
) -> ApiResult<Json<TaskDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_task_scope(&ctx, &id)?;
    if !ctx
        .session
        .as_ref()
        .is_some_and(|s| s.role() == Role::Integrator)
    {
        return Err(ApiError::forbidden(
            "only the integrator of a task may record its pull request",
        ));
    }
    let task = state.store.get_task(&id).await?;
    if task.status() != TaskStatus::Integrating {
        return Err(ApiError::conflict(format!(
            "task is {}, a pull request belongs to a task being integrated",
            task.status
        )));
    }
    let url = req.url.trim();
    let Some(number) = forge::pull_request_number(url) else {
        return Err(ApiError::bad_request(format!(
            "{url} is not a pull request URL: pass the one `gh pr create` or \
             `glab mr create` printed, e.g. https://github.com/owner/repo/pull/12 \
             or https://gitlab.com/owner/repo/-/merge_requests/12"
        )));
    };
    let announce = task.pr_url.as_deref() != Some(url);
    state.store.set_task_pull_request(&id, number, url).await?;
    if announce {
        announce_pull_request(&state, &ctx, &task, number, url).await?;
    }
    // The scheduler starts watching it on the next reconciliation rather than
    // on the poll interval, so the first look is immediate.
    state.notify_scheduler(&id).await;
    let task = state.store.get_task(&id).await?;
    Ok(Json(to_dto(&state.store, task).await?))
}

/// Tell the user a pull request was opened for them, as the request itself is
/// recorded rather than as something an agent has to remember to say.
///
/// A published task is the user's from here: nothing in Ariadne merges it, and
/// until this the only trace of one was whatever the integrator happened to
/// write into the thread, addressed to nobody and so waking nobody.
///
/// Announced once per request — a re-reported URL is the same request, and
/// [`Store::set_task_pull_request`] is idempotent for exactly that reason. The
/// announcement counts as the telling that `pr_approved_notified` records: on
/// a repository that gates nothing the first poll reads the request as
/// approved from the moment it exists, and this is that same news a moment
/// earlier. Where a review *is* required the poll reads it as not approved and
/// clears the flag again, so the approval, when it comes, is still announced.
///
/// The attention flag is raised here and again by every poll of the request
/// ([`crate::scheduler`]): raised here it is the integrator's own next event
/// that takes it down — `post_tool_use` on a live session clears attention,
/// and this runs mid-turn, one tool call before the turn ends.
async fn announce_pull_request(
    state: &AppState,
    ctx: &CallCtx,
    task: &Task,
    number: i64,
    url: &str,
) -> ApiResult<()> {
    let watched = forge::forge_of(url);
    let label = match watched {
        Some(forge) => forge::WatchedPr {
            forge,
            number,
            url: url.to_string(),
        }
        .label(),
        // A forge with no watcher has no vocabulary of its own here either.
        None => format!("Pull request #{number}"),
    };
    // A forge Ariadne cannot poll is one nothing will ever say more about, so
    // the notice says so rather than promising a watch there is none of.
    let from_here = match watched {
        Some(forge) => format!(
            "It is yours from here — Ariadne watches the {}, relays what is said on it \
             and finishes the task once it is merged.",
            forge.noun()
        ),
        None => "It is yours from here. Ariadne does not watch this forge, so tell the \
                 integrator in this thread once it is merged."
            .to_string(),
    };
    state
        .store
        .create_message(NewMessage {
            goal_id: task.goal_id.clone(),
            task_id: Some(task.id.clone()),
            author_role: AuthorRole::System,
            author_session_id: None,
            recipient: Some(Recipient::User),
            body: format!(
                "{label} is open for \"{}\": {url}\n\n{from_here}",
                task.title
            ),
        })
        .await?;
    state
        .store
        .set_task_pr_approved_notified(&task.id, true)
        .await?;
    if let Some(session) = &ctx.session {
        state
            .store
            .set_session_attention(&session.id, AttentionReason::WaitingInput)
            .await?;
    }
    Ok(())
}

/// Cancel a task (user).
#[utoipa::path(post, path = "/v1/tasks/{id}/cancel", tag = "tasks",
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = TaskDto), (status = 409)))]
pub async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<TaskDto>> {
    let ctx = CallCtx::user();
    let task = apply_transition(
        &state,
        &ctx,
        &id,
        TransitionRequest {
            to: TaskStatus::Cancelled,
            reason: Some("cancelled by user".into()),
            merge_commit: None,
        },
    )
    .await?;
    Ok(Json(to_dto(&state.store, task).await?))
}

/// Retry a failed task (user): failed -> ready.
#[utoipa::path(post, path = "/v1/tasks/{id}/retry", tag = "tasks",
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = TaskDto), (status = 409)))]
pub async fn retry(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<TaskDto>> {
    let ctx = CallCtx::user();
    let task = apply_transition(
        &state,
        &ctx,
        &id,
        TransitionRequest {
            to: TaskStatus::Ready,
            reason: Some("retried by user".into()),
            merge_commit: None,
        },
    )
    .await?;
    Ok(Json(to_dto(&state.store, task).await?))
}

/// Transition audit log of a task.
#[utoipa::path(get, path = "/v1/tasks/{id}/transitions", tag = "tasks",
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = [TaskTransitionDto])))]
pub async fn list_transitions(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<TaskTransitionDto>>> {
    state.store.get_task(&id).await?;
    let rows = state.store.list_task_transitions(&id).await?;
    Ok(Json(rows.into_iter().map(transition_dto).collect()))
}

/// Task conversation.
#[utoipa::path(get, path = "/v1/tasks/{id}/messages", tag = "tasks",
    params(("id" = String, Path, description = "task id"), Page),
    responses((status = 200, body = [MessageDto])))]
pub async fn list_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(page): Query<Page>,
) -> ApiResult<Json<Vec<MessageDto>>> {
    state.store.get_task(&id).await?;
    let msgs = state
        .store
        .list_task_messages(&id, page.after.as_deref(), page.limit())
        .await?;
    Ok(Json(message_dtos(&state.store, msgs).await?))
}

/// Post into the task conversation.
#[utoipa::path(post, path = "/v1/tasks/{id}/messages", tag = "tasks",
    request_body = CreateMessageRequest,
    params(("id" = String, Path, description = "task id")),
    responses(
        (status = 201, body = MessageDto),
        (status = 400, description = "unknown addressee, or one taking no part in the task")
    ))]
pub async fn post_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<CreateMessageRequest>,
) -> ApiResult<(StatusCode, Json<MessageDto>)> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_task_scope(&ctx, &id)?;
    let task = state.store.get_task(&id).await?;
    let recipient = match &req.to {
        Some(to) => {
            let participants = recipients::task_participants(&state.store, &task).await?;
            Some(recipients::resolve(&state.store, to, &participants).await?)
        }
        None => None,
    };
    let msg = state
        .store
        .create_message(NewMessage {
            goal_id: task.goal_id,
            task_id: Some(id),
            author_role: ctx.author_role,
            author_session_id: ctx.session.map(|s| s.id),
            recipient,
            body: req.body,
        })
        .await?;
    // Addressed or not, the scheduler is told: what an unaddressed message
    // wakes is nobody, and that is its decision to make rather than the
    // handler's.
    state.notify_scheduler_message(&msg.id).await;
    Ok((
        StatusCode::CREATED,
        Json(message_dto_of(&state.store, msg).await?),
    ))
}

/// Reviews of a task.
#[utoipa::path(get, path = "/v1/tasks/{id}/reviews", tag = "tasks",
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, body = [ReviewDto])))]
pub async fn list_reviews(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<ReviewDto>>> {
    state.store.get_task(&id).await?;
    let rows = state.store.list_reviews(&id, None).await?;
    Ok(Json(rows.into_iter().map(review_dto).collect()))
}

/// Submit a review verdict for the current round.
#[utoipa::path(post, path = "/v1/tasks/{id}/reviews", tag = "tasks",
    request_body = CreateReviewRequest,
    params(("id" = String, Path, description = "task id")),
    responses((status = 201, body = ReviewDto), (status = 409)))]
pub async fn post_review(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<CreateReviewRequest>,
) -> ApiResult<(StatusCode, Json<ReviewDto>)> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_task_scope(&ctx, &id)?;
    let task = state.store.get_task(&id).await?;
    if task.status() != TaskStatus::UnderReview {
        return Err(ApiError::conflict(format!(
            "task is {}, reviews are only accepted under_review",
            task.status
        )));
    }

    // Reviewer identity: from the session, or explicit for user-submitted reviews.
    let reviewer_profile_id = match (&ctx.session, &req.reviewer_profile) {
        (Some(session), _) => {
            if session.role() != Role::Reviewer {
                return Err(ApiError::forbidden(
                    "only reviewer sessions may submit reviews",
                ));
            }
            session.profile_id.clone()
        }
        (None, Some(spec)) => state.store.resolve_profile(spec).await?.id,
        (None, None) => {
            return Err(ApiError::bad_request(
                "reviewer_profile is required for user-submitted reviews",
            ));
        }
    };
    let assigned = state.store.list_task_reviewers(&id).await?;
    if !assigned.contains(&reviewer_profile_id) {
        return Err(ApiError::forbidden(format!(
            "profile {reviewer_profile_id} is not an assigned reviewer of task {id}"
        )));
    }

    let review = state
        .store
        .create_review(NewReview {
            task_id: id.clone(),
            round: task.review_round,
            reviewer_profile_id,
            session_id: ctx.session.map(|s| s.id),
            verdict: req.verdict,
            body: req.body,
        })
        .await?;
    state.notify_scheduler(&id).await;
    Ok((StatusCode::CREATED, Json(review_dto(review))))
}

/// Hand an integrating task back to its engineer (integrator).
///
/// The feedback is recorded as a change-request verdict on the round that was
/// approved, so it reaches the engineer exactly the way a reviewer's does — in
/// the resume briefing, and in `get_reviews` beside the approvals it follows.
#[utoipa::path(post, path = "/v1/tasks/{id}/return-to-engineer", tag = "tasks",
    request_body = ReturnToEngineerRequest,
    params(("id" = String, Path, description = "task id")),
    responses(
        (status = 200, body = TaskDto),
        (status = 403, description = "not an integrator session"),
        (status = 409, description = "the task is not being integrated")
    ))]
pub async fn return_to_engineer(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<ReturnToEngineerRequest>,
) -> ApiResult<Json<TaskDto>> {
    let ctx = call_ctx(&state.store, &headers).await?;
    ensure_task_scope(&ctx, &id)?;
    let Some(session) = ctx
        .session
        .as_ref()
        .filter(|s| s.role() == Role::Integrator)
    else {
        return Err(ApiError::forbidden(
            "only the integrator of a task may send it back to its engineer",
        ));
    };
    let task = state.store.get_task(&id).await?;
    if task.status() != TaskStatus::Integrating {
        return Err(ApiError::conflict(format!(
            "task is {}, only a task being integrated can be sent back to its engineer",
            task.status
        )));
    }

    // The feedback is recorded before the transition, so that the engineer the
    // scheduler resumes on it has it to read. A transition that then fails
    // leaves a verdict on a round nobody is waiting on, which is inert — the
    // next round is a new one.
    state
        .store
        .create_review(NewReview {
            task_id: id.clone(),
            round: task.review_round,
            reviewer_profile_id: session.profile_id.clone(),
            session_id: Some(session.id.clone()),
            verdict: ReviewVerdict::RequestChanges,
            body: Some(feedback_body(&req)),
        })
        .await?;
    let task = apply_transition(
        &state,
        &ctx,
        &id,
        TransitionRequest {
            to: TaskStatus::ChangesRequested,
            reason: Some(req.summary),
            merge_commit: None,
        },
    )
    .await?;
    Ok(Json(to_dto(&state.store, task).await?))
}

/// The send-back as the engineer reads it: what happened, then the list of
/// what to do about it.
fn feedback_body(req: &ReturnToEngineerRequest) -> String {
    let mut body = req.summary.trim().to_string();
    if !req.changes.is_empty() {
        if !body.is_empty() {
            body.push_str("\n\n");
        }
        body.push_str(
            &req.changes
                .iter()
                .map(|change| format!("- {}", change.trim()))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    body
}

/// Diff of the task branch against its base (`git diff base...branch`), or,
/// once the task is merged, the diff its merge commit brought into the base —
/// after the merge the branch is contained in the base, so the three-dot diff
/// would be forever empty.
#[utoipa::path(get, path = "/v1/tasks/{id}/diff", tag = "tasks",
    params(("id" = String, Path, description = "task id")),
    responses((status = 200, content_type = "text/plain", body = String), (status = 404), (status = 409)))]
pub async fn diff(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<String> {
    let task = state.store.get_task(&id).await?;
    let repo = state.store.get_repository(&task.repo_id).await?;
    let repo_path = std::path::PathBuf::from(&repo.path);

    if let Some(merge_commit) = &task.merge_commit {
        // Also the only diff that still exists once the merged branch and
        // worktree have been cleaned up.
        return state
            .launcher
            .git
            .diff_against_first_parent(&repo_path, merge_commit)
            .await
            .map_err(|e| ApiError::conflict(e.to_string()));
    }

    if !state
        .launcher
        .git
        .branch_exists(&repo_path, &task.branch)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))?
    {
        return Err(ApiError::conflict(format!(
            "branch {} does not exist yet (task not started?)",
            task.branch
        )));
    }
    state
        .launcher
        .git
        .diff(&repo_path, &repo.base_branch, &task.branch)
        .await
        .map_err(|e| ApiError::conflict(e.to_string()))
}
