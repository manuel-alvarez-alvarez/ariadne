//! Normalized forge data, and the one path from a forge read to the row of
//! a request Ariadne starts to work on.
use super::PullRequestRef;
use ariadne_store::{
    ForgeIntegration, NewPullRequest, NewPullRequestComment, PullRequestRow, Store,
};

/// What a detail fetch reads of one request beyond the list fetch's
/// fields (026): the request as `pull_request` reads it, every comment with
/// its thread, the checks that failed on the head, and whether the head is
/// behind its base.
#[derive(Debug, Clone)]
pub struct ForgeDetails {
    pub pull: ForgePullRequest,
    pub comments: Vec<NewPullRequestComment>,
    pub failed_checks: Vec<FailedCheck>,
    pub behind_base: bool,
}

/// One check that failed on a request's head, as `pull_requests.failed_checks`
/// stores it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FailedCheck {
    pub name: String,
    pub url: String,
    pub conclusion: String,
}

/// One review a reviewer session posts in the name of the integration
/// login (029): its verdict and its inline findings. Its summary is not
/// part of it: the review keeps one summary comment on the request, written
/// with `write_summary` and edited on every round. The forge takes no
/// approval from it: the user gives every approval.
#[derive(Debug, Clone)]
pub struct ReviewDraft {
    /// Ask for changes; else the review is a comment.
    pub request_changes: bool,
    /// The head the review is on.
    pub head_sha: String,
    pub comments: Vec<DraftComment>,
}

/// One inline comment of a [`ReviewDraft`], its body already led by its
/// priority.
#[derive(Debug, Clone)]
pub struct DraftComment {
    pub path: String,
    pub line: i64,
    pub body: String,
}

/// The thread every comment on a request's conversation is in: a comment
/// on the request and a review body are flat on both forges, so a reply to
/// any of them answers the conversation.
pub(crate) const CONVERSATION: &str = "conversation";

/// What every comment an Ariadne review posts ends on (029): an HTML
/// comment, which the forge renders as nothing. It is what tells the
/// review's comments from the user's own, which share the login, and it
/// lives on the forge with the comment: a review stopped and started again,
/// whose row went in between, still knows its own findings.
pub(crate) const REVIEW_MARK: &str = "<!-- ariadne:review -->";

/// What the review's one summary comment ends on beside [`REVIEW_MARK`]: how
/// a review that has no record of its summary finds it again, to edit it
/// rather than post a second.
pub(crate) const SUMMARY_MARK: &str = "<!-- ariadne:review-summary -->";

/// `body` signed as a review's, or as its summary's.
pub(crate) fn signed(body: &str, summary: bool) -> String {
    match summary {
        true => format!("{body}\n\n{SUMMARY_MARK}\n{REVIEW_MARK}"),
        false => format!("{body}\n\n{REVIEW_MARK}"),
    }
}

/// How a body reads once its signature is taken off: the text, and whether
/// a review signed it, as its summary or not. Only the trailing marks
/// [`signed`] writes count: a comment that quotes a mark anywhere else, or
/// ends on one in a code span, is signed by nobody, and keeps its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Signature {
    pub text: String,
    pub review: bool,
    pub summary: bool,
}

pub(crate) fn signature(body: &str) -> Signature {
    let unsigned = Signature {
        text: body.to_string(),
        review: false,
        summary: false,
    };
    let Some(rest) = body.trim_end().strip_suffix(REVIEW_MARK) else {
        return unsigned;
    };
    let (rest, summary) = match rest.trim_end_matches('\n').strip_suffix(SUMMARY_MARK) {
        Some(rest) => (rest, true),
        None => (rest, false),
    };
    // `signed` leaves a blank line between the text and the marks.
    let Some(text) = rest.strip_suffix("\n\n") else {
        return unsigned;
    };
    Signature {
        text: text.to_string(),
        review: true,
        summary,
    }
}

/// `body` without the marks a review signs it with: what a reader is shown.
pub(crate) fn unsigned(body: &str) -> String {
    signature(body).text
}

/// A forge repository as the fetch names it, `host/owner/name`, split into
/// its three parts. On GitLab the owner can be a group path.
pub(crate) fn split_slug(slug: &str) -> Result<(&str, &str, &str), String> {
    let (host, path) = slug
        .split_once('/')
        .ok_or_else(|| format!("{slug} names no forge host"))?;
    let (owner, name) = path
        .rsplit_once('/')
        .ok_or_else(|| format!("{slug} names no owner"))?;
    Ok((host, owner, name))
}

#[derive(Debug, Clone)]
pub struct ForgePullRequest {
    pub number: i64,
    pub url: String,
    pub title: String,
    /// The request's description; empty where it has none.
    pub body: String,
    pub author_login: String,
    pub state: String,
    pub draft: bool,
    pub head_branch: String,
    pub head_sha: String,
    pub head_repo: Option<String>,
    pub base_branch: String,
    pub checks: String,
    pub review_decision: String,
    /// The forge's own mergeability of the head now (029): `clean`,
    /// `blocked`, `dirty` or `unknown` where the forge has not finished
    /// computing it. Read from the forge's own evidence alone, never
    /// derived from `checks` or `review_decision`.
    pub mergeable: String,
    pub opened_at: String,
    /// When the forge last saw the request move.
    pub updated_at: String,
    /// The commit a merged request landed as; None on an open one.
    pub merge_sha: Option<String>,
}

/// What a repository fetch lists: every open request, and the numbers of
/// the ones that ask for the user's review (029).
#[derive(Debug, Clone, Default)]
pub struct Listed {
    pub open: Vec<ForgePullRequest>,
    pub requested: std::collections::HashSet<i64>,
}

pub(crate) fn role(author: &str, integration: &ForgeIntegration) -> &'static str {
    if integration
        .login
        .as_deref()
        .is_some_and(|login| author.eq_ignore_ascii_case(login))
    {
        "author"
    } else {
        "reviewer"
    }
}

/// Start to work on `pull`, a request of `integration`'s repository: the
/// row that keeps Ariadne's bookkeeping of it, created where none is (026),
/// with the task that opened it where one did. Answers the row, and whether
/// it is new.
pub(crate) async fn start_work(
    store: &Store,
    integration: &ForgeIntegration,
    pull: &ForgePullRequest,
    origin_task_id: Option<String>,
) -> Result<(PullRequestRow, bool), String> {
    let reference = PullRequestRef::parse(&pull.url, integration)
        .filter(|reference| reference.number == pull.number)
        .ok_or_else(|| "the forge returned a pull request outside this repository".to_string())?;
    store
        .upsert_pull_request(NewPullRequest {
            repository_id: reference.repository_id,
            number: reference.number,
            url: pull.url.clone(),
            role: role(&pull.author_login, integration).into(),
            origin_task_id,
        })
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A body a review signed reads back as its text and its marks, as a
    /// finding or as the summary.
    #[test]
    fn a_signed_body_reads_back_as_its_text_and_its_marks() {
        let finding = signature(&signed("**[P1] Untested**\n\nNo test.", false));
        assert_eq!(
            finding,
            Signature {
                text: "**[P1] Untested**\n\nNo test.".into(),
                review: true,
                summary: false,
            }
        );
        let summary = signature(&signed("No findings.", true));
        assert_eq!(summary.text, "No findings.");
        assert!(summary.review && summary.summary);
    }

    /// Only the trailing marks `signed` writes sign a body (029): a reply
    /// that quotes a mark, mid-text or in a code span it ends on, is signed
    /// by nobody and keeps its text whole.
    #[test]
    fn a_quoted_mark_signs_nothing_and_keeps_its_text() {
        for body in [
            format!("Mid-text `{REVIEW_MARK}` is quoted."),
            format!("Ends on a code span: `{REVIEW_MARK}`"),
            format!("Quotes the summary's: {SUMMARY_MARK}"),
            format!("No blank line before it {REVIEW_MARK}"),
        ] {
            let read = signature(&body);
            assert!(!read.review && !read.summary, "{body}");
            assert_eq!(read.text, body);
        }
    }
}
