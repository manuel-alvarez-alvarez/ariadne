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
                let enabled = integration.is_some_and(|i| i.enabled);
                items.extend(readiness_item(store, launcher, &pull, enabled, &repo).await?);
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
/// backing that claim up:
/// - a current approval (`review_decision == "approved"`);
/// - no conversation-side comment the login's own side has not answered,
///   read fresh here rather than off `unanswered_comments`: that field
///   counts a review's own summary too, posted under the login as a plain
///   `issue_comment` on the conversation thread (`forge/pulls.rs::CONVERSATION`)
///   and never resolved, since it names no diff-anchored thread at all —
///   `is_mine` reads it as the other side's (029 rule 13, so a task author
///   is told of it as news) rather than as nothing to answer, and a
///   summary carries nothing to answer. The babysitting skill never
///   replies to a comment that asks for no change, so a summary with no
///   finding must never need an invented reply just to clear this item;
/// - no *resolvable* review thread the forge still shows unresolved,
///   whoever opened it: a reply alone answers a thread for routing
///   purposes (`forge::live::waiting_threads`'s own "answered" reading)
///   without resolving it, and a resolve is the only mark this item reads
///   as the thread actually being done with (031, "Require explicit
///   resolution of every review thread"). Read by `kind == "review_comment"`
///   rather than by who posted it (`from_review`): a human's own unresolved
///   finding must block exactly as an Ariadne one does, and a review's own
///   summary — posted as a plain `issue_comment`, never a resolvable thread
///   at all — must never be read as one that blocks forever;
/// - every check green (`checks == "success"`);
/// - the forge's own confirmation the head can be merged now
///   (`mergeable == "clean"`);
/// - the last attempt to refresh the comment evidence itself actually
///   succeeded (`Live::evidence_ok`) *and* left it current for this head
///   (`Details::head_sha == pull.head_sha`) — a failed detail fetch
///   leaves an older read standing (`forge::live::LivePulls::set_pull_evidence_failed`),
///   which must never be read as "no open comment" whether or not the
///   head happened to move too;
/// - the babysitting task's own claim itself current for this head
///   (`PullRequestRow::ready_head_sha == pull.head_sha`) — a push the
///   claim predates must drop it, whatever the forge's own approval or
///   checks still say about the new head;
/// - the repository's forge integration still enabled — a disabled one's
///   cached evidence is nobody's business (recovery's own rule 4
///   `configuration` note applies the same way here).
///
/// Any one short of this — stale, missing or simply not there yet —
/// withholds the item rather than claiming readiness on a guess. Raised
/// only for a request a task keeps (`origin_task_id`): that is the
/// babysitting task's own request, and only its claim counts (029, "Only
/// the babysitter raises readiness attention for a request it manages") —
/// `crates/ariadne-daemon/src/http/pull_requests.rs::report` refuses a
/// `ready` write from any other session a task-kept row also answers to.
async fn readiness_item(
    store: &Store,
    launcher: &Launcher,
    pull: &PullRequest,
    integration_enabled: bool,
    repo: &str,
) -> Result<Option<AttentionItemDto>> {
    if !integration_enabled
        || !pull.ready
        || pull.review_decision != "approved"
        || pull.checks != "success"
        || pull.mergeable != "clean"
    {
        return Ok(None);
    }
    let cached = launcher.live.get(&pull.id);
    if !cached.as_ref().is_some_and(|cached| cached.evidence_ok) {
        return Ok(None);
    }
    let details_head_sha = cached.and_then(|cached| cached.details).map(|d| d.head_sha);
    if details_head_sha.as_deref() != Some(pull.head_sha.as_str()) {
        return Ok(None);
    }
    if pull.ready_head_sha.as_deref() != Some(pull.head_sha.as_str()) {
        return Ok(None);
    }
    // Every resolvable review thread — one a diff-anchored review comment
    // opened, whoever posted it — must actually be resolved on the forge,
    // not merely answered: a reply settles a thread for conversation
    // routing without resolving it (`forge/live.rs::waiting_threads`), and
    // a review's own summary — posted as a plain `issue_comment`, never a
    // resolvable thread at all — must never be read as one that blocks
    // forever.
    let comments = live::comments_of(store, &launcher.live, pull).await?;
    if comments
        .iter()
        .any(|c| c.kind == "review_comment" && !c.resolved)
    {
        return Ok(None);
    }
    // The conversation side, read fresh rather than off `unanswered_comments`:
    // that field counts a review's own summary as the other side's input
    // (`forge/live.rs::is_mine`, 029 rule 13), since a task author is told
    // of it as news the same way a genuine finding is. A summary carries
    // nothing to answer, so this item excludes it — the one comment the
    // login posts, from a review, to the conversation thread — rather than
    // waiting on a reply the babysitting skill is never meant to write.
    let login = live::login_of(store, &pull.repository_id).await?;
    let actionable: Vec<_> = comments
        .iter()
        .filter(|c| {
            !(c.thread_id == crate::forge::pulls::CONVERSATION
                && c.from_review
                && c.author_login.eq_ignore_ascii_case(&login))
        })
        .cloned()
        .collect();
    if !live::waiting_threads(&actionable, &pull.role, &login).is_empty() {
        return Ok(None);
    }
    let since = pull
        .ready_confirmed_at
        .clone()
        .unwrap_or_else(|| pull.updated_at.clone());
    Ok(Some(AttentionItemDto {
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
    }))
}
