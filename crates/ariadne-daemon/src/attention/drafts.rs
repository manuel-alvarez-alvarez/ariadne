//! The producer for a repository's own held-back forge posts (031): the
//! viewer's unsubmitted pending review on GitHub, or their draft notes on
//! GitLab, read off the last fetch of a tracked request whose repository
//! holds agent replies and reviews back for approval
//! (`Repository::supervise_posts`). Ariadne has no surface of its own to
//! finish or submit either, so the item's one action, and its target, both
//! send a person straight to the request's own page on the forge.

use ariadne_api::attention::{
    AttentionCause, AttentionItemDto, AttentionProducer, AttentionSubjectDto, AttentionSubjectKind,
    AttentionTarget,
};
use ariadne_store::{PullRequestFilter, Result, Store, StoreError};

use crate::forge::live;
use crate::launcher::Launcher;

/// One item for every tracked request of a repository with
/// `supervise_posts` on, where the last fetch (`forge::poll`) still shows
/// the viewer's own drafts sitting unsubmitted on the forge. A repository
/// the store cannot find any more (`StoreError::NotFound`) is a row this
/// read lost the race with a deletion on, not a read that failed, and is
/// skipped the same way a gone pull request is elsewhere in this module.
pub(crate) async fn items(store: &Store, launcher: &Launcher) -> Result<Vec<AttentionItemDto>> {
    let mut items = Vec::new();
    for row in store
        .list_pull_requests(PullRequestFilter::default())
        .await?
    {
        let repository = match store.get_repository(&row.repository_id).await {
            Ok(repository) => repository,
            Err(StoreError::NotFound { .. }) => continue,
            Err(error) => return Err(error),
        };
        if !repository.supervise_posts() {
            continue;
        }
        let Some(pull) = live::of_row(store, &launcher.live, row).await? else {
            continue;
        };
        if pull.state != "open" {
            continue;
        }
        // `None` is no draft at all; `Some(0)` is still one — a pending
        // GitHub review can carry a verdict and no inline comment of its
        // own, and is still a draft waiting to be submitted.
        let Some(count) = launcher
            .live
            .get(&pull.id)
            .and_then(|live| live.details)
            .and_then(|details| details.pending_drafts)
        else {
            continue;
        };
        let repo = repository
            .forge
            .as_ref()
            .map(|integration| format!("{}/{}", integration.owner, integration.name))
            .unwrap_or_else(|| pull.repository_id.clone());
        let label = format!("{repo}#{} {}", pull.number, pull.title);
        let summary = if count == 0 {
            format!("{label} has an unsubmitted draft waiting on the forge.")
        } else {
            let noun = if count == 1 { "comment" } else { "comments" };
            format!("{label} holds {count} unsubmitted draft {noun} waiting on the forge.")
        };
        items.push(AttentionItemDto {
            id: format!("pull_request:drafts:{}", pull.id),
            producer: AttentionProducer::Drafts,
            reason: AttentionCause::Unknown,
            summary,
            required_action: "Finish and submit it yourself, directly on the forge.".into(),
            // No stored mark of its own says when this count first moved
            // off zero — this producer reads the forge's live count alone,
            // never a column — so the forge's own last-moved time is the
            // best approximation to hand: it undercounts how long a draft
            // has actually waited wherever the request moved for an
            // unrelated reason since, the same trap rule 4 of 031 already
            // names for a task's `updated_at`.
            since: pull.updated_at.clone(),
            affected: vec![AttentionSubjectDto {
                kind: AttentionSubjectKind::PullRequest,
                id: pull.id.clone(),
                label,
            }],
            target: AttentionTarget::Forge {
                url: pull.url.clone(),
            },
        });
    }
    Ok(items)
}
