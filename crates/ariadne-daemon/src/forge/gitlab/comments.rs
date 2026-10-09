//! Replying to a note on a merge request, and resolving a discussion a
//! reviewer session opened once a push fixed it, through `glab api` (026,
//! 029).
use serde::Deserialize;

use super::Gitlab;
use super::details::{note_id, project};
use crate::forge::pulls::{CONVERSATION, split_slug};
use ariadne_store::PullRequestComment;

#[derive(Deserialize)]
struct Created {
    id: i64,
}

impl Gitlab {
    /// Reply to `comment` with `body` as a note on its discussion, and
    /// answer the forge id the reply is stored under.
    pub(crate) async fn reply(
        &self,
        repo: &str,
        number: i64,
        comment: &PullRequestComment,
        body: &str,
    ) -> Result<String, String> {
        let (host, owner, name) = split_slug(repo)?;
        let output = self
            .cli
            .answer(&[
                "api",
                &format!(
                    "projects/{}/merge_requests/{number}/discussions/{}/notes",
                    project(owner, name),
                    comment.thread_id
                ),
                "--hostname",
                host,
                "--method",
                "POST",
                "-f",
                &format!("body={body}"),
            ])
            .await?;
        let created: Created = serde_json::from_str(&output)
            .map_err(|e| format!("cannot read the note GitLab stored: {e}"))?;
        Ok(note_id(created.id))
    }

    /// Resolve the discussion `comment` is in. The conversation of notes
    /// that start no discussion is none, and nothing resolves it.
    pub(crate) async fn resolve(
        &self,
        repo: &str,
        number: i64,
        comment: &PullRequestComment,
    ) -> Result<(), String> {
        if comment.thread_id == CONVERSATION {
            return Err("only a discussion resolves".into());
        }
        let (host, owner, name) = split_slug(repo)?;
        self.cli
            .answer(&[
                "api",
                &format!(
                    "projects/{}/merge_requests/{number}/discussions/{}",
                    project(owner, name),
                    comment.thread_id
                ),
                "--hostname",
                host,
                "--method",
                "PUT",
                "-f",
                "resolved=true",
            ])
            .await?;
        Ok(())
    }
}
