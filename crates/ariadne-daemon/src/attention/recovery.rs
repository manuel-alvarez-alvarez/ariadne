//! The recovery producer: the first "Needs attention" path.
//!
//! What it reads is evidence the daemon already keeps for its own retry
//! loops — a session's switch chain, a task's own ended reason, a
//! repository's forge fetch error — rather than a new guess at a cause. A
//! blocker with no such evidence stays out: better an item missing than one
//! that names a cause nothing actually found.

use std::collections::BTreeMap;

use ariadne_api::attention::{
    AttentionCause, AttentionItemDto, AttentionProducer, AttentionSubjectDto, AttentionSubjectKind,
    AttentionTarget,
};
use ariadne_core::{AttentionReason, GoalStatus, Seat, SessionStatus, TaskStatus};
use ariadne_store::{Result, SessionFilter, Store, TaskFilter};

use crate::launcher::Launcher;
use crate::scheduler::DESCRIPTOR_LIMIT_REASON;
use crate::scheduler::SPAWN_RETRY_BUDGET;
use crate::scheduler::auto_switch::recovery_block;

use super::work_is_active_checked;

/// The fixed words [`crate::forge::Cli::binary`] gives a forge CLI the
/// daemon's PATH does not have. Matched exactly, since this is the
/// daemon's own message rather than a guess at the forge CLI's.
const FORGE_BINARY_MISSING: &str = "is not installed on the daemon's PATH";

/// Every recovery item this producer currently finds: a model exhausted
/// with no automatic switch left, every task whose own automatic retries
/// are spent (`failed`, which the daemon never retries on its own), and a
/// forge CLI missing from the daemon's own PATH.
pub(crate) async fn items(store: &Store, launcher: &Launcher) -> Result<Vec<AttentionItemDto>> {
    let mut items = quota_items(store, launcher).await?;
    items.extend(failed_task_items(store).await?);
    items.extend(configuration_items(store).await?);
    items.extend(access_items(store).await?);
    items.extend(orchestrator_given_up_items(store).await?);
    items.extend(reviewer_given_up_items(store).await?);
    Ok(items)
}

/// The most recent `session.error` this session reported, read the same
/// way `http::classify::agent_text` reads one off the live event stream —
/// the one piece of failure evidence a launch that could not be started
/// is likely to have left behind. `None` where it reported nothing at
/// all, which a launch that never got as far as the agent is free to do.
async fn last_session_error(store: &Store, session_id: &str) -> Result<Option<String>> {
    let events = store
        .list_events(ariadne_store::EventFilter {
            session_id: Some(session_id.to_string()),
            order: ariadne_store::EventOrder::Desc,
            limit: 50,
            ..Default::default()
        })
        .await?;
    Ok(events
        .iter()
        .find(|event| event.kind == "session.error")
        .and_then(|event| serde_json::from_str::<serde_json::Value>(&event.payload).ok())
        .and_then(|payload| {
            payload
                .pointer("/error/data/message")
                .and_then(|m| m.as_str())
                .map(str::to_string)
        }))
}

/// Every goal whose orchestrator `scheduler::goals::orchestrator_could_not_start`
/// has given up on (`Goal::orchestrator_given_up_at`), narrowed to goals
/// still `planning` or `active` — one cancelled or completed no longer
/// needs the recovery it names, whatever its last attempt left behind.
/// Raised beside a failed task's own item rather than instead of it: a
/// task that failed for its own reason (a test, a review) and an
/// orchestrator that will not start are two different problems with two
/// different actions, and suppressing this one on the strength of the
/// other existing would drop the one action — resume the orchestrator —
/// nothing else names. Each item names the goal's own alarm session, the
/// one row the give-up itself is raised on, or the last orchestrator
/// session the goal ever had where none carries the alarm any more, and
/// its summary carries the budget spent and the last error that session
/// itself reported, where it reported one.
async fn orchestrator_given_up_items(store: &Store) -> Result<Vec<AttentionItemDto>> {
    let mut items = Vec::new();
    for goal in store
        .list_goals(&[GoalStatus::Planning, GoalStatus::Active])
        .await?
    {
        let Some(since) = goal.orchestrator_given_up_at.clone() else {
            continue;
        };
        let orchestrators = store
            .list_sessions(SessionFilter {
                goal_id: Some(goal.id.clone()),
                ..Default::default()
            })
            .await?
            .into_iter()
            .filter(|s| s.seat() == Some(Seat::Orchestrator))
            .collect::<Vec<_>>();
        let alarm = orchestrators
            .iter()
            .find(|s| s.attention_reason() == Some(AttentionReason::Disconnected))
            .or_else(|| orchestrators.last());
        let Some(alarm) = alarm else {
            continue;
        };
        let error = last_session_error(store, &alarm.id).await?;
        let summary = match &error {
            Some(error) => format!(
                "The goal's orchestrator would not start after {SPAWN_RETRY_BUDGET} attempts: {error}"
            ),
            None => format!(
                "The goal's orchestrator would not start after {SPAWN_RETRY_BUDGET} attempts."
            ),
        };
        items.push(AttentionItemDto {
            id: format!("recovery:unknown:orchestrator:{}", goal.id),
            producer: AttentionProducer::Recovery,
            reason: AttentionCause::Unknown,
            summary,
            required_action: "Read why it will not start, then resume it yourself.".into(),
            since,
            affected: vec![AttentionSubjectDto {
                kind: AttentionSubjectKind::Goal,
                id: goal.id.clone(),
                label: goal.title.clone(),
            }],
            target: AttentionTarget::Console {
                session_id: alarm.id.clone(),
            },
        });
    }
    Ok(items)
}

/// Every pull request whose reviewer session
/// `scheduler::pull_requests::start_pull_request_session` has given up on
/// (`PullRequestRow::reviewer_given_up_at`): the same "automatic recovery
/// has given up" evidence as the orchestrator's own, read from the
/// scheduler's actual decision rather than the generic `disconnected`
/// flag a mere crash also raises, cleared the moment the request itself
/// no longer wants a reviewer session (`scheduler::pull_requests::end_review`)
/// so a request back in draft, or whose repository lost its review pin,
/// does not carry a stale blocker for work nothing is trying any more.
async fn reviewer_given_up_items(store: &Store) -> Result<Vec<AttentionItemDto>> {
    let mut items = Vec::new();
    for pull in store
        .list_pull_requests(ariadne_store::PullRequestFilter::default())
        .await?
    {
        let Some(since) = pull.reviewer_given_up_at.clone() else {
            continue;
        };
        let reviewer = store
            .list_sessions(SessionFilter {
                pull_request_id: Some(pull.id.clone()),
                ..Default::default()
            })
            .await?
            .into_iter()
            .rfind(|s| s.seat() == Some(Seat::Reviewer));
        let error = match &reviewer {
            Some(session) => last_session_error(store, &session.id).await?,
            None => None,
        };
        let summary = match &error {
            Some(error) => format!(
                "The request's reviewer session would not start after {SPAWN_RETRY_BUDGET} attempts: {error}"
            ),
            None => format!(
                "The request's reviewer session would not start after {SPAWN_RETRY_BUDGET} attempts."
            ),
        };
        items.push(AttentionItemDto {
            id: format!("recovery:unknown:pull_request:{}", pull.id),
            producer: AttentionProducer::Recovery,
            reason: AttentionCause::Unknown,
            summary,
            required_action: "Read why it will not start, then resume it yourself.".into(),
            since,
            affected: vec![AttentionSubjectDto {
                kind: AttentionSubjectKind::Repository,
                id: pull.repository_id.clone(),
                label: format!("pull request #{}", pull.number),
            }],
            target: AttentionTarget::PullRequest {
                pull_request_id: pull.id,
            },
        });
    }
    Ok(items)
}

/// A session flagged `exhausted` groups with every other one exhausted on
/// the same model *for the same reason* — the key is `(model, reason)`,
/// not the model alone, so two sessions blocked for different reasons
/// (one out of budget, one out of candidates) never collapse into one
/// item that could only report one of them. Every affected entry still
/// carries its own session id, the task it was running (or its seat, for
/// one that ran none) and its own switch count, so each is identifiable
/// and answerable on its own even inside a shared item.
async fn quota_items(store: &Store, launcher: &Launcher) -> Result<Vec<AttentionItemDto>> {
    let sessions = store
        .list_sessions(SessionFilter {
            attention_only: true,
            ..Default::default()
        })
        .await?;
    let mut groups: BTreeMap<(String, &'static str), QuotaGroup> = BTreeMap::new();
    for session in sessions {
        if session.attention_reason() != Some(AttentionReason::Exhausted)
            || session.status() != SessionStatus::Exited
            || !work_is_active_checked(store, &session).await?
        {
            continue;
        }
        let Some(block) = recovery_block(store, launcher, &session).await? else {
            continue;
        };
        let since = session
            .attention_since
            .clone()
            .unwrap_or_else(|| session.created_at.clone());
        let label = match (&session.task_id, session.seat()) {
            (Some(task_id), _) => format!("{task_id} ({} switch(es))", block.switches),
            (None, Some(Seat::Reviewer)) => format!("reviewer ({} switch(es))", block.switches),
            _ => format!("orchestrator ({} switch(es))", block.switches),
        };
        let group = groups
            .entry((session.model.clone(), block.reason))
            .or_insert_with(|| QuotaGroup {
                since: since.clone(),
                affected: Vec::new(),
            });
        if since < group.since {
            group.since = since;
        }
        group.affected.push(AttentionSubjectDto {
            kind: AttentionSubjectKind::Session,
            id: session.id.clone(),
            label,
        });
    }
    Ok(groups
        .into_iter()
        .map(|((model, reason), group)| {
            let session_id = group
                .affected
                .first()
                .map(|s| s.id.clone())
                .unwrap_or_default();
            AttentionItemDto {
                id: format!("recovery:quota:{model}:{}", reason.replace(' ', "-")),
                producer: AttentionProducer::Recovery,
                reason: AttentionCause::Quota,
                summary: format!("{model} hit a usage limit; automatic switching {reason}."),
                required_action: required_action_for(reason),
                since: group.since,
                affected: group.affected,
                target: AttentionTarget::Console { session_id },
            }
        })
        .collect())
}

struct QuotaGroup {
    since: String,
    affected: Vec<AttentionSubjectDto>,
}

fn required_action_for(reason: &str) -> String {
    match reason {
        "automatic model switching is disabled" => {
            "Enable automatic switching in config.toml, or switch the session to another model \
             yourself, then resume it."
                .into()
        }
        "spent its automatic switch budget" => {
            "Raise the model's quota, or switch the session to another model yourself, then \
             resume it."
                .into()
        }
        _ => "Enable another model in the catalog, or switch the session to another model \
              yourself, then resume it."
            .into(),
    }
}

/// Every task the daemon will never retry on its own, and whose
/// orchestrator has already had its say: `failed` is terminal — nothing in
/// the scheduler starts a failed task's agent again — but the orchestrator
/// is told of the failure (009 rule 4) and may retry it itself
/// (`http::tasks::retry`), which moves the task off `failed` entirely and
/// so off this list, before a human ever needs to. While the orchestrator
/// might still be answering that telling — mid-turn, or not yet idle since
/// the failure — the task is left out, the same "automatic recovery still
/// trying" rule every other cause here follows
/// (`orchestrator_has_answered_for`). One failed on the daemon's own
/// descriptor-limit words groups with every other one on it: one machine,
/// one shortage. Every other one is its own `unknown` item, naming that
/// task alone, rather than guessed into a group with no reliable evidence
/// it shares a cause with another (009, "unknown causes remain
/// separate").
async fn failed_task_items(store: &Store) -> Result<Vec<AttentionItemDto>> {
    let tasks = store
        .list_tasks(TaskFilter {
            status: Some(TaskStatus::Failed),
            ..Default::default()
        })
        .await?;
    let mut descriptor_since: Option<String> = None;
    let mut descriptor_affected = Vec::new();
    let mut unknown = Vec::new();
    for task in tasks {
        if !orchestrator_has_answered_for(store, &task.goal_id, &task.id, &task.updated_at).await? {
            continue;
        }
        let reason = store.ended_reason(&task).await?;
        let affected = AttentionSubjectDto {
            kind: AttentionSubjectKind::Task,
            id: task.id.clone(),
            label: task.title.clone(),
        };
        if reason.as_deref() == Some(DESCRIPTOR_LIMIT_REASON) {
            let at = task.updated_at.clone();
            descriptor_since = Some(match descriptor_since {
                Some(previous) if previous <= at => previous,
                _ => at,
            });
            descriptor_affected.push(affected);
        } else {
            unknown.push(AttentionItemDto {
                id: format!("recovery:unknown:task:{}", task.id),
                producer: AttentionProducer::Recovery,
                reason: AttentionCause::Unknown,
                summary: reason.unwrap_or_else(|| "the task failed".to_string()),
                required_action: "Read why it failed, then retry the task or take over its work."
                    .into(),
                since: task.updated_at.clone(),
                affected: vec![affected],
                target: AttentionTarget::Task { task_id: task.id },
            });
        }
    }
    let mut items = unknown;
    if let Some(since) = descriptor_since {
        let task_id = descriptor_affected
            .first()
            .map(|s| s.id.clone())
            .unwrap_or_default();
        items.push(AttentionItemDto {
            id: "recovery:resource:descriptor-limit".into(),
            producer: AttentionProducer::Recovery,
            reason: AttentionCause::Resource,
            summary: DESCRIPTOR_LIMIT_REASON.into(),
            required_action: "Free file descriptors on the daemon's machine, then retry the task."
                .into(),
            since,
            affected: descriptor_affected,
            target: AttentionTarget::Task { task_id },
        });
    }
    Ok(items)
}

/// Whether a failed task's goal has nothing left to automatically answer
/// it: either nothing automatic is ever coming (the goal is not
/// `orchestrated`, or not `planning` or `active` any more), its
/// orchestrator has already been given up on
/// (`Goal::orchestrator_given_up_at`, stamped by
/// `scheduler::goals::orchestrator_could_not_start` once the spawn budget
/// actually runs out — distinct from the `disconnected` flag a mere crash
/// also raises, which the liveness sweep may still resolve on its own, and
/// kept whether or not an orchestrator session exists this instant: an
/// orchestrated goal with none yet is one `keep_orchestrator` has not
/// finished its first launch of, not one with nothing coming), or this
/// exact task — its id *and* the `updated_at` its failure carries right
/// now — is in `Goal::orchestrator_answered_failed_task_ids`.
///
/// That list is written only once, by the ACP driver itself, the moment
/// the one `session/prompt` turn that actually carried this task's
/// failure returns (`acp::serve_with_input`, confirming exactly the
/// `Delivery::GoalAttention` it was handed) — never by an ambient session
/// status, which an unrelated turn landing on the same session first
/// could satisfy without ever having carried this task's news. Matching
/// on `updated_at` as well as the id is what keeps an old confirmation
/// from answering for a new failure of the same task: a retry stamps a
/// new `updated_at`, which a stale confirmation already written does not
/// carry.
async fn orchestrator_has_answered_for(
    store: &Store,
    goal_id: &str,
    task_id: &str,
    failed_at: &str,
) -> Result<bool> {
    let goal = match store.get_goal(goal_id).await {
        Ok(goal) => goal,
        // A goal gone from the store is a terminal one cleaned up after
        // the fact: nothing is left to automatically answer for it.
        Err(ariadne_store::StoreError::NotFound { .. }) => return Ok(true),
        Err(error) => return Err(error),
    };
    if !goal.orchestrated || !matches!(goal.status(), GoalStatus::Planning | GoalStatus::Active) {
        return Ok(true);
    }
    if goal.orchestrator_given_up_at.is_some() {
        return Ok(true);
    }
    let answered: Vec<(String, String)> = goal
        .orchestrator_answered_failed_task_ids
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok())
        .unwrap_or_default();
    Ok(answered
        .iter()
        .any(|(id, at)| id == task_id && at == failed_at))
}

/// An enabled forge integration whose last fetch failed on the daemon's
/// own "not installed" message groups with every other one naming the
/// same forge CLI: the fix is the same install wherever it is missing
/// from. Any other fetch error is left out: `forge/poll.rs` retries it
/// forever with no budget to spend and no evidence here to tell a
/// transient one from a confirmed blocker, and raising every retryable
/// forge hiccup as a human blocker is exactly the speculative
/// classification this producer does not make. A disabled integration's
/// fetch error is nobody's business: turning the integration off is
/// itself the fix.
async fn configuration_items(store: &Store) -> Result<Vec<AttentionItemDto>> {
    let repositories = store.list_repositories().await?;
    let mut groups: BTreeMap<String, (String, Vec<AttentionSubjectDto>, String, String)> =
        BTreeMap::new();
    for repository in repositories {
        let Some(forge) = repository.forge.as_ref().filter(|forge| forge.enabled) else {
            continue;
        };
        let Some(error) = forge
            .fetch_error
            .as_ref()
            .filter(|error| error.contains(FORGE_BINARY_MISSING))
        else {
            continue;
        };
        // `name` of the fixed message `` `{name}` is not installed... ``
        // — the CLI program the daemon could not find, and the grouping
        // key: every repository missing the same CLI shares the same fix.
        let program = error
            .split('`')
            .nth(1)
            .unwrap_or(error.as_str())
            .to_string();
        let section = format!("repositories/{}/forge", repository.id);
        let group = groups
            .entry(program)
            .or_insert_with(|| (forge.updated_at.clone(), Vec::new(), error.clone(), section));
        if forge.updated_at < group.0 {
            group.0 = forge.updated_at.clone();
        }
        group.1.push(AttentionSubjectDto {
            kind: AttentionSubjectKind::Repository,
            id: repository.id.clone(),
            label: format!("{}/{}", forge.owner, forge.name),
        });
    }
    Ok(groups
        .into_iter()
        .map(
            |(program, (since, affected, error, section))| AttentionItemDto {
                id: format!("recovery:configuration:{program}"),
                producer: AttentionProducer::Recovery,
                reason: AttentionCause::Configuration,
                summary: error,
                required_action: "Install the forge CLI the error names, or point config.toml at \
                               it, then it is used on the next poll."
                    .into(),
                since,
                affected,
                target: AttentionTarget::Settings { section },
            },
        )
        .collect())
}

/// An enabled forge integration whose last fetch failed with the daemon's
/// own reactive confirmation that the forge CLI is signed out groups with
/// every other one of the same host: the fix — signing back in — is the
/// same wherever it is. The confirmation
/// (`crate::forge::poll::FORGE_SIGNED_OUT`) is read off the fetch error
/// rather than guessed from the error alone, since `forge/poll.rs` runs
/// the CLI's own sign-in check only once a fetch has already failed —
/// checking an existing status, reactively, is not the new monitoring
/// service this producer does not add. Any other fetch error stays
/// `configuration`'s or nobody's, the same way as before.
async fn access_items(store: &Store) -> Result<Vec<AttentionItemDto>> {
    let repositories = store.list_repositories().await?;
    let mut groups: BTreeMap<String, (String, Vec<AttentionSubjectDto>, String)> = BTreeMap::new();
    for repository in repositories {
        let Some(forge) = repository.forge.as_ref().filter(|forge| forge.enabled) else {
            continue;
        };
        let Some(_error) = forge
            .fetch_error
            .as_ref()
            .filter(|error| error.contains(crate::forge::poll::FORGE_SIGNED_OUT))
        else {
            continue;
        };
        let group = groups.entry(forge.host.clone()).or_insert_with(|| {
            (
                forge.updated_at.clone(),
                Vec::new(),
                format!("repositories/{}/forge", repository.id),
            )
        });
        if forge.updated_at < group.0 {
            group.0 = forge.updated_at.clone();
        }
        group.1.push(AttentionSubjectDto {
            kind: AttentionSubjectKind::Repository,
            id: repository.id.clone(),
            label: format!("{}/{}", forge.owner, forge.name),
        });
    }
    Ok(groups
        .into_iter()
        .map(|(host, (since, affected, section))| AttentionItemDto {
            id: format!("recovery:access:{host}"),
            producer: AttentionProducer::Recovery,
            reason: AttentionCause::Access,
            summary: format!("The forge CLI is not signed in to {host}."),
            required_action: format!(
                "Sign the forge CLI back in to {host}, then it is used on the next poll."
            ),
            since,
            affected,
            target: AttentionTarget::Settings { section },
        })
        .collect())
}
