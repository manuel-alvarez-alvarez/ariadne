//! Replying to a comment on a pull request, through `gh` (026). There is no
//! call here that resolves a thread: a human closes a thread.
use serde::Deserialize;

use super::Github;
use super::details::{issue_comment_id, review_comment_id};
use crate::forge::pulls::split_slug;
use ariadne_store::PullRequestComment;

#[derive(Deserialize)]
struct Created {
    id: i64,
}

impl Github {
    /// Reply to `comment` with `body`, and answer the forge id the reply is
    /// stored under.
    ///
    /// A review comment takes the reply in its own thread, `gh api
    /// .../pulls/comments/<id>/replies`, on the comment that opened it: the
    /// forge takes no reply to a reply. Anything else is the conversation,
    /// and the reply is a comment on it, `gh pr comment`.
    pub(crate) async fn reply(
        &self,
        repo: &str,
        number: i64,
        comment: &PullRequestComment,
        body: &str,
    ) -> Result<String, String> {
        if comment.kind == "review_comment" {
            let (host, owner, name) = split_slug(repo)?;
            let root = comment
                .in_reply_to
                .as_deref()
                .unwrap_or(&comment.forge_id)
                .trim_start_matches("rc-");
            let output = self
                .cli
                .answer(&[
                    "api",
                    &format!("repos/{owner}/{name}/pulls/{number}/comments/{root}/replies"),
                    "--hostname",
                    host,
                    "--method",
                    "POST",
                    "-f",
                    &format!("body={body}"),
                ])
                .await?;
            let created: Created = serde_json::from_str(&output)
                .map_err(|e| format!("cannot read the reply GitHub stored: {e}"))?;
            return Ok(review_comment_id(created.id));
        }
        let url = self
            .cli
            .answer(&[
                "pr",
                "comment",
                &number.to_string(),
                "--repo",
                repo,
                "--body",
                body,
            ])
            .await?;
        // `gh pr comment` answers the comment's URL, which ends in its id.
        Ok(url
            .rsplit_once("#issuecomment-")
            .and_then(|(_, id)| id.trim().parse::<i64>().ok())
            .map(issue_comment_id)
            .unwrap_or(url))
    }
}
