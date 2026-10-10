//! The requests Ariadne works on (026): one row each, holding the daemon's
//! own bookkeeping and nothing the forge holds. The database owns the
//! repository/number identity.
use crate::{Change, PullRequestRow, Result, Store, StoreError, not_found, now};
use ariadne_core::id::new_id;

/// A request Ariadne starts to work on: what identifies it, whose it is,
/// and the task that opened it, where one did.
#[derive(Debug, Clone)]
pub struct NewPullRequest {
    pub repository_id: String,
    pub number: i64,
    pub url: String,
    /// `author` for a request of the user's, `reviewer` for one that asks
    /// for their review.
    pub role: String,
    pub origin_task_id: Option<String>,
}

/// What a request's session has been told, written after a news prompt went
/// out (026): the failed checks by name, whether the head is behind its
/// base, and the review decision and state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestTold {
    pub checks: Vec<String>,
    pub behind_base: bool,
    pub review_decision: String,
    pub state: String,
    /// The rolled-up state of the checks: `pending`, `success`, `failure`
    /// or `none`. A change of it is what a ready report is decided on.
    pub check_state: String,
    /// The head a reviewer session was told of (029); empty where none was.
    pub head_sha: String,
}

#[derive(Debug, Clone, Default)]
pub struct PullRequestFilter {
    pub repository_id: Option<String>,
    pub role: Option<String>,
    /// The task that opened the request: its author keeps it (005).
    pub origin_task_id: Option<String>,
}

impl Store {
    /// Start to work on a request, or answer the row that already works on
    /// it: a task that opens a request its row already holds becomes its
    /// origin, and the first origin stays. The boolean is true only for an
    /// insertion, even with concurrent callers.
    pub async fn upsert_pull_request(&self, new: NewPullRequest) -> Result<(PullRequestRow, bool)> {
        if !matches!(new.role.as_str(), "author" | "reviewer") {
            return Err(StoreError::Invalid(format!(
                "a pull request is the user's (author) or asks for their review (reviewer), not {}",
                new.role
            )));
        }
        let id = new_id();
        let ts = now();
        let row: PullRequestRow = sqlx::query_as(
            "INSERT INTO pull_requests (id, repository_id, number, url, role, origin_task_id,
                                        ready, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?)
             ON CONFLICT (repository_id, number) DO UPDATE SET
                 url = excluded.url,
                 role = excluded.role,
                 origin_task_id = COALESCE(pull_requests.origin_task_id, excluded.origin_task_id),
                 updated_at = excluded.updated_at
             RETURNING *",
        )
        .bind(&id)
        .bind(&new.repository_id)
        .bind(new.number)
        .bind(&new.url)
        .bind(&new.role)
        .bind(&new.origin_task_id)
        .bind(&ts)
        .bind(&ts)
        .fetch_one(self.w())
        .await?;
        let created = row.id == id;
        if created {
            self.publish(Change::PullRequestsChanged(row.repository_id.clone()));
        }
        Ok((row, created))
    }

    pub async fn list_pull_requests(
        &self,
        filter: PullRequestFilter,
    ) -> Result<Vec<PullRequestRow>> {
        Ok(sqlx::query_as(
            "SELECT * FROM pull_requests
              WHERE (? IS NULL OR repository_id = ?) AND (? IS NULL OR role = ?)
                AND (? IS NULL OR origin_task_id = ?)
              ORDER BY updated_at DESC, id DESC",
        )
        .bind(&filter.repository_id)
        .bind(&filter.repository_id)
        .bind(&filter.role)
        .bind(&filter.role)
        .bind(&filter.origin_task_id)
        .bind(&filter.origin_task_id)
        .fetch_all(self.r())
        .await?)
    }

    pub async fn get_pull_request(&self, id: &str) -> Result<PullRequestRow> {
        sqlx::query_as("SELECT * FROM pull_requests WHERE id = ?")
            .bind(id)
            .fetch_optional(self.r())
            .await?
            .ok_or_else(|| not_found("pull_request", id))
    }

    /// The row that works on request `number` of a repository, where one
    /// does.
    pub async fn pull_request_by_number(
        &self,
        repository_id: &str,
        number: i64,
    ) -> Result<Option<PullRequestRow>> {
        Ok(
            sqlx::query_as("SELECT * FROM pull_requests WHERE repository_id = ? AND number = ?")
                .bind(repository_id)
                .bind(number)
                .fetch_optional(self.r())
                .await?,
        )
    }

    /// Stop working on a request (026): its row and its comment marks go.
    /// The sessions that ran on it stay, with their history and their
    /// spend: they are let go of the request rather than deleted with it.
    /// Answers the row as it was, or None where none was left.
    pub async fn delete_pull_request(&self, id: &str) -> Result<Option<PullRequestRow>> {
        let mut tx = self.w().begin().await?;
        sqlx::query("UPDATE agent_sessions SET pull_request_id = NULL WHERE pull_request_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let row: Option<PullRequestRow> =
            sqlx::query_as("DELETE FROM pull_requests WHERE id = ? RETURNING *")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        tx.commit().await?;
        if let Some(row) = &row {
            self.publish(Change::PullRequestsChanged(row.repository_id.clone()));
        }
        Ok(row)
    }

    /// The request a task opened, where Ariadne still works on it.
    pub async fn pull_request_of_task(&self, task_id: &str) -> Result<Option<PullRequestRow>> {
        Ok(sqlx::query_as(
            "SELECT * FROM pull_requests WHERE origin_task_id = ?
              ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(task_id)
        .fetch_optional(self.r())
        .await?)
    }

    /// Record what the request's session was told without a prompt: the
    /// marks of a check that turned green again, or of a head that caught up
    /// with its base, so their next turn is news again. Publishes nothing:
    /// the mark is the daemon's own bookkeeping.
    pub async fn set_pull_request_told(&self, id: &str, told: &PullRequestTold) -> Result<()> {
        let mut tx = self.w().begin().await?;
        crate::pull_request_comments::write_told(&mut tx, id, told).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Point the told head of a reviewer row at `head_sha`: what its
    /// session is briefed on when it starts, so only a later push is news
    /// (029).
    pub async fn set_pull_request_told_head(&self, id: &str, head_sha: &str) -> Result<()> {
        sqlx::query("UPDATE pull_requests SET told_head_sha = ? WHERE id = ?")
            .bind(head_sha)
            .bind(id)
            .execute(self.w())
            .await?;
        Ok(())
    }

    /// Mark that this request's reviewer session has been given up on:
    /// either `scheduler::pull_requests::start_pull_request_session`'s
    /// spawn-retry budget running out (`wedged: false`), or
    /// `scheduler::quiet::relaunch_wedged`'s own exhausted-relaunch
    /// decision (`wedged: true`) — neither merely a crash the liveness
    /// sweep is about to retry. A request already marked keeps its first
    /// `since` *and* its first `wedged` — this fires again on every later
    /// pass that still finds the reviewer session given up, and a mark
    /// moving its own age or cause forward every time would hide how
    /// long, and why, the user has actually been waiting on it.
    pub async fn set_pull_request_reviewer_given_up(&self, id: &str, wedged: bool) -> Result<()> {
        let row: Option<(String,)> = sqlx::query_as(
            "UPDATE pull_requests SET reviewer_given_up_at = ?, reviewer_given_up_wedged = ?
             WHERE id = ? AND reviewer_given_up_at IS NULL
             RETURNING repository_id",
        )
        .bind(now())
        .bind(wedged)
        .bind(id)
        .fetch_optional(self.w())
        .await?;
        if let Some((repository_id,)) = row {
            self.publish(Change::PullRequestsChanged(repository_id));
        }
        Ok(())
    }

    /// Take the give-up mark down: recovery has taken ownership of this
    /// request's reviewer session again, a resume or spawn succeeded.
    pub async fn clear_pull_request_reviewer_given_up(&self, id: &str) -> Result<()> {
        let row: Option<(String,)> = sqlx::query_as(
            "UPDATE pull_requests SET reviewer_given_up_at = NULL
             WHERE id = ? AND reviewer_given_up_at IS NOT NULL
             RETURNING repository_id",
        )
        .bind(id)
        .fetch_optional(self.w())
        .await?;
        if let Some((repository_id,)) = row {
            self.publish(Change::PullRequestsChanged(repository_id));
        }
        Ok(())
    }

    /// Record the summary comment an Ariadne review keeps on the request
    /// (029), once it first posts it.
    pub async fn set_pull_request_summary(&self, id: &str, forge_id: &str) -> Result<()> {
        sqlx::query("UPDATE pull_requests SET summary_comment_id = ? WHERE id = ?")
            .bind(forge_id)
            .bind(id)
            .execute(self.w())
            .await?;
        Ok(())
    }

    /// Ask Ariadne to review a request of the user's own on `pin` with
    /// `skills` beside `pr-reviewer`, or stop asking with None (029).
    pub async fn set_pull_request_review_asked(
        &self,
        id: &str,
        asked: Option<(&crate::AgentPin, &[String])>,
    ) -> Result<PullRequestRow> {
        let skills = asked.map_or_else(Vec::new, |(_, skills)| skills.to_vec());
        let row: PullRequestRow = sqlx::query_as(
            "UPDATE pull_requests SET review_asked = ?, review_model = ?, review_effort = ?,
              review_skills = ?, updated_at = ? WHERE id = ? RETURNING *",
        )
        .bind(asked.is_some())
        .bind(asked.map(|(p, _)| p.model.clone()))
        .bind(asked.and_then(|(p, _)| p.effort.clone()))
        .bind(serde_json::to_string(&skills).unwrap_or_else(|_| "[]".into()))
        .bind(now())
        .bind(id)
        .fetch_optional(self.w())
        .await?
        .ok_or_else(|| not_found("pull_request", id))?;
        self.publish(Change::PullRequestsChanged(row.repository_id.clone()));
        Ok(row)
    }

    /// Record the head a reviewer session posted its review on (029).
    /// Answers the row, and whether the sha moved.
    pub async fn set_pull_request_reviewed(
        &self,
        id: &str,
        reviewed_sha: &str,
    ) -> Result<(PullRequestRow, bool)> {
        let changed: Option<PullRequestRow> = sqlx::query_as(
            "UPDATE pull_requests SET reviewed_sha = ?, updated_at = ?
              WHERE id = ? AND reviewed_sha IS NOT ? RETURNING *",
        )
        .bind(reviewed_sha)
        .bind(now())
        .bind(id)
        .bind(reviewed_sha)
        .fetch_optional(self.w())
        .await?;
        match changed {
            Some(row) => Ok((row, true)),
            None => Ok((self.get_pull_request(id).await?, false)),
        }
    }

    /// Set whether the request's session reports it ready to merge. Answers
    /// the row, and whether the flag moved.
    pub async fn set_pull_request_ready(
        &self,
        id: &str,
        ready: bool,
    ) -> Result<(PullRequestRow, bool)> {
        let changed: Option<PullRequestRow> = sqlx::query_as(
            "UPDATE pull_requests SET ready = ?, updated_at = ? WHERE id = ? AND ready <> ?
             RETURNING *",
        )
        .bind(ready)
        .bind(now())
        .bind(id)
        .bind(ready)
        .fetch_optional(self.w())
        .await?;
        match changed {
            Some(row) => {
                self.publish(Change::PullRequestsChanged(row.repository_id.clone()));
                Ok((row, true))
            }
            None => Ok((self.get_pull_request(id).await?, false)),
        }
    }
}
