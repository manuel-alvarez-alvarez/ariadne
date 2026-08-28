//! Who a request is from, who it may be addressed to, and the one path a
//! message takes into a thread.
//!
//! Agent-originated requests (via the MCP server) carry `X-Ariadne-Session`;
//! everything else (CLI, curl, web) is the user. The unix socket is the trust
//! boundary — the header is context, not cryptographic auth.
//!
//! A message may name one addressee: a profile, by id or by name the way
//! tasks name theirs, or the literal `"user"`. Which profiles a thread can
//! address is the set of people working in it, so that a message never names
//! someone who will not read it — and so that the wake path a recipient
//! exists for has a session to look for.

use axum::Json;
use axum::http::{HeaderMap, StatusCode};

use ariadne_api::SESSION_HEADER;
use ariadne_api::messages::{CreateMessageRequest, MessageDto};
use ariadne_core::{Actor, AuthorRole, Role};
use ariadne_store::{AgentSession, Goal, NewMessage, Profile, Recipient, Store, StoreError, Task};

use super::AppState;
use super::convert::message_dto_of;
use super::error::{ApiError, ApiResult};

/// The addressee that is not a profile: the human user.
pub const USER: &str = "user";

/// Resolved identity of the caller.
pub struct CallCtx {
    pub actor: Actor,
    pub author_role: AuthorRole,
    /// Present when the call came from an agent session.
    pub session: Option<AgentSession>,
}

impl CallCtx {
    pub fn user() -> Self {
        Self {
            actor: Actor::User,
            author_role: AuthorRole::User,
            session: None,
        }
    }
}

pub async fn call_ctx(store: &Store, headers: &HeaderMap) -> ApiResult<CallCtx> {
    let Some(raw) = headers.get(SESSION_HEADER) else {
        return Ok(CallCtx::user());
    };
    let session_id = raw
        .to_str()
        .map_err(|_| ApiError::bad_request("invalid X-Ariadne-Session header"))?;
    let session = store
        .get_session(session_id)
        .await
        .map_err(|_| ApiError::forbidden(format!("unknown agent session: {session_id}")))?;
    let (actor, author_role) = match session.role() {
        Role::Planner => (Actor::Planner, AuthorRole::Planner),
        Role::Engineer => (Actor::Engineer, AuthorRole::Engineer),
        Role::Reviewer => (Actor::Reviewer, AuthorRole::Reviewer),
    };
    Ok(CallCtx {
        actor,
        author_role,
        session: Some(session),
    })
}

/// Ensure an agent session is scoped to the given task (users pass freely).
pub fn ensure_task_scope(ctx: &CallCtx, task_id: &str) -> ApiResult<()> {
    if let Some(session) = &ctx.session
        && session.task_id.as_deref() != Some(task_id)
        && session.role() != Role::Planner
    {
        return Err(ApiError::forbidden(format!(
            "session {} is not assigned to task {task_id}",
            session.id
        )));
    }
    Ok(())
}

/// A thread a message can be posted to, and the people working in it.
pub enum Thread {
    /// A goal's planning thread. Its planner is the only profile addressable
    /// there: engineers and reviewers are addressed in the task threads they
    /// work in, where which of their tasks is meant is not in question.
    Goal(Goal),
    /// A task's thread: its engineer, its reviewers and the planner that
    /// wrote it.
    Task(Task),
}

impl Thread {
    async fn participants(&self, store: &Store) -> Result<Vec<Profile>, StoreError> {
        let mut participants = Vec::new();
        let planner_of = match self {
            Self::Goal(goal) => goal.planner_profile_id.clone(),
            Self::Task(task) => {
                participants.push(store.get_profile(&task.engineer_profile_id).await?);
                for pin in store.list_task_reviewer_pins(&task.id).await? {
                    participants.push(store.get_profile(&pin.profile_id).await?);
                }
                store.get_goal(&task.goal_id).await?.planner_profile_id
            }
        };
        participants.push(store.get_profile(&planner_of).await?);
        Ok(participants)
    }

    /// The (goal, task) a message in this thread belongs to.
    fn ids(self) -> (String, Option<String>) {
        match self {
            Self::Goal(goal) => (goal.id, None),
            Self::Task(task) => (task.goal_id, Some(task.id)),
        }
    }
}

/// Write a message into `thread`, and tell the scheduler about it.
///
/// Addressed or not, the scheduler is told: what an unaddressed message wakes
/// is nobody, and that is its decision to make rather than the handler's.
///
/// A message the user wrote takes "waiting for you" down across the thread it
/// was written in, which is the answer to whatever was waiting for them there.
/// It happens here rather than on the scheduler's side of the notification
/// because an unaddressed message — which is most of what a person types — is
/// never delivered anywhere, and a rule kept there would only ever run for the
/// ones that name somebody.
pub async fn post(
    state: &AppState,
    ctx: CallCtx,
    thread: Thread,
    req: CreateMessageRequest,
) -> ApiResult<(StatusCode, Json<MessageDto>)> {
    let recipient = match &req.to {
        Some(to) => {
            let participants = thread.participants(&state.store).await?;
            Some(resolve(&state.store, to, &participants).await?)
        }
        None => None,
    };
    let (goal_id, task_id) = thread.ids();
    let author_role = ctx.author_role;
    let msg = state
        .store
        .create_message(NewMessage {
            goal_id: goal_id.clone(),
            task_id: task_id.clone(),
            author_role,
            author_session_id: ctx.session.map(|s| s.id),
            recipient,
            body: req.body,
        })
        .await?;
    if author_role == AuthorRole::User {
        state
            .store
            .clear_user_attention_in_thread(&goal_id, task_id.as_deref())
            .await?;
    }
    state.notify_scheduler_message(&msg.id);
    Ok((
        StatusCode::CREATED,
        Json(message_dto_of(&state.store, msg).await?),
    ))
}

/// Resolve what a `to` field names against the thread's `participants`.
///
/// An addressee the thread has no one to deliver to is refused rather than
/// quietly dropped, and the refusal names everyone it could have addressed
/// instead.
async fn resolve(store: &Store, to: &str, participants: &[Profile]) -> ApiResult<Recipient> {
    if to == USER {
        return Ok(Recipient::User);
    }
    match store.resolve_profile(to).await {
        Ok(profile) if participants.iter().any(|p| p.id == profile.id) => {
            Ok(Recipient::Profile(profile.id))
        }
        Ok(profile) => Err(refuse(
            format!("{} takes no part in this thread", profile.name),
            participants,
        )),
        Err(StoreError::NotFound { .. }) => Err(refuse(
            format!("no profile has the id or name {to}"),
            participants,
        )),
        Err(e) => Err(e.into()),
    }
}

fn refuse(why: String, participants: &[Profile]) -> ApiError {
    let mut addressable: Vec<&str> = participants.iter().map(|p| p.name.as_str()).collect();
    addressable.push(USER);
    ApiError::bad_request(format!("{why}; address one of: {}", addressable.join(", ")))
}
