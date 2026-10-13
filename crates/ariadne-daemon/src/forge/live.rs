//! What the forge says of the requests Ariadne works on, held in memory
//! alone (026).
//!
//! The database keeps Ariadne's own bookkeeping of a request
//! ([`PullRequestRow`]) and a mark per comment ([`CommentMark`]), never what
//! the forge holds: a stored title, check or comment would go stale the
//! moment somebody pushed. Each fetch leaves its open list here, keyed by
//! repository id, and the request reads of rows Ariadne works on, keyed by
//! row id; the scheduler and the routes join the latter to the row. A daemon
//! that restarts holds nothing until its first fetch, which runs at once.
//!
//! The rules that read the comments are here too, over the forge's comments
//! and the marks: which comments a thread's answer settles, which threads
//! wait on the integration login, and which comments a session has not been
//! told of.
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use ariadne_store::{
    CommentMark, ForgeIntegration, NewPullRequestComment, PullRequest, PullRequestComment,
    PullRequestLive, PullRequestRow,
};

use super::pulls::{CONVERSATION, FailedCheck, ForgePullRequest, signature, unsigned};

/// The age at which a matching list read must refresh its held details.
///
/// The fallback forge poll runs every five minutes, so this retains the
/// detail read for no more than three unchanged fetches.
const DETAILS_MAX_AGE: Duration = Duration::from_secs(15 * 60);

/// The last read of one request Ariadne works on.
#[derive(Debug, Clone)]
pub struct Live {
    pub pull: ForgePullRequest,
    /// Whether the request asks for the user's review (029).
    pub review_requested: bool,
    /// What a detail read found; None where the request was only listed.
    pub details: Option<Details>,
    /// When `details` last came from a successful detail read. A matching
    /// list read refreshes it at [`DETAILS_MAX_AGE`], because a base move,
    /// thread resolution, or different failed check names can leave every
    /// listed field unchanged.
    details_read_at: Option<Instant>,
    /// Whether the last attempt to refresh `details` actually succeeded.
    /// `set` (a successful detail fetch) sets this `true`; the forge
    /// poll's own failure path sets it `false` explicitly
    /// (`set_pull_evidence_failed`) while still leaving the last good
    /// `details` standing, so a reader can tell "the comments are exactly
    /// as stale as the head they were last read on" (`Details::head_sha`)
    /// from "the most recent attempt to read them failed outright" —
    /// the second a `pull_request` readiness item must withhold on even
    /// where the head has not moved at all, since a reopened thread or a
    /// withdrawn approval the failed read would have caught is invisible
    /// to a head comparison alone. A call that touches no comment
    /// evidence at all — an API write's own echo of the pull
    /// (`set_pull`) — leaves it exactly as it found it: that call has
    /// nothing to say about whether the evidence is still good.
    pub evidence_ok: bool,
    /// Bumped on every `add_comments`: a detail read that captured a
    /// lower number before it started its own, slower fetch could not
    /// have seen what a bump past it later added.
    generation: u64,
    /// Which locally-added comment, by forge id, a bump of `generation`
    /// added, and which bump: a fresh read's merge carries one forward
    /// only across a read whose own captured generation sat before that
    /// bump, since that read's silence on it then says nothing of the
    /// forge deleting it. One confirmed by a fresh read on its own terms
    /// is dropped here: the forge has it now, so this cache no longer
    /// has to carry it forward itself.
    local_additions: HashMap<String, u64>,
}

impl Live {
    /// A read with no local addition of its own this cache does not
    /// already know about: for a caller outside this module building one
    /// fresh, never itself merged into an existing entry.
    pub fn new(
        pull: ForgePullRequest,
        review_requested: bool,
        details: Option<Details>,
        evidence_ok: bool,
    ) -> Self {
        let details_read_at = details.as_ref().map(|_| Instant::now());
        Live {
            pull,
            review_requested,
            details,
            details_read_at,
            evidence_ok,
            generation: 0,
            local_additions: HashMap::new(),
        }
    }

    /// Whether `listed`'s read of the request leaves nothing for a detail
    /// read to catch that this held read does not already show, so
    /// `read_row` may keep this entry's own `details` rather than pay for
    /// a fresh detail read: `updated_at` is the forge's own "this moved"
    /// bit; `head_sha` is a new commit on the head; `checks`,
    /// `review_decision` and `mergeable` are each read straight off the
    /// forge's own evidence, independently of one another, so any one of
    /// them can move on its own; `draft`, `title` and `state` are read off
    /// the list the same way a detail read would read them, so the list
    /// already shows a change to them. None of that is enough on its own
    /// where the held `details` are missing, where the last attempt to
    /// read them failed (`evidence_ok` false), or where a comment Ariadne
    /// itself posted has not yet been read back by a detail read
    /// (`local_additions`), where `generation` changed after `since`, the
    /// generation `read_row` captured before it began this read, or where
    /// the held details reached [`DETAILS_MAX_AGE`]: each leaves `details`
    /// behind a state the list's own read never shows.
    pub fn unmoved(&self, listed: &ForgePullRequest, since: u64, now: Instant) -> bool {
        self.details.is_some()
            && self.details_read_at.is_some_and(|read_at| {
                now.checked_duration_since(read_at)
                    .is_some_and(|age| age < DETAILS_MAX_AGE)
            })
            && self.evidence_ok
            && self.local_additions.is_empty()
            && self.generation == since
            && self.pull.updated_at == listed.updated_at
            && self.pull.head_sha == listed.head_sha
            && self.pull.checks == listed.checks
            && self.pull.review_decision == listed.review_decision
            && self.pull.mergeable == listed.mergeable
            && self.pull.draft == listed.draft
            && self.pull.title == listed.title
            && self.pull.state == listed.state
    }
}

/// What a detail read finds of a request beyond its own read: its
/// comments, the checks that failed on its head, and whether the head is
/// behind its base.
#[derive(Debug, Clone, Default)]
pub struct Details {
    pub comments: Vec<NewPullRequestComment>,
    pub failed_checks: Vec<FailedCheck>,
    pub behind_base: bool,
    /// The head this detail read was taken at: a failed detail fetch
    /// leaves the last successful one standing (`set_pull`) rather than
    /// losing it, which can leave it behind a `pull.head_sha` a later,
    /// successful list read already moved on. Comparing the two is how a
    /// reader tells a comment list that is current for this head from one
    /// that is merely the last one read, which the `pull_request`
    /// attention producer must never read as "no open comment" for a head
    /// it was never actually read on.
    pub head_sha: String,
    /// The viewer's own unsubmitted posts on this request, as this detail
    /// read found them (031): a pending review's comment count on GitHub,
    /// or the draft notes count on GitLab. Read beside the rest of the
    /// detail fetch, so it is skipped and left standing the same way
    /// `comments` and `failed_checks` are wherever the fetch itself is
    /// (`Live::unmoved`), and dropped the same way they are on a failed
    /// refresh (`LivePulls::set_pull_evidence_failed`).
    pub pending_drafts: i64,
}

/// The last list a fetch read of one repository (026): every open request
/// and which of them ask for the user's review, as `forge::poll::fetch`
/// read them off the forge, and when. `GET /v1/pull-requests` and its
/// search answer from this alone: a repository with no fetch yet holds
/// none, which both read as empty rather than calling the forge themselves.
#[derive(Debug, Clone, Default)]
pub struct RepositoryList {
    identity: RepositoryListIdentity,
    pub open: Vec<ForgePullRequest>,
    pub requested: HashSet<i64>,
    pub fetched_at: String,
}

/// The forge whose open list a repository fetch read. A checkout may move to
/// another forge while the daemon stays up, so repository id alone cannot
/// prove a held list still belongs to its current integration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RepositoryListIdentity {
    kind: String,
    host: String,
    owner: String,
    name: String,
}

impl From<&ForgeIntegration> for RepositoryListIdentity {
    fn from(integration: &ForgeIntegration) -> Self {
        Self {
            kind: integration.kind.clone(),
            host: integration.host.clone(),
            owner: integration.owner.clone(),
            name: integration.name.clone(),
        }
    }
}

/// The last read of every request Ariadne works on, by row id, and the
/// last list read of every repository, by repository id.
#[derive(Debug, Clone, Default)]
pub struct LivePulls {
    rows: Arc<RwLock<HashMap<String, Live>>>,
    lists: Arc<RwLock<HashMap<String, RepositoryList>>>,
}

impl LivePulls {
    pub fn get(&self, id: &str) -> Option<Live> {
        self.rows
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(id)
            .cloned()
    }

    pub fn set(&self, id: &str, live: Live) {
        self.rows
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(id.to_string(), live);
    }

    /// The last list `repository_id`'s fetch read, or None before its
    /// first fetch.
    pub fn list(
        &self,
        repository_id: &str,
        integration: &ForgeIntegration,
    ) -> Option<RepositoryList> {
        self.lists
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(repository_id)
            .filter(|list| list.identity == RepositoryListIdentity::from(integration))
            .cloned()
    }

    /// Keep what a fetch just read of `repository_id`'s open requests
    /// (026): every fetch replaces the whole list, since the forge's own is
    /// the only truth of what is open now.
    pub fn set_list(
        &self,
        repository_id: &str,
        integration: &ForgeIntegration,
        open: Vec<ForgePullRequest>,
        requested: HashSet<i64>,
        fetched_at: String,
    ) {
        self.lists
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                repository_id.to_string(),
                RepositoryList {
                    identity: RepositoryListIdentity::from(integration),
                    open,
                    requested,
                    fetched_at,
                },
            );
    }

    /// The generation this cache stands at for `id`, 0 where it holds
    /// none yet: a detail read captures this before it starts its own,
    /// slower fetch, so the `set_details` it later calls knows which
    /// local addition, if any, is newer than this read's own snapshot.
    pub fn generation(&self, id: &str) -> u64 {
        self.rows
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(id)
            .map(|live| live.generation)
            .unwrap_or(0)
    }

    /// [`Self::set`], answering the attention fingerprint
    /// ([`attention_fingerprint`]) of whatever this call itself just
    /// replaced, and of `new` — both read and written under the one
    /// lock this call holds throughout, so a caller comparing them
    /// afterward is comparing against the value this exact write
    /// followed, never a snapshot taken before some slow network await,
    /// which a second call racing the first past that same await could
    /// already have moved past by the time either one gets here.
    pub fn set_fingerprinted(&self, id: &str, new: Live) -> (Option<u64>, u64) {
        let mut held = self
            .rows
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = held.get(id).map(attention_fingerprint);
        let after = attention_fingerprint(&new);
        held.insert(id.to_string(), new);
        (before, after)
    }

    /// Keep a new read of the request itself, and the details and evidence
    /// validity an earlier read found where this one read none: a call
    /// here has nothing of its own to say about whether the comment
    /// evidence is still good, so it leaves that exactly as it found it.
    pub fn set_pull(&self, id: &str, pull: ForgePullRequest, review_requested: bool) {
        let mut held = self
            .rows
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (details, details_read_at, evidence_ok, generation, local_additions) = held
            .get(id)
            .map(|live| {
                (
                    live.details.clone(),
                    live.details_read_at,
                    live.evidence_ok,
                    live.generation,
                    live.local_additions.clone(),
                )
            })
            .unwrap_or((None, None, true, 0, HashMap::new()));
        held.insert(
            id.to_string(),
            Live {
                pull,
                review_requested,
                details,
                details_read_at,
                evidence_ok,
                generation,
                local_additions,
            },
        );
    }

    /// Drop the held open list when its integration is disabled or moves to
    /// another forge. Rows keep their own reads, but a repository list has no
    /// meaning without the forge that supplied it.
    pub fn clear_list(&self, repository_id: &str) -> bool {
        self.lists
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(repository_id)
            .is_some()
    }

    /// The same as [`Self::set_pull`], but for the one case that does have
    /// something to say about the comment evidence: a detail fetch that
    /// itself failed. The last good `details` is left standing — losing
    /// comments a session still needs over one failed read would be its
    /// own regression — but `evidence_ok` goes `false`, so a reader knows
    /// this read is not merely old by a head comparison but outright
    /// unconfirmed, whether or not the head moved too.
    pub fn set_pull_evidence_failed(
        &self,
        id: &str,
        pull: ForgePullRequest,
        review_requested: bool,
    ) {
        let mut held = self
            .rows
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (details, details_read_at, generation, local_additions) = held
            .get(id)
            .map(|live| {
                (
                    live.details.clone(),
                    live.details_read_at,
                    live.generation,
                    live.local_additions.clone(),
                )
            })
            .unwrap_or((None, None, 0, HashMap::new()));
        held.insert(
            id.to_string(),
            Live {
                pull,
                review_requested,
                details,
                details_read_at,
                evidence_ok: false,
                generation,
                local_additions,
            },
        );
    }

    /// Hold what Ariadne itself just posted on the request beside the last
    /// read, as the forge now holds it: the next fetch reads it back, and
    /// until then the request's sessions find it all the same. Bumps the
    /// generation and marks each comment with it, so a `set_details`
    /// whose own read started before this write knows to carry it
    /// forward rather than read its absence as the forge deleting it.
    pub fn add_comments(&self, id: &str, posted: &[NewPullRequestComment]) {
        let mut held = self
            .rows
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(live) = held.get_mut(id) {
            for comment in posted {
                live.generation += 1;
                live.local_additions
                    .insert(comment.forge_id.clone(), live.generation);
            }
            let details = live.details.get_or_insert_with(Details::default);
            for comment in posted {
                details.comments.retain(|c| c.forge_id != comment.forge_id);
                details.comments.push(comment.clone());
            }
        }
    }

    /// Keep a fresh detail read, its comments carrying forward any this
    /// cache already held that the read's own comments do not, and that
    /// were added after `since` — the generation this cache stood at
    /// when the read's own, slower fetch began ([`Self::generation`]): a
    /// comment missing from before that generation is gone from the
    /// forge, not merely missed by a read that started too early to see
    /// it.
    pub fn set_details(
        &self,
        id: &str,
        pull: ForgePullRequest,
        review_requested: bool,
        since: u64,
        details: Details,
    ) {
        let mut held = self
            .rows
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (details, generation, local_additions) =
            merge_fresh_details(held.get(id), since, details);
        held.insert(
            id.to_string(),
            Live {
                pull,
                review_requested,
                details: Some(details),
                details_read_at: Some(Instant::now()),
                evidence_ok: true,
                generation,
                local_additions,
            },
        );
    }

    /// [`Self::set_details`], fingerprinted the same way
    /// [`Self::set_fingerprinted`] is.
    pub fn set_details_fingerprinted(
        &self,
        id: &str,
        pull: ForgePullRequest,
        review_requested: bool,
        since: u64,
        details: Details,
    ) -> (Option<u64>, u64) {
        let mut held = self
            .rows
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = held.get(id).map(attention_fingerprint);
        let (details, generation, local_additions) =
            merge_fresh_details(held.get(id), since, details);
        let new = Live {
            pull,
            review_requested,
            details: Some(details),
            details_read_at: Some(Instant::now()),
            generation,
            local_additions,
            evidence_ok: true,
        };
        let after = attention_fingerprint(&new);
        held.insert(id.to_string(), new);
        (before, after)
    }

    /// Flip `evidence_ok` false on an already-cached read, for a failure
    /// that has no new read of its own to leave standing in its place —
    /// the outer list call, or an initial, listless lookup, failing
    /// outright, rather than a detail fetch that at least read the pull
    /// itself (`set_pull_evidence_failed`). A row this cache has never
    /// read yet has nothing to invalidate: `of_row` already reads a
    /// missing cache entry as missing evidence, not a false claim.
    /// Answers whether it found a row to flip, so a caller with several
    /// rows to invalidate at once knows which of them actually moved.
    pub fn mark_evidence_failed(&self, id: &str) -> bool {
        let mut held = self
            .rows
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match held.get_mut(id) {
            Some(live) => {
                live.evidence_ok = false;
                true
            }
            None => false,
        }
    }

    /// [`Self::mark_evidence_failed`], fingerprinted the same way
    /// [`Self::set_fingerprinted`] is: both reads of
    /// [`attention_fingerprint`] bracket the one flip to `evidence_ok`,
    /// under the one lock they share with it, so the "before" here is
    /// the state this call's own flip actually followed rather than one
    /// a concurrent read raced it past.
    pub fn mark_evidence_failed_fingerprinted(&self, id: &str) -> Option<(u64, u64)> {
        let mut held = self
            .rows
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let live = held.get_mut(id)?;
        let before = attention_fingerprint(live);
        live.evidence_ok = false;
        let after = attention_fingerprint(live);
        Some((before, after))
    }

    pub fn remove(&self, id: &str) {
        self.rows
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(id);
    }
}

/// A fresh detail read's comments, merged with whatever `previous` already
/// held: one missing from the fresh read is carried forward only where
/// `previous` marks it added after `since`, the generation the read's own
/// snapshot started at — `add_comments` wrote it after this read could
/// have seen it, so its absence here says nothing of the forge deleting
/// it. One missing and not so marked, or confirmed by the fresh read on
/// its own terms, is a local addition this cache no longer has to carry:
/// dropped from what is kept, and from what is tracked after the merge
/// too, the way the review a forge user actually deleted would be.
/// Answers the merged comments, and the generation and the
/// still-unconfirmed local additions to carry into the entry this read
/// writes.
fn merge_fresh_details(
    previous: Option<&Live>,
    since: u64,
    mut details: Details,
) -> (Details, u64, HashMap<String, u64>) {
    let Some(previous) = previous else {
        return (details, 0, HashMap::new());
    };
    let mut local_additions = previous.local_additions.clone();
    if let Some(prev_details) = previous.details.as_ref() {
        for comment in &prev_details.comments {
            let in_fresh_read = details
                .comments
                .iter()
                .any(|c| c.forge_id == comment.forge_id);
            if in_fresh_read {
                local_additions.remove(&comment.forge_id);
                continue;
            }
            let added_after_since = local_additions
                .get(&comment.forge_id)
                .is_some_and(|&generation| generation > since);
            match added_after_since {
                true => details.comments.push(comment.clone()),
                false => {
                    local_additions.remove(&comment.forge_id);
                }
            }
        }
    }
    (details, previous.generation, local_additions)
}

/// A fingerprint of everything about one read the `pull_request`
/// attention producer's output can depend on: the forge's own fields
/// `forge::poll::pulls_moved` hashes for a whole repository's list,
/// read here for one row alone, beside every comment's own resolution
/// (`forge_id`, `resolved` — this item cares which thread resolved, not
/// a comment's text) and `evidence_ok` itself. Two reads that
/// fingerprint the same can never produce a different readiness item,
/// review-start item, or absence of either; two that differ might —
/// which is what [`LivePulls::set_fingerprinted`] and
/// [`LivePulls::mark_evidence_failed_fingerprinted`] compare a direct,
/// route-level read's own before and after against, rather than
/// `evidence_ok` alone, which a successful read changing nothing about
/// it can still leave exactly as it found it while every other field
/// above moves — a dismissed approval, a reopened thread, a new commit,
/// a failed check, or a request that closed.
fn attention_fingerprint(live: &Live) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let pull = &live.pull;
    (
        &pull.state,
        pull.draft,
        &pull.head_sha,
        &pull.checks,
        &pull.review_decision,
        &pull.mergeable,
        live.review_requested,
    )
        .hash(&mut hasher);
    live.evidence_ok.hash(&mut hasher);
    if let Some(details) = &live.details {
        details.head_sha.hash(&mut hasher);
        // The viewer's own unsubmitted drafts (031): hashed on its own,
        // beside the comment list, since a `drafts` attention item
        // appearing or clearing can be the only thing a detail read moved.
        details.pending_drafts.hash(&mut hasher);
        for comment in &details.comments {
            (&comment.forge_id, comment.resolved).hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// The request `row` keeps, as `live` read it, with `marks` of its
/// comments.
pub fn view(row: PullRequestRow, live: &Live, marks: &[CommentMark], login: &str) -> PullRequest {
    let details = live.details.clone().unwrap_or_default();
    let comments = answered(
        comments(&row.id, &details.comments, marks),
        &row.role,
        login,
    );
    let unanswered = waiting_threads(&comments, &row.role, login).len() as i64;
    let pull = &live.pull;
    PullRequest::of(
        row,
        PullRequestLive {
            title: pull.title.clone(),
            body: pull.body.clone(),
            author_login: pull.author_login.clone(),
            state: pull.state.clone(),
            draft: pull.draft,
            head_branch: pull.head_branch.clone(),
            head_sha: pull.head_sha.clone(),
            head_repo: pull.head_repo.clone(),
            base_branch: pull.base_branch.clone(),
            checks: pull.checks.clone(),
            review_decision: pull.review_decision.clone(),
            mergeable: pull.mergeable.clone(),
            opened_at: pull.opened_at.clone(),
            forge_updated_at: pull.updated_at.clone(),
            failed_checks: serde_json::to_string(&details.failed_checks)
                .unwrap_or_else(|_| "[]".into()),
            behind_base: details.behind_base,
            review_requested: live.review_requested,
            merge_sha: pull.merge_sha.clone(),
            unanswered_comments: unanswered,
        },
    )
}

/// The forge's comments of `row`'s request, with the marks read beside
/// them, oldest first. A thread is resolved or not as a whole, and a
/// comment is answered once a later comment of its thread is the login's
/// own.
pub fn comments(
    pull_request_id: &str,
    forge: &[NewPullRequestComment],
    marks: &[CommentMark],
) -> Vec<PullRequestComment> {
    let marks: HashMap<&str, &CommentMark> =
        marks.iter().map(|m| (m.forge_id.as_str(), m)).collect();
    let resolved: HashSet<&str> = forge
        .iter()
        .filter(|c| c.resolved)
        .map(|c| c.thread_id.as_str())
        .collect();
    let mut all: Vec<PullRequestComment> = forge
        .iter()
        .map(|c| {
            let mark = marks.get(c.forge_id.as_str());
            PullRequestComment {
                id: c.forge_id.clone(),
                pull_request_id: pull_request_id.to_string(),
                forge_id: c.forge_id.clone(),
                thread_id: c.thread_id.clone(),
                kind: c.kind.clone(),
                author_login: c.author_login.clone(),
                author_is_bot: c.author_is_bot,
                body: unsigned(&c.body),
                path: c.path.clone(),
                line: c.line,
                in_reply_to: c.in_reply_to.clone(),
                created_at: c.created_at.clone(),
                answered: false,
                resolved: resolved.contains(c.thread_id.as_str()),
                told_at: mark.and_then(|m| m.told_at.clone()),
                // The review's mark is on the forge with the comment, and
                // the store's beside it: either says so.
                from_review: c.from_review
                    || signature(&c.body).review
                    || mark.is_some_and(|m| m.from_review),
            }
        })
        .collect();
    all.sort_by(|a, b| (&a.created_at, &a.id).cmp(&(&b.created_at, &b.id)));
    all
}

/// Whether a comment is the integration login's own side: written under
/// the login, and on a request of the user's own not by an Ariadne review,
/// whose findings are the task author's to answer (029).
fn is_mine(comment: &PullRequestComment, role: &str, login: &str) -> bool {
    comment.author_login.eq_ignore_ascii_case(login) && !(comment.from_review && role == "author")
}

/// Whether `comment` is the review's own summary (029): the one comment it
/// posts to the conversation thread, under the integration's own `login`,
/// rather than to a line of the diff. `forge/news.rs` reads this to leave
/// it out of the news, since its findings are already told as inline
/// comments; `attention::pull_requests::readiness_item` reads it to leave
/// it out of what a reply must still answer, since a summary carries
/// nothing to answer.
pub(crate) fn is_review_summary(comment: &PullRequestComment, login: &str) -> bool {
    comment.thread_id == CONVERSATION
        && comment.from_review
        && comment.author_login.eq_ignore_ascii_case(login)
}

/// The comments with `answered` worked out (026): a later comment of the
/// same thread on the login's own side answers it.
pub fn answered(
    mut comments: Vec<PullRequestComment>,
    role: &str,
    login: &str,
) -> Vec<PullRequestComment> {
    let mine: Vec<(String, String, String)> = comments
        .iter()
        .filter(|c| is_mine(c, role, login))
        .map(|c| (c.thread_id.clone(), c.created_at.clone(), c.id.clone()))
        .collect();
    for comment in &mut comments {
        comment.answered = mine.iter().any(|(thread, at, id)| {
            *thread == comment.thread_id
                && (at.as_str(), id.as_str()) > (comment.created_at.as_str(), comment.id.as_str())
        });
    }
    comments
}

/// The threads that wait on the login (026): one with a comment of the
/// other side nobody answered, in a thread nobody resolved.
pub fn waiting_threads(
    comments: &[PullRequestComment],
    role: &str,
    login: &str,
) -> HashSet<String> {
    answered(comments.to_vec(), role, login)
        .iter()
        .filter(|c| !c.resolved && !c.answered && !is_mine(c, role, login))
        .map(|c| c.thread_id.clone())
        .collect()
}

/// The comments of the threads that wait on the login.
pub fn waiting(
    comments: &[PullRequestComment],
    role: &str,
    login: &str,
) -> Vec<PullRequestComment> {
    let threads = waiting_threads(comments, role, login);
    answered(comments.to_vec(), role, login)
        .into_iter()
        .filter(|c| !c.resolved && threads.contains(&c.thread_id))
        .collect()
}

/// The first comment of each thread: who opened it.
fn openers(comments: &[PullRequestComment]) -> HashMap<&str, &PullRequestComment> {
    let mut first: HashMap<&str, &PullRequestComment> = HashMap::new();
    for comment in comments {
        first.entry(comment.thread_id.as_str()).or_insert(comment);
    }
    first
}

/// The comments the request's session has not been told of: of the other
/// side, in a thread nobody resolved, and not answered yet. With
/// `opened_by_login`, only those of a thread the login opened: what a
/// reviewer session hears of (029).
pub fn untold(
    comments: &[PullRequestComment],
    role: &str,
    login: &str,
    opened_by_login: bool,
) -> Vec<PullRequestComment> {
    let comments = answered(comments.to_vec(), role, login);
    let openers = openers(&comments);
    comments
        .iter()
        .filter(|c| c.told_at.is_none() && !c.answered && !c.resolved && !is_mine(c, role, login))
        .filter(|c| {
            !opened_by_login
                || openers
                    .get(c.thread_id.as_str())
                    .is_some_and(|first| first.author_login.eq_ignore_ascii_case(login))
        })
        .cloned()
        .collect()
}

/// The answers a review session has not been told of on a request of the
/// user's own (029): comments under the login no review posted — the task
/// author's replies, or the user's own — in a thread an Ariadne review
/// opened. Every other comment there is the author's news, so no comment is
/// told to both.
pub fn untold_review_replies(
    comments: &[PullRequestComment],
    login: &str,
) -> Vec<PullRequestComment> {
    let openers = openers(comments);
    comments
        .iter()
        .filter(|c| {
            c.told_at.is_none()
                && !c.resolved
                && !c.from_review
                && c.author_login.eq_ignore_ascii_case(login)
                && c.thread_id != CONVERSATION
                && openers
                    .get(c.thread_id.as_str())
                    .is_some_and(|first| first.from_review)
        })
        .cloned()
        .collect()
}

/// The integration login of a request's repository: whose own comments
/// are no news.
pub async fn login_of(
    store: &ariadne_store::Store,
    repository_id: &str,
) -> ariadne_store::Result<String> {
    Ok(store
        .forge_integration(repository_id)
        .await?
        .and_then(|i| i.login)
        .unwrap_or_default())
}

/// The request `row` keeps, as the last read found it; None until a fetch
/// has read it.
pub async fn of_row(
    store: &ariadne_store::Store,
    live: &LivePulls,
    row: PullRequestRow,
) -> ariadne_store::Result<Option<PullRequest>> {
    let Some(read) = live.get(&row.id) else {
        return Ok(None);
    };
    let login = login_of(store, &row.repository_id).await?;
    let marks = store.pull_request_comment_marks(&row.id).await?;
    Ok(Some(view(row, &read, &marks, &login)))
}

/// The comments of a request as the last read found them, with the marks
/// beside them and `answered` worked out.
pub async fn comments_of(
    store: &ariadne_store::Store,
    live: &LivePulls,
    pull: &PullRequest,
) -> ariadne_store::Result<Vec<PullRequestComment>> {
    let forge = live
        .get(&pull.id)
        .and_then(|l| l.details)
        .map(|d| d.comments)
        .unwrap_or_default();
    let marks = store.pull_request_comment_marks(&pull.id).await?;
    let login = login_of(store, &pull.repository_id).await?;
    Ok(answered(
        comments(&pull.id, &forge, &marks),
        &pull.role,
        &login,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(forge_id: &str, thread: &str, author: &str, at: &str) -> NewPullRequestComment {
        NewPullRequestComment {
            forge_id: forge_id.into(),
            thread_id: thread.into(),
            kind: "review_comment".into(),
            author_login: author.into(),
            author_is_bot: false,
            body: format!("comment {forge_id}"),
            path: Some("src/lib.rs".into()),
            line: Some(3),
            in_reply_to: None,
            created_at: at.into(),
            resolved: false,
            from_review: false,
        }
    }

    fn mark(forge_id: &str, told: bool, from_review: bool) -> CommentMark {
        CommentMark {
            pull_request_id: "p".into(),
            forge_id: forge_id.into(),
            told_at: told.then(|| "2026-10-03T00:00:00Z".into()),
            from_review,
        }
    }

    fn ids(comments: &[PullRequestComment]) -> Vec<&str> {
        comments.iter().map(|c| c.id.as_str()).collect()
    }

    /// A thread waits on my login while its last word is another login's
    /// and nobody resolved it (026): a thread I answered last waits on
    /// nobody, and nor does a resolved one. A comment is told once, and my
    /// own comments are never news. A later comment by another login opens
    /// an answered thread again.
    #[test]
    fn a_thread_waits_on_the_login_until_it_answers_and_a_comment_is_told_once() {
        let mut resolved = comment("3", "t3", "carol", "2026-10-02T00:00:00Z");
        resolved.resolved = true;
        let mut forge = vec![
            comment("1", "t1", "alice", "2026-10-02T00:00:00Z"),
            comment("2", "t2", "bob", "2026-10-02T00:00:00Z"),
            comment("2r", "t2", "ME", "2026-10-03T00:00:00Z"),
            resolved,
        ];
        let all = answered(comments("p", &forge, &[]), "author", "me");
        assert_eq!(waiting_threads(&all, "author", "me").len(), 1);
        assert_eq!(ids(&untold(&all, "author", "me", false)), ["1"]);
        assert_eq!(ids(&waiting(&all, "author", "me")), ["1"]);
        assert!(all.iter().find(|c| c.id == "2").unwrap().answered);

        let told = answered(
            comments("p", &forge, &[mark("1", true, false)]),
            "author",
            "me",
        );
        assert!(untold(&told, "author", "me", false).is_empty(), "told once");

        forge.push(comment("1r", "t1", "me", "2026-10-04T00:00:00Z"));
        let answered_now = answered(comments("p", &forge, &[]), "author", "me");
        assert!(waiting_threads(&answered_now, "author", "me").is_empty());
        forge.push(comment("1rr", "t1", "alice", "2026-10-05T00:00:00Z"));
        let reopened = answered(comments("p", &forge, &[]), "author", "me");
        assert_eq!(waiting_threads(&reopened, "author", "me").len(), 1);
    }

    /// A reviewer session hears of the replies in the threads its login
    /// opened alone (029).
    #[test]
    fn a_reviewer_hears_of_the_replies_in_its_own_threads() {
        let forge = [
            comment("1", "mine", "me", "2026-10-02T00:00:00Z"),
            comment("1r", "mine", "alice", "2026-10-03T00:00:00Z"),
            comment("2", "theirs", "bob", "2026-10-02T00:00:00Z"),
        ];
        let all = answered(comments("p", &forge, &[]), "reviewer", "me");
        assert_eq!(ids(&untold(&all, "reviewer", "me", true)), ["1r"]);
        let untold_all = untold(&all, "reviewer", "me", false);
        let mut every = ids(&untold_all);
        every.sort();
        assert_eq!(every, ["1r", "2"]);
    }

    /// On a request of mine a comment an Ariadne review posted is another
    /// side's (029): it waits on the task's author, and is the author's
    /// news; the author's reply under my login answers it, and is the
    /// review's news alone.
    #[test]
    fn an_ariadne_finding_on_my_request_is_the_authors_and_its_answer_the_reviews() {
        let forge = [
            comment("f", "t1", "me", "2026-10-02T00:00:00Z"),
            comment("a", "t1", "me", "2026-10-03T00:00:00Z"),
        ];
        let only_the_finding = answered(
            comments("p", &forge[..1], &[mark("f", false, true)]),
            "author",
            "me",
        );
        assert_eq!(
            ids(&untold(&only_the_finding, "author", "me", false)),
            ["f"]
        );
        assert_eq!(waiting_threads(&only_the_finding, "author", "me").len(), 1);
        let both = answered(
            comments("p", &forge, &[mark("f", false, true)]),
            "author",
            "me",
        );
        assert!(
            waiting_threads(&both, "author", "me").is_empty(),
            "the reply answers it"
        );
        assert_eq!(ids(&untold_review_replies(&both, "me")), ["a"]);
        // On a request I review, my own review's comments are mine.
        let theirs = answered(
            comments("p", &forge[..1], &[mark("f", false, true)]),
            "reviewer",
            "me",
        );
        assert!(waiting_threads(&theirs, "reviewer", "me").is_empty());
    }

    fn sample_live(evidence_ok: bool) -> Live {
        Live {
            pull: ForgePullRequest {
                number: 1,
                url: "https://github.com/acme/widgets/pull/1".into(),
                title: "Fix widgets".into(),
                body: String::new(),
                author_login: "me".into(),
                state: "open".into(),
                draft: false,
                head_branch: "fix".into(),
                head_sha: "abc".into(),
                head_repo: None,
                base_branch: "main".into(),
                checks: "success".into(),
                review_decision: "approved".into(),
                mergeable: "clean".into(),
                opened_at: "2026-10-01T00:00:00Z".into(),
                updated_at: "2026-10-01T00:00:00Z".into(),
                merge_sha: None,
            },
            review_requested: false,
            details: Some(Details {
                comments: Vec::new(),
                failed_checks: Vec::new(),
                behind_base: false,
                head_sha: "abc".into(),
                pending_drafts: 0,
            }),
            details_read_at: Some(Instant::now()),
            evidence_ok,
            generation: 0,
            local_additions: HashMap::new(),
        }
    }

    /// Two direct reads of the same request racing past the forge on
    /// real, concurrent threads — one fails and flips `evidence_ok`
    /// false, the other succeeds and flips it back true — started
    /// together off one barrier to make them overlap rather than run
    /// one after the other. Whichever actually lands second must see
    /// the first's own write as what it replaced, never a snapshot
    /// taken before either one reached the forge: the race
    /// `http::pull_requests::read_now`'s `before` used to carry,
    /// captured ahead of a slow network await the other call could run
    /// entirely inside, comparing against a value neither mutation ever
    /// actually touched once it landed. Checked against the map's own
    /// final state, read back after both joins, rather than an assumed
    /// order: a thread scheduler gives no promise which of the two
    /// barrier-released threads a lock actually admits first.
    #[test]
    fn overlapping_mutations_each_compare_against_what_they_actually_replaced() {
        let live = LivePulls::default();
        live.set("p", sample_live(true));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

        let failing = live.clone();
        let failing_gate = barrier.clone();
        let failure = std::thread::spawn(move || {
            failing_gate.wait();
            failing.mark_evidence_failed_fingerprinted("p").unwrap()
        });
        let succeeding = live.clone();
        let succeeding_gate = barrier.clone();
        let recovery = std::thread::spawn(move || {
            succeeding_gate.wait();
            succeeding.set_fingerprinted("p", sample_live(true))
        });
        let (before_failure, after_failure) = failure.join().unwrap();
        let (before_recovery, after_recovery) = recovery.join().unwrap();

        // The map holds whichever of the two actually wrote last.
        if live.get("p").unwrap().evidence_ok {
            assert_eq!(
                before_recovery,
                Some(after_failure),
                "the recovery ran second; its own before must be the failure's own after"
            );
            assert_ne!(before_recovery, Some(after_recovery));
        } else {
            assert_eq!(
                Some(before_failure),
                Some(after_recovery),
                "the failure ran second; its own before must be the recovery's own after"
            );
            assert_ne!(before_failure, after_failure);
        }
    }

    /// A list read that repeats every field a detail read would also show
    /// leaves `unmoved` true, so `read_row` may skip the detail read.
    #[test]
    fn unmoved_holds_where_the_list_read_repeats_the_held_fields() {
        let live = sample_live(true);
        assert!(live.unmoved(&live.pull.clone(), live.generation, Instant::now()));
    }

    /// Each field a detail read could also catch, moved on its own, turns
    /// `unmoved` false: the forge's own "this moved" bit, a new head, each
    /// of the three evidence fields read independently of one another, and
    /// the three fields a detail read would otherwise be the only place a
    /// change to them is caught.
    #[test]
    fn unmoved_fails_where_any_one_field_moved() {
        let live = sample_live(true);
        let mutations: Vec<fn(&mut ForgePullRequest)> = vec![
            |p| p.updated_at = "2026-10-02T00:00:00Z".into(),
            |p| p.head_sha = "def".into(),
            |p| p.checks = "failure".into(),
            |p| p.review_decision = "changes_requested".into(),
            |p| p.mergeable = "dirty".into(),
            |p| p.draft = true,
            |p| p.title = "Renamed".into(),
            |p| p.state = "closed".into(),
        ];
        for mutate in mutations {
            let mut listed = live.pull.clone();
            mutate(&mut listed);
            assert!(
                !live.unmoved(&listed, live.generation, Instant::now()),
                "{listed:?}"
            );
        }
    }

    /// `unmoved` fails where the held details are missing or the last
    /// attempt to read them failed: either leaves nothing a skipped detail
    /// read could trust.
    #[test]
    fn unmoved_fails_with_no_details_or_failed_evidence() {
        let mut no_details = sample_live(true);
        no_details.details = None;
        assert!(!no_details.unmoved(
            &no_details.pull.clone(),
            no_details.generation,
            Instant::now()
        ));

        let failed = sample_live(false);
        assert!(!failed.unmoved(&failed.pull.clone(), failed.generation, Instant::now()));
    }

    /// A matching list read still refreshes details once the held detail
    /// read reaches its maximum age: changes to the base, resolved threads,
    /// and failed check names can otherwise stay invisible forever.
    #[test]
    fn unmoved_fails_when_held_details_expire() {
        let live = sample_live(true);
        let now = Instant::now();
        assert!(!live.unmoved(&live.pull.clone(), live.generation, now + DETAILS_MAX_AGE));
    }

    /// A comment Ariadne itself posted, not yet read back by a detail
    /// read, turns `unmoved` false even where the list repeats every other
    /// field: the held `details` would otherwise go stale the moment
    /// Ariadne's own post lands.
    #[test]
    fn unmoved_fails_while_a_local_addition_awaits_confirmation() {
        let mut live = sample_live(true);
        live.generation = 1;
        live.local_additions.insert("ic-1".into(), 1);
        assert!(!live.unmoved(&live.pull.clone(), live.generation, Instant::now()));
    }

    /// A local addition that lands after this read starts also turns
    /// `unmoved` false. Its generation is later than the read's snapshot,
    /// so a detail read must confirm the addition before the cache trusts
    /// its held details.
    #[test]
    fn unmoved_fails_where_a_local_addition_landed_during_this_read() {
        let mut live = sample_live(true);
        live.generation = 1;
        assert!(!live.unmoved(&live.pull.clone(), 0, Instant::now()));
    }
}
