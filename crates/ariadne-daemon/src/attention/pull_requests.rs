//! The pull-request producer: a request's next step.
//!
//! Two items, both raised from the forge's own evidence of the last fetch
//! (026) rather than a guess: a review request nobody is assigned to, and
//! a request a babysitting task has claimed is ready to merge, confirmed
//! against the forge's own approval, comment, check and mergeability
//! evidence for the current head. A request automatic recovery has given
//! up starting a reviewer for (`PullRequestRow::reviewer_given_up_at`) is
//! `recovery`'s own item (009, 031); this producer never raises a second
//! one for it, since both would carry the same underlying request.

use ariadne_api::attention::{
    AttentionCause, AttentionItemDto, AttentionProducer, AttentionSubjectDto, AttentionSubjectKind,
    AttentionTarget,
};
use ariadne_store::{PullRequest, PullRequestFilter, Result, Store};

use crate::forge::live;
use crate::launcher::Launcher;

/// Every item this producer currently finds, each read off the last fetch
/// of a request Ariadne already keeps a row for: `live::of_row` answers
/// `None` where no fetch has read the row yet, which is missing evidence
/// rather than a blocker either item raises on.
pub(crate) async fn items(store: &Store, launcher: &Launcher) -> Result<Vec<AttentionItemDto>> {
    let mut items = Vec::new();
    for row in store
        .list_pull_requests(PullRequestFilter::default())
        .await?
    {
        let Some(pull) = live::of_row(store, &launcher.live, row).await? else {
            continue;
        };
        if pull.state != "open" || pull.draft {
            continue;
        }
        let integration = store.forge_integration(&pull.repository_id).await?;
        let repo = integration
            .as_ref()
            .map(|i| format!("{}/{}", i.owner, i.name))
            .unwrap_or_else(|| pull.repository_id.clone());
        match pull.role.as_str() {
            "reviewer" => {
                if let Some(item) = review_start_item(&pull, integration.as_ref(), &repo) {
                    items.push(item);
                }
            }
            "author" if pull.origin_task_id.is_some() => {
                items.extend(readiness_item(&pull, &repo));
            }
            _ => {}
        }
    }
    Ok(items)
}

/// What to call the request where there is room for one word more than its
/// id (`AttentionSubjectDto::label`): its repository, so a client that
/// shows only the label still names where the request is.
fn label(pull: &PullRequest, repo: &str) -> String {
    format!("{repo}#{} {}", pull.number, pull.title)
}

fn subject(pull: &PullRequest, repo: &str) -> AttentionSubjectDto {
    AttentionSubjectDto {
        kind: AttentionSubjectKind::PullRequest,
        id: pull.id.clone(),
        label: label(pull, repo),
    }
}

/// A request that asks for my review, out of draft and open, nobody will
/// ever start a session for: neither the repository's own review pin nor a
/// pin asked directly on the row (029's `wants_session`) names a model. A
/// request automatic recovery gave up starting is left to `recovery`'s own
/// item, which already names the same request and the same fix — raising a
/// second here would be two items for one blocker.
fn review_start_item(
    pull: &PullRequest,
    integration: Option<&ariadne_store::ForgeIntegration>,
    repo: &str,
) -> Option<AttentionItemDto> {
    if !pull.review_requested || pull.reviewer_given_up_at.is_some() {
        return None;
    }
    let pinned =
        integration.is_some_and(|i| i.review_model.is_some()) || pull.review_model.is_some();
    if pinned {
        return None;
    }
    Some(AttentionItemDto {
        id: format!("pull_request:review_start:{}", pull.id),
        producer: AttentionProducer::PullRequest,
        reason: AttentionCause::Configuration,
        summary: format!(
            "{} asks for your review, and no agent is assigned to it.",
            label(pull, repo)
        ),
        required_action: "Start a review yourself, on a model you pick.".into(),
        since: pull.created_at.clone(),
        affected: vec![subject(pull, repo)],
        target: AttentionTarget::PullRequest {
            pull_request_id: pull.id.clone(),
        },
    })
}

/// A request a babysitting task claims is ready to merge (`ready`), with
/// the forge's own evidence, for the request's current head, actually
/// backing that claim up: a current approval (`review_decision`), no open
/// review comment (`unanswered_comments`), every check green (`checks`)
/// and the forge's own confirmation the head can be merged now
/// (`mergeable`). Any one short of this — stale, missing or simply not
/// there yet — withholds the item rather than claiming readiness on a
/// guess; since every field is read off the live state of the current
/// head, a later commit, a reopened comment or a withdrawn approval drops
/// the item on its own, with no mark of its own to invalidate. Raised only
/// for a request a task keeps (`origin_task_id`): that is the babysitting
/// task's own request, and only its claim counts (029, "Only the babysitter
/// raises readiness attention for a request it manages").
fn readiness_item(pull: &PullRequest, repo: &str) -> Option<AttentionItemDto> {
    if !pull.ready
        || pull.review_decision != "approved"
        || pull.unanswered_comments != 0
        || pull.checks != "success"
        || pull.mergeable != "clean"
    {
        return None;
    }
    let since = pull
        .ready_confirmed_at
        .clone()
        .unwrap_or_else(|| pull.updated_at.clone());
    Some(AttentionItemDto {
        id: format!("pull_request:ready:{}", pull.id),
        producer: AttentionProducer::PullRequest,
        reason: AttentionCause::Unknown,
        summary: format!(
            "{} is approved, every check passes, and the forge reports it clear to merge.",
            label(pull, repo)
        ),
        required_action: "Merge it yourself: Ariadne gives no approval.".into(),
        since,
        affected: vec![subject(pull, repo)],
        target: AttentionTarget::PullRequest {
            pull_request_id: pull.id.clone(),
        },
    })
}
