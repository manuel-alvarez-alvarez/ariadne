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
use ariadne_core::{AttentionReason, SessionStatus, TaskStatus};
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
    Ok(items)
}

/// A session flagged `exhausted` groups with every other one exhausted on
/// the same model: the same cause, and much of the same action — raise
/// that model's quota, or switch away from it yourself. The group's
/// summary and action follow one representative session's own blocker
/// (`scheduler::auto_switch::recovery_block`); its `affected` entries each
/// still carry their own session id and the task it was running, so every
/// one of them is identifiable on its own.
async fn quota_items(store: &Store, launcher: &Launcher) -> Result<Vec<AttentionItemDto>> {
    let sessions = store
        .list_sessions(SessionFilter {
            attention_only: true,
            ..Default::default()
        })
        .await?;
    let mut groups: BTreeMap<String, QuotaGroup> = BTreeMap::new();
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
        let label = session
            .task_id
            .clone()
            .unwrap_or_else(|| "orchestrator".to_string());
        let group = groups
            .entry(session.model.clone())
            .or_insert_with(|| QuotaGroup {
                since: since.clone(),
                affected: Vec::new(),
                switches: block.switches,
                reason: block.reason,
            });
        if since < group.since {
            group.since = since;
        }
        group.switches = block.switches;
        group.reason = block.reason;
        group.affected.push(AttentionSubjectDto {
            kind: AttentionSubjectKind::Session,
            id: session.id.clone(),
            label,
        });
    }
    Ok(groups
        .into_iter()
        .map(|(model, group)| {
            let session_id = group
                .affected
                .first()
                .map(|s| s.id.clone())
                .unwrap_or_default();
            AttentionItemDto {
                id: format!("recovery:quota:{model}"),
                producer: AttentionProducer::Recovery,
                reason: AttentionCause::Quota,
                summary: format!(
                    "{model} hit a usage limit; automatic switching {} after {} switch(es).",
                    group.reason, group.switches
                ),
                required_action: required_action_for(group.reason),
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
    switches: u32,
    reason: &'static str,
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

/// Every task the daemon will never retry on its own: `failed` is terminal
/// — nothing in the scheduler starts a failed task's agent again, so
/// reaching it is itself proof that whatever automatic recovery and the
/// orchestrator could do, they already did. One failed on the daemon's
/// own descriptor-limit words groups with every other one on it: one
/// machine, one shortage. Every other one is its own `unknown` item,
/// naming that task alone, rather than guessed into a group with no
/// reliable evidence it shares a cause with another (009, "unknown causes
/// remain separate").
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
