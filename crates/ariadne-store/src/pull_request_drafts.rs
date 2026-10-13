//! Drafts a repository that supervises its posts holds for the user: a
//! reply, a review or a resolve an agent wrote, which waits as `pending`
//! until the user publishes it to the forge or discards it (026).
use std::collections::HashSet;

use ariadne_core::id::new_id;

use crate::{PullRequestDraft, Result, Store, StoreError, not_found, now};

/// A draft before it is written: its id, its state and its timestamps are
/// the store's to set.
#[derive(Debug, Clone)]
pub struct NewPullRequestDraft {
    pub pull_request_id: String,
    pub session_id: String,
    /// `reply`, `review` or `resolve`.
    pub kind: String,
    /// The forge call the draft replays on publish, as JSON text.
    pub payload: String,
}

impl Store {
    /// Write a new pending draft.
    pub async fn create_pull_request_draft(
        &self,
        new: NewPullRequestDraft,
    ) -> Result<PullRequestDraft> {
        let id = new_id();
        let at = now();
        sqlx::query(
            "INSERT INTO pull_request_drafts
                (id, pull_request_id, session_id, kind, payload, state, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, 'pending', ?, ?)",
        )
        .bind(&id)
        .bind(&new.pull_request_id)
        .bind(&new.session_id)
        .bind(&new.kind)
        .bind(&new.payload)
        .bind(&at)
        .bind(&at)
        .execute(self.w())
        .await?;
        self.get_pull_request_draft(&id).await
    }

    /// One draft by id.
    pub async fn get_pull_request_draft(&self, id: &str) -> Result<PullRequestDraft> {
        sqlx::query_as("SELECT * FROM pull_request_drafts WHERE id = ?")
            .bind(id)
            .fetch_optional(self.r())
            .await?
            .ok_or_else(|| not_found("pull_request_draft", id))
    }

    /// The drafts of one request, newest first; with `state`, only those in
    /// it.
    pub async fn list_pull_request_drafts(
        &self,
        pull_request_id: &str,
        state: Option<&str>,
    ) -> Result<Vec<PullRequestDraft>> {
        Ok(match state {
            Some(state) => {
                sqlx::query_as(
                    "SELECT * FROM pull_request_drafts
                      WHERE pull_request_id = ? AND state = ?
                      ORDER BY id DESC",
                )
                .bind(pull_request_id)
                .bind(state)
                .fetch_all(self.r())
                .await?
            }
            None => {
                sqlx::query_as(
                    "SELECT * FROM pull_request_drafts
                      WHERE pull_request_id = ?
                      ORDER BY id DESC",
                )
                .bind(pull_request_id)
                .fetch_all(self.r())
                .await?
            }
        })
    }

    /// The `comment_id` of every pending reply draft of one request: a
    /// comment one already answers counts as answered, so it raises no news
    /// and takes no second reply (026).
    pub async fn pending_reply_draft_comment_ids(
        &self,
        pull_request_id: &str,
    ) -> Result<HashSet<String>> {
        Ok(self
            .list_pull_request_drafts(pull_request_id, Some("pending"))
            .await?
            .into_iter()
            .filter(|d| d.kind == "reply")
            .filter_map(|d| serde_json::from_str::<serde_json::Value>(&d.payload).ok())
            .filter_map(|v| v.get("comment_id")?.as_str().map(str::to_string))
            .collect())
    }

    /// Replace the payload of a pending draft; refused once it is published
    /// or discarded.
    pub async fn update_pull_request_draft_payload(
        &self,
        id: &str,
        payload: &str,
    ) -> Result<PullRequestDraft> {
        let draft = self.get_pull_request_draft(id).await?;
        if draft.state != "pending" {
            return Err(StoreError::Conflict(format!(
                "draft {id} is {}, not pending",
                draft.state
            )));
        }
        sqlx::query("UPDATE pull_request_drafts SET payload = ?, updated_at = ? WHERE id = ?")
            .bind(payload)
            .bind(now())
            .bind(id)
            .execute(self.w())
            .await?;
        self.get_pull_request_draft(id).await
    }

    /// Record why the user discarded a draft.
    pub async fn set_pull_request_draft_feedback(
        &self,
        id: &str,
        feedback: &str,
    ) -> Result<PullRequestDraft> {
        sqlx::query("UPDATE pull_request_drafts SET feedback = ?, updated_at = ? WHERE id = ?")
            .bind(feedback)
            .bind(now())
            .bind(id)
            .execute(self.w())
            .await?;
        self.get_pull_request_draft(id).await
    }

    /// Move a draft to `published` or `discarded`.
    pub async fn set_pull_request_draft_state(
        &self,
        id: &str,
        state: &str,
    ) -> Result<PullRequestDraft> {
        sqlx::query("UPDATE pull_request_drafts SET state = ?, updated_at = ? WHERE id = ?")
            .bind(state)
            .bind(now())
            .bind(id)
            .execute(self.w())
            .await?;
        self.get_pull_request_draft(id).await
    }
}
