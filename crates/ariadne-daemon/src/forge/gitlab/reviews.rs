//! Posting one review of a merge request, through `glab api` (029). GitLab
//! has no review verdict: each inline comment is a discussion of its own,
//! and a summary note says "Request changes" where the review asks for
//! them. There is no call here that approves: the user gives every approval.
use serde::Deserialize;

use super::Gitlab;
use super::details::{note_id, project};
use crate::forge::pulls::{CONVERSATION, ReviewDraft, split_slug};
use ariadne_store::NewPullRequestComment;

#[derive(Deserialize)]
struct MergeRequest {
    diff_refs: DiffRefs,
}

#[derive(Deserialize)]
struct DiffRefs {
    base_sha: String,
    start_sha: String,
    head_sha: String,
}

#[derive(Deserialize)]
struct Discussion {
    id: String,
    #[serde(default)]
    notes: Vec<PostedNote>,
}

#[derive(Deserialize)]
struct PostedNote {
    id: i64,
    created_at: String,
}

/// The first words of a summary note that asks for changes.
const REQUEST_CHANGES: &str = "Request changes";

impl Gitlab {
    /// Post `review`: one discussion per inline comment, placed on the
    /// merge request's diff, then one summary note. Answers what it posted.
    pub(crate) async fn submit_review(
        &self,
        repo: &str,
        number: i64,
        review: &ReviewDraft,
        login: &str,
    ) -> Result<Vec<NewPullRequestComment>, String> {
        let (host, owner, name) = split_slug(repo)?;
        let base = format!("projects/{}/merge_requests/{number}", project(owner, name));
        let mut stored = Vec::new();
        if !review.comments.is_empty() {
            let output = self.cli.answer(&["api", &base, "--hostname", host]).await?;
            let refs = serde_json::from_str::<MergeRequest>(&output)
                .map_err(|e| format!("cannot read `glab api {base}`: {e}"))?
                .diff_refs;
            for comment in &review.comments {
                let output = self
                    .cli
                    .answer(&[
                        "api",
                        &format!("{base}/discussions"),
                        "--hostname",
                        host,
                        "--method",
                        "POST",
                        "-f",
                        &format!("body={}", comment.body),
                        "-f",
                        "position[position_type]=text",
                        "-f",
                        &format!("position[base_sha]={}", refs.base_sha),
                        "-f",
                        &format!("position[start_sha]={}", refs.start_sha),
                        "-f",
                        &format!("position[head_sha]={}", refs.head_sha),
                        "-f",
                        &format!("position[new_path]={}", comment.path),
                        "-f",
                        &format!("position[old_path]={}", comment.path),
                        "-F",
                        &format!("position[new_line]={}", comment.line),
                    ])
                    .await?;
                let discussion: Discussion = serde_json::from_str(&output)
                    .map_err(|e| format!("cannot read the discussion GitLab stored: {e}"))?;
                let Some(note) = discussion.notes.first() else {
                    return Err("GitLab stored a discussion with no note".into());
                };
                stored.push(NewPullRequestComment {
                    forge_id: note_id(note.id),
                    thread_id: discussion.id.clone(),
                    kind: "review_comment".into(),
                    author_login: login.to_string(),
                    author_is_bot: false,
                    body: comment.body.clone(),
                    path: Some(comment.path.clone()),
                    line: Some(comment.line),
                    in_reply_to: None,
                    created_at: note.created_at.clone(),
                    resolved: false,
                });
            }
        }
        let summary = match (review.request_changes, review.body.trim().is_empty()) {
            (true, true) => REQUEST_CHANGES.to_string(),
            (true, false) => format!("{REQUEST_CHANGES}\n\n{}", review.body),
            (false, _) => review.body.clone(),
        };
        if summary.trim().is_empty() {
            return Ok(stored);
        }
        let output = self
            .cli
            .answer(&[
                "api",
                &format!("{base}/notes"),
                "--hostname",
                host,
                "--method",
                "POST",
                "-f",
                &format!("body={summary}"),
            ])
            .await?;
        let note: PostedNote = serde_json::from_str(&output)
            .map_err(|e| format!("cannot read the note GitLab stored: {e}"))?;
        stored.push(NewPullRequestComment {
            forge_id: note_id(note.id),
            thread_id: CONVERSATION.into(),
            kind: "issue_comment".into(),
            author_login: login.to_string(),
            author_is_bot: false,
            body: summary,
            path: None,
            line: None,
            in_reply_to: None,
            created_at: note.created_at,
            resolved: false,
        });
        Ok(stored)
    }
}
