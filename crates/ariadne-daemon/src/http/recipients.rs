//! Resolving the addressee of a message against the thread it is posted to.
//!
//! A message may name one addressee: a profile, by id or by name the way tasks
//! name theirs, or the literal `"user"`. Which profiles a thread can address
//! is the set of people working in it, so that a message never names someone
//! who will not read it — and so that the wake path a recipient exists for has
//! a session to look for.

use ariadne_store::{Goal, Profile, Recipient, Store, StoreError, Task};

use super::error::{ApiError, ApiResult};

/// The addressee that is not a profile: the human user.
pub const USER: &str = "user";

/// Who may be addressed in a goal's planning thread: its planner, or the user.
///
/// Engineers and reviewers are addressed in the task threads they work in,
/// where which of their tasks is meant is not in question.
pub async fn goal_participants(store: &Store, goal: &Goal) -> Result<Vec<Profile>, StoreError> {
    Ok(vec![store.get_profile(&goal.planner_profile_id).await?])
}

/// Who may be addressed in a task's thread: everyone working on the task — its
/// engineer, its reviewers and the planner that wrote it — or the user.
pub async fn task_participants(store: &Store, task: &Task) -> Result<Vec<Profile>, StoreError> {
    let goal = store.get_goal(&task.goal_id).await?;
    let mut participants = vec![store.get_profile(&task.engineer_profile_id).await?];
    for pin in store.list_task_reviewer_pins(&task.id).await? {
        participants.push(store.get_profile(&pin.profile_id).await?);
    }
    participants.push(store.get_profile(&goal.planner_profile_id).await?);
    Ok(participants)
}

/// Resolve what a `to` field names against the thread's `participants`.
///
/// An addressee the thread has no one to deliver to is refused rather than
/// quietly dropped, and the refusal names everyone it could have addressed
/// instead.
pub async fn resolve(store: &Store, to: &str, participants: &[Profile]) -> ApiResult<Recipient> {
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
