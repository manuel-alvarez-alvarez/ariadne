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
        if !orchestrator_has_answered_for(store, &task.goal_id, &task.updated_at).await? {
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
/// it: either nothing automatic is ever coming (the goal carries no
/// orchestrator at all, is not even `planning` or `active` any more, or
/// has never once had an orchestrator session to its name), its last
/// attempt at one has already been given up on
/// (`AttentionReason::Disconnected`, the alarm
/// `scheduler::goals::orchestrator_could_not_start` raises once the spawn
/// budget runs out), or its orchestrator has actually been handed a
/// prompt naming this failure — `Goal::orchestrator_told_at`, stamped by
/// `tell_orchestrator` at the exact moment the prompt goes out, at or
/// after the failure — and is not mid-turn on it right now.
///
/// The stamp, not a session's own `last_activity_at`, is what is read for
/// "told": an orchestrator can go idle and run again for a reason that has
/// nothing to do with this task — ending a turn already running when the
/// task failed, say — and `last_activity_at` would move right along with
/// it whether or not the orchestrator was ever told, where the stamp only
/// ever moves at the one call that hands it a prompt naming the goal's
/// current failures. Told is not yet answered, though: the orchestrator
/// may retry the task on the very turn the prompt starts, so "answered" is
/// held back until that turn is not still running — the same "automatic
/// recovery still trying" grace mid-turn gets everywhere else here. And a
/// goal with no *live* orchestrator this instant is not necessarily one
/// with nothing coming: `keep_orchestrator` relaunches one across a
/// restart or a crash on its own, within its own budget
/// (`SPAWN_RETRY_BUDGET`), so every orchestrator session the goal has ever
/// had is read here, not only the live ones, to tell a relaunch still in
/// flight from one already given up on.
async fn orchestrator_has_answered_for(
    store: &Store,
    goal_id: &str,
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
    let orchestrators: Vec<_> = store
        .list_sessions(SessionFilter {
            goal_id: Some(goal_id.to_string()),
            ..Default::default()
        })
        .await?
        .into_iter()
        .filter(|s| s.seat() == Some(Seat::Orchestrator))
        .collect();
    // Never had one at all: nothing automatic has ever touched this goal,
    // which is as good as nothing coming.
    if orchestrators.is_empty() {
        return Ok(true);
    }
    if orchestrators
        .iter()
        .any(|s| s.attention_reason() == Some(AttentionReason::Disconnected))
    {
        return Ok(true);
    }
    let told = goal
        .orchestrator_told_at
        .as_deref()
        .is_some_and(|at| at >= failed_at);
    if !told {
        return Ok(false);
    }
    let mid_turn = orchestrators
        .iter()
        .any(|s| s.status() == SessionStatus::Running);
    Ok(!mid_turn)
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
