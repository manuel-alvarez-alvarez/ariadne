//! Posting one review of a pull request, through `gh api` (029). The event
//! is `REQUEST_CHANGES` or `COMMENT`: there is no call here that approves,
//! since the user gives every approval.
use serde::Deserialize;

use super::Github;
use super::details::{review_comment_id, review_id};
use crate::forge::pulls::{CONVERSATION, ReviewDraft, split_slug};
use ariadne_store::NewPullRequestComment;

#[derive(Deserialize)]
struct Login {
    login: String,
}

/// One event of a request's timeline: the ones that say whether a review is
/// still asked of a login.
#[derive(Deserialize)]
struct TimelineEvent {
    #[serde(default)]
    event: String,
    requested_reviewer: Option<Login>,
}

#[derive(Deserialize)]
struct Posted {
    id: i64,
    submitted_at: Option<String>,
}

#[derive(Deserialize)]
struct PostedComment {
    id: i64,
    #[serde(default)]
    body: String,
    path: Option<String>,
    line: Option<i64>,
    created_at: String,
}

impl Github {
    /// Post `review` as one review with its inline comments, `gh api
    /// repos/<owner>/<name>/pulls/<n>/reviews`, then read back the comments
    /// it holds, so each is stored under the id the forge gave it.
    pub(crate) async fn submit_review(
        &self,
        repo: &str,
        number: i64,
        review: &ReviewDraft,
        login: &str,
    ) -> Result<Vec<NewPullRequestComment>, String> {
        let (host, owner, name) = split_slug(repo)?;
        let path = format!("repos/{owner}/{name}/pulls/{number}/reviews");
        let event = match review.request_changes {
            true => "REQUEST_CHANGES",
            false => "COMMENT",
        };
        let mut args: Vec<String> = ["api", &path, "--hostname", host, "--method", "POST"]
            .map(String::from)
            .into();
        // `-f` sends a string as it is; `-F` sends the line as a number.
        // Each `comments[]` field after a `path` belongs to that comment.
        let mut field = |flag: &str, value: String| args.extend([flag.to_string(), value]);
        field("-f", format!("event={event}"));
        field("-f", format!("body={}", review.body));
        field("-f", format!("commit_id={}", review.head_sha));
        for comment in &review.comments {
            field("-f", format!("comments[][path]={}", comment.path));
            field("-F", format!("comments[][line]={}", comment.line));
            field("-f", "comments[][side]=RIGHT".to_string());
            field("-f", format!("comments[][body]={}", comment.body));
        }
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = self.cli.answer(&args).await?;
        let posted: Posted = serde_json::from_str(&output)
            .map_err(|e| format!("cannot read the review GitHub stored: {e}"))?;
        let at = posted.submitted_at.unwrap_or_else(|| {
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        });
        let mut stored = Vec::new();
        if !review.body.trim().is_empty() {
            stored.push(NewPullRequestComment {
                forge_id: review_id(posted.id),
                thread_id: CONVERSATION.into(),
                kind: "review".into(),
                author_login: login.to_string(),
                author_is_bot: false,
                body: review.body.clone(),
                path: None,
                line: None,
                in_reply_to: None,
                created_at: at.clone(),
                resolved: false,
                from_review: false,
            });
        }
        if review.comments.is_empty() {
            return Ok(stored);
        }
        let output = self
            .cli
            .answer(&[
                "api",
                &format!("{path}/{}/comments", posted.id),
                "--hostname",
                host,
            ])
            .await?;
        let comments: Vec<PostedComment> = serde_json::from_str(&output)
            .map_err(|e| format!("cannot read the comments of the review GitHub stored: {e}"))?;
        for comment in comments {
            // The thread is the comment's own until the next detail fetch
            // reads the forge's thread id for it.
            stored.push(NewPullRequestComment {
                forge_id: review_comment_id(comment.id),
                thread_id: review_comment_id(comment.id),
                kind: "review_comment".into(),
                author_login: login.to_string(),
                author_is_bot: false,
                body: comment.body,
                path: comment.path,
                line: comment.line,
                in_reply_to: None,
                created_at: comment.created_at,
                resolved: false,
                from_review: false,
            });
        }
        Ok(stored)
    }

    /// Whether request `number` still asks for the review of `login`, read
    /// off its timeline (029). GitHub stops listing a request as asking for a
    /// review once the reviewer reviewed it, and writes no removal event for
    /// that, so the list cannot say it. The last request or removed request
    /// for `login` decides. A review decides nothing: a review posted after a
    /// withdrawal does not ask again.
    pub(crate) async fn review_still_requested(
        &self,
        repo: &str,
        number: i64,
        login: &str,
    ) -> Result<bool, String> {
        let (host, owner, name) = split_slug(repo)?;
        let path = format!("repos/{owner}/{name}/issues/{number}/timeline");
        let output = self
            .cli
            .answer(&["api", &path, "--hostname", host, "--paginate"])
            .await?;
        let mut requested = false;
        for page in serde_json::Deserializer::from_str(&output).into_iter::<Vec<TimelineEvent>>() {
            let page = page.map_err(|e| format!("cannot read `gh api {path}`: {e}"))?;
            for event in page {
                let mine = |who: &Option<Login>| {
                    who.as_ref()
                        .is_some_and(|w| w.login.eq_ignore_ascii_case(login))
                };
                match event.event.as_str() {
                    "review_requested" if mine(&event.requested_reviewer) => requested = true,
                    "review_request_removed" if mine(&event.requested_reviewer) => {
                        requested = false
                    }
                    _ => {}
                }
            }
        }
        Ok(requested)
    }
}
