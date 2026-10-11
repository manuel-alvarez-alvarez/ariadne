//! What the forge says of the requests Ariadne works on, held in memory
//! alone (026).
//!
//! The database keeps Ariadne's own bookkeeping of a request
//! ([`PullRequestRow`]) and a mark per comment ([`CommentMark`]), never what
//! the forge holds: a stored title, check or comment would go stale the
//! moment somebody pushed. Each fetch reads the forge and leaves its read
//! here, keyed by the row's id; the scheduler and the routes join it to the
//! row. A daemon that restarts holds nothing until its first fetch, which
//! runs at once.
//!
//! The rules that read the comments are here too, over the forge's comments
//! and the marks: which comments a thread's answer settles, which threads
//! wait on the integration login, and which comments a session has not been
//! told of.
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use ariadne_store::{
    CommentMark, NewPullRequestComment, PullRequest, PullRequestComment, PullRequestLive,
    PullRequestRow,
};

use super::pulls::{CONVERSATION, FailedCheck, ForgePullRequest, signature, unsigned};

/// The last read of one request Ariadne works on.
#[derive(Debug, Clone)]
pub struct Live {
    pub pull: ForgePullRequest,
    /// Whether the request asks for the user's review (029).
    pub review_requested: bool,
    /// What a detail read found; None where the request was only listed.
    pub details: Option<Details>,
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
}

/// The last read of every request Ariadne works on, by row id.
#[derive(Debug, Clone, Default)]
pub struct LivePulls(Arc<RwLock<HashMap<String, Live>>>);

impl LivePulls {
    pub fn get(&self, id: &str) -> Option<Live> {
        self.0
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(id)
            .cloned()
    }

    pub fn set(&self, id: &str, live: Live) {
        self.0
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(id.to_string(), live);
    }

    /// Keep a new read of the request itself, and the details and evidence
    /// validity an earlier read found where this one read none: a call
    /// here has nothing of its own to say about whether the comment
    /// evidence is still good, so it leaves that exactly as it found it.
    pub fn set_pull(&self, id: &str, pull: ForgePullRequest, review_requested: bool) {
        let mut held = self
            .0
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (details, evidence_ok) = held
            .get(id)
            .map(|live| (live.details.clone(), live.evidence_ok))
            .unwrap_or((None, true));
        held.insert(
            id.to_string(),
            Live {
                pull,
                review_requested,
                details,
                evidence_ok,
            },
        );
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
            .0
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let details = held.get(id).and_then(|live| live.details.clone());
        held.insert(
            id.to_string(),
            Live {
                pull,
                review_requested,
                details,
                evidence_ok: false,
            },
        );
    }

    /// Hold what Ariadne itself just posted on the request beside the last
    /// read, as the forge now holds it: the next fetch reads it back, and
    /// until then the request's sessions find it all the same.
    pub fn add_comments(&self, id: &str, posted: &[NewPullRequestComment]) {
        let mut held = self
            .0
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(live) = held.get_mut(id) {
            let details = live.details.get_or_insert_with(Details::default);
            for comment in posted {
                details.comments.retain(|c| c.forge_id != comment.forge_id);
                details.comments.push(comment.clone());
            }
        }
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
            .0
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

    pub fn remove(&self, id: &str) {
        self.0
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(id);
    }
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
}
