//! The recovery producer: the first "Needs attention" path.
//!
//! What it reads is evidence the daemon already keeps for its own retry
//! loops — a session's switch chain, a task's own failure reason, a
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
use crate::scheduler::auto_switch::recovery_exhausted;

use super::work_is_active;

/// The fixed words [`crate::forge::Cli::binary`] gives a forge CLI the
/// daemon's PATH does not have. Matched exactly, since this is the
/// daemon's own message rather than a guess at the forge CLI's.
const FORGE_BINARY_MISSING: &str = "is not installed on the daemon's PATH";

/// Every recovery item this producer currently finds: a model exhausted
/// with no automatic switch left, a task failed on a machine resource
/// shortage, and whatever an enabled forge integration's own last fetch
/// error says — the daemon's "CLI not installed" words where they match,
/// and the error verbatim, kept apart and ungrouped, where they do not.
pub(crate) async fn items(store: &Store, launcher: &Launcher) -> Result<Vec<AttentionItemDto>> {
    let mut items = quota_items(store, launcher).await?;
    items.extend(resource_items(store).await?);
    items.extend(forge_items(store).await?);
    Ok(items)
}

/// A session flagged `exhausted` groups with every other one exhausted on
/// the same model: the same cause, the same action — raise that model's
/// quota, or switch away from it yourself.
async fn quota_items(store: &Store, launcher: &Launcher) -> Result<Vec<AttentionItemDto>> {
    let sessions = store
        .list_sessions(SessionFilter {
            attention_only: true,
            ..Default::default()
        })
        .await?;
    let mut groups: BTreeMap<String, (String, Vec<AttentionSubjectDto>)> = BTreeMap::new();
    for session in sessions {
        if session.attention_reason() != Some(AttentionReason::Exhausted)
            || session.status() != SessionStatus::Exited
            || !work_is_active(store, &session).await
        {
            continue;
        }
        if !recovery_exhausted(store, launcher, &session).await? {
            continue;
        }
        let since = session
            .attention_since
            .clone()
            .unwrap_or_else(|| session.created_at.clone());
        let group = groups
            .entry(session.model.clone())
            .or_insert_with(|| (since.clone(), Vec::new()));
        if since < group.0 {
            group.0 = since;
        }
        group.1.push(AttentionSubjectDto {
            kind: AttentionSubjectKind::Session,
            id: session.id.clone(),
            label: session.model.clone(),
        });
    }
    Ok(groups
        .into_iter()
        .map(|(model, (since, affected))| {
            let session_id = affected.first().map(|s| s.id.clone()).unwrap_or_default();
            AttentionItemDto {
                id: format!("recovery:quota:{model}"),
                producer: AttentionProducer::Recovery,
                cause: AttentionCause::Quota,
                summary: format!(
                    "{model} hit a usage limit, and automatic model switching has no model \
                     left to try."
                ),
                required_action: "Raise the model's quota, or switch the session to another \
                                   model yourself, then resume it."
                    .into(),
                since,
                affected,
                target: AttentionTarget::Console { session_id },
            }
        })
        .collect())
}

/// A task failed on the exact words the daemon's own descriptor-limit
/// detection gives groups with every other one: one machine, one shortage.
async fn resource_items(store: &Store) -> Result<Vec<AttentionItemDto>> {
    let tasks = store
        .list_tasks(TaskFilter {
            status: Some(TaskStatus::Failed),
            ..Default::default()
        })
        .await?;
    let mut since: Option<String> = None;
    let mut affected = Vec::new();
    for task in tasks {
        if store.ended_reason(&task).await?.as_deref() != Some(DESCRIPTOR_LIMIT_REASON) {
            continue;
        }
        let at = task.updated_at.clone();
        since = Some(match since {
            Some(previous) if previous <= at => previous,
            _ => at,
        });
        affected.push(AttentionSubjectDto {
            kind: AttentionSubjectKind::Task,
            id: task.id.clone(),
            label: task.title.clone(),
        });
    }
    let Some(since) = since else {
        return Ok(Vec::new());
    };
    let task_id = affected.first().map(|s| s.id.clone()).unwrap_or_default();
    Ok(vec![AttentionItemDto {
        id: "recovery:resource:descriptor-limit".into(),
        producer: AttentionProducer::Recovery,
        cause: AttentionCause::Resource,
        summary: DESCRIPTOR_LIMIT_REASON.into(),
        required_action: "Free file descriptors on the daemon's machine, then retry the task."
            .into(),
        since,
        affected,
        target: AttentionTarget::Task { task_id },
    }])
}

/// Every enabled forge integration whose last fetch failed, read for the
/// daemon's own evidence: an integration missing its CLI groups with every
/// other one naming the same program, since the fix is the same install
/// wherever it is missing from — a `configuration` item. Anything else the
/// forge CLI answered is not guessed at; it is kept, as its own `unknown`
/// item naming that one repository, rather than dropped for want of a
/// pattern to name it by (009, "unknown causes remain separate"). A
/// disabled integration's fetch error is nobody's business: turning the
/// integration off is itself the fix.
async fn forge_items(store: &Store) -> Result<Vec<AttentionItemDto>> {
    let repositories = store.list_repositories().await?;
    let mut configuration: BTreeMap<String, (String, Vec<AttentionSubjectDto>, String, String)> =
        BTreeMap::new();
    let mut unknown = Vec::new();
    for repository in repositories {
        let Some(forge) = repository.forge.as_ref().filter(|forge| forge.enabled) else {
            continue;
        };
        let Some(error) = forge.fetch_error.as_ref() else {
            continue;
        };
        let section = format!("repositories/{}/forge", repository.id);
        let affected = AttentionSubjectDto {
            kind: AttentionSubjectKind::Repository,
            id: repository.id.clone(),
            label: format!("{}/{}", forge.owner, forge.name),
        };
        if error.contains(FORGE_BINARY_MISSING) {
            // `name` of the fixed message `` `{name}` is not installed... ``
            // — the CLI program the daemon could not find, and the
            // grouping key: every repository missing the same CLI shares
            // the same fix.
            let program = error
                .split('`')
                .nth(1)
                .unwrap_or(error.as_str())
                .to_string();
            let group = configuration
                .entry(program)
                .or_insert_with(|| (forge.updated_at.clone(), Vec::new(), error.clone(), section));
            if forge.updated_at < group.0 {
                group.0 = forge.updated_at.clone();
            }
            group.1.push(affected);
        } else {
            unknown.push(AttentionItemDto {
                id: format!("recovery:unknown:forge:{}", repository.id),
                producer: AttentionProducer::Recovery,
                cause: AttentionCause::Unknown,
                summary: error.clone(),
                required_action: "Read the forge CLI's own error, fix what it names, then it is \
                                   used on the next poll."
                    .into(),
                since: forge.updated_at.clone(),
                affected: vec![affected],
                target: AttentionTarget::Settings { section },
            });
        }
    }
    let mut items: Vec<AttentionItemDto> = configuration
        .into_iter()
        .map(
            |(program, (since, affected, error, section))| AttentionItemDto {
                id: format!("recovery:configuration:{program}"),
                producer: AttentionProducer::Recovery,
                cause: AttentionCause::Configuration,
                summary: error,
                required_action: "Install the forge CLI the error names, or point config.toml at \
                               it, then it is used on the next poll."
                    .into(),
                since,
                affected,
                target: AttentionTarget::Settings { section },
            },
        )
        .collect();
    items.extend(unknown);
    Ok(items)
}
