//! The pull request ledger. The database owns its repository/number identity.
use crate::{Change, PullRequest, Result, Store, StoreError, not_found, now};
use ariadne_core::id::new_id;

#[derive(Debug, Clone)]
pub struct NewPullRequest {
    /// A fetch of an existing row must not recreate it after user removal.
    pub existing_id: Option<String>,
    pub repository_id: String,
    pub number: i64,
    pub url: String,
    pub title: String,
    pub author_login: String,
    pub tracked_by: String,
    pub state: String,
    pub draft: bool,
    pub head_branch: String,
    pub head_sha: String,
    pub head_repo: Option<String>,
    pub base_branch: String,
    pub checks: String,
    pub review_decision: String,
    pub origin_task_id: Option<String>,
    pub opened_at: String,
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
}

#[derive(Debug, Clone, Default)]
pub struct PullRequestFilter {
    pub repository_id: Option<String>,
    pub role: Option<String>,
    pub state: Option<String>,
}

impl Store {
    /// Insert or update one identity. Preserve user tracking and the first origin.
    /// The boolean is true only for an insertion, even with concurrent callers.
    pub async fn upsert_pull_request(&self, new: NewPullRequest) -> Result<(PullRequest, bool)> {
        let mut tx = self.w().begin().await?;
        if let Some(id) = &new.existing_id {
            let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pull_requests WHERE id = ? AND repository_id = ? AND number = ?)")
                .bind(id).bind(&new.repository_id).bind(new.number).fetch_one(&mut *tx).await?;
            if !exists {
                return Err(not_found("pull_request", id));
            }
        }
        let login: Option<String> = sqlx::query_scalar(
            "SELECT login FROM forge_integrations WHERE repository_id = ? AND enabled = 1",
        )
        .bind(&new.repository_id)
        .fetch_optional(&mut *tx)
        .await?
        .flatten();
        let login = login.ok_or_else(|| {
            StoreError::Conflict("the repository's forge integration is off".into())
        })?;
        let role = if new.author_login.eq_ignore_ascii_case(&login) {
            "author"
        } else {
            "reviewer"
        };
        let id = new_id();
        let ts = now();
        let row: PullRequest = sqlx::query_as(
            "INSERT INTO pull_requests (id, repository_id, number, url, title, author_login, tracked_by, state, draft, head_branch, head_sha, head_repo, base_branch, checks, review_decision, unanswered_comments, origin_task_id, opened_at, role, ready, last_seen_at, created_at, updated_at, cleaned_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
                    CASE WHEN ? = 'open' THEN NULL ELSE ? END)
            ON CONFLICT (repository_id, number) DO UPDATE SET
                url = excluded.url,
                title = excluded.title,
                author_login = excluded.author_login,
                tracked_by = CASE WHEN pull_requests.tracked_by = 'user' THEN 'user' ELSE excluded.tracked_by END,
                state = excluded.state,
                draft = excluded.draft,
                head_branch = excluded.head_branch,
                head_sha = excluded.head_sha,
                head_repo = excluded.head_repo,
                base_branch = excluded.base_branch,
                checks = excluded.checks,
                review_decision = excluded.review_decision,
                origin_task_id = COALESCE(pull_requests.origin_task_id, excluded.origin_task_id),
                opened_at = excluded.opened_at,
                role = excluded.role,
                last_seen_at = excluded.last_seen_at,
                updated_at = excluded.updated_at,
                cleaned_at = CASE WHEN excluded.state = 'open' THEN NULL ELSE pull_requests.cleaned_at END
            RETURNING *")
            .bind(&id)
            .bind(&new.repository_id)
            .bind(new.number)
            .bind(&new.url)
            .bind(&new.title)
            .bind(&new.author_login)
            .bind(&new.tracked_by)
            .bind(&new.state)
            .bind(new.draft)
            .bind(&new.head_branch)
            .bind(&new.head_sha)
            .bind(&new.head_repo)
            .bind(&new.base_branch)
            .bind(&new.checks)
            .bind(&new.review_decision)
            .bind(0_i64)
            .bind(&new.origin_task_id)
            .bind(&new.opened_at)
            .bind(role)
            .bind(false)
            .bind(&ts)
            .bind(&ts)
            .bind(&ts)
            // A row born ended owes no cleanup: no session ever ran for it.
            .bind(&new.state)
            .bind(&ts)
            .fetch_one(&mut *tx).await?;
        tx.commit().await?;
        let created = row.id == id;
        self.publish(if created {
            Change::PullRequestCreated(row.clone())
        } else {
            Change::PullRequestUpdated(row.clone())
        });
        Ok((row, created))
    }

    pub async fn list_pull_requests(&self, filter: PullRequestFilter) -> Result<Vec<PullRequest>> {
        Ok(sqlx::query_as("SELECT * FROM pull_requests WHERE (? IS NULL OR repository_id = ?) AND (? IS NULL OR role = ?) AND (? IS NULL OR state = ?) ORDER BY updated_at DESC, id DESC")
            .bind(&filter.repository_id).bind(&filter.repository_id)
            .bind(&filter.role).bind(&filter.role).bind(&filter.state).bind(&filter.state)
            .fetch_all(self.r()).await?)
    }

    pub async fn get_pull_request(&self, id: &str) -> Result<PullRequest> {
        sqlx::query_as("SELECT * FROM pull_requests WHERE id = ?")
            .bind(id)
            .fetch_optional(self.r())
            .await?
            .ok_or_else(|| not_found("pull_request", id))
    }

    pub async fn delete_pull_request(&self, id: &str) -> Result<PullRequest> {
        let mut tx = self.w().begin().await?;
        let row: PullRequest = sqlx::query_as("SELECT * FROM pull_requests WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| not_found("pull_request", id))?;
        if row.tracked_by != "user" {
            return Err(StoreError::Conflict(
                "only a user-tracked pull request can be removed".into(),
            ));
        }
        sqlx::query("DELETE FROM pull_requests WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.publish(Change::PullRequestDeleted(row.clone()));
        Ok(row)
    }

    /// Close open rows only while the integration is disabled or absent.
    pub async fn close_disabled_pull_requests(
        &self,
        repository_id: &str,
    ) -> Result<Vec<PullRequest>> {
        let rows: Vec<PullRequest> = sqlx::query_as("UPDATE pull_requests SET state = 'closed', updated_at = ? WHERE repository_id = ? AND state = 'open' AND NOT EXISTS (SELECT 1 FROM forge_integrations WHERE repository_id = ? AND enabled = 1) RETURNING *")
            .bind(now()).bind(repository_id).bind(repository_id).fetch_all(self.w()).await?;
        for row in &rows {
            self.publish(Change::PullRequestUpdated(row.clone()));
        }
        Ok(rows)
    }

    /// Write what a detail fetch read beside the list fetch's fields: the
    /// checks that failed, as a JSON list, and whether the head is behind
    /// its base.
    pub async fn set_pull_request_details(
        &self,
        id: &str,
        failed_checks: &str,
        behind_base: bool,
    ) -> Result<PullRequest> {
        let row: PullRequest = sqlx::query_as(
            "UPDATE pull_requests SET failed_checks = ?, behind_base = ?, updated_at = ?
              WHERE id = ? RETURNING *",
        )
        .bind(failed_checks)
        .bind(behind_base)
        .bind(now())
        .bind(id)
        .fetch_optional(self.w())
        .await?
        .ok_or_else(|| not_found("pull_request", id))?;
        self.publish(Change::PullRequestUpdated(row.clone()));
        Ok(row)
    }

    /// Record what the request's session was told without a prompt: the
    /// marks of a check that turned green again, or of a head that caught up
    /// with its base, so their next turn is news again. Publishes nothing:
    /// the mark is the daemon's own bookkeeping, and no reader of the ledger
    /// shows it.
    pub async fn set_pull_request_told(&self, id: &str, told: &PullRequestTold) -> Result<()> {
        let mut tx = self.w().begin().await?;
        crate::pull_request_comments::write_told(&mut tx, id, told).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Record that the work of an ended request is taken down, so no later
    /// pass, in this daemon or the next, owes it anything (026).
    pub async fn mark_pull_request_cleaned(&self, id: &str) -> Result<()> {
        sqlx::query("UPDATE pull_requests SET cleaned_at = ? WHERE id = ?")
            .bind(now())
            .bind(id)
            .execute(self.w())
            .await?;
        Ok(())
    }

    /// Set whether the request's session reports it ready to merge. Answers
    /// the row, and whether the flag moved.
    pub async fn set_pull_request_ready(
        &self,
        id: &str,
        ready: bool,
    ) -> Result<(PullRequest, bool)> {
        let changed: Option<PullRequest> = sqlx::query_as(
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
                self.publish(Change::PullRequestUpdated(row.clone()));
                Ok((row, true))
            }
            None => Ok((self.get_pull_request(id).await?, false)),
        }
    }

    /// Write the state the request's session reports: `open`, `merged` or
    /// `closed`.
    pub async fn set_pull_request_state(&self, id: &str, state: &str) -> Result<PullRequest> {
        if !matches!(state, "open" | "merged" | "closed") {
            return Err(StoreError::Invalid(format!(
                "a pull request is open, merged or closed, not {state}"
            )));
        }
        let row: PullRequest = sqlx::query_as(
            "UPDATE pull_requests SET state = ?1, updated_at = ?2,
                    cleaned_at = CASE WHEN ?1 = 'open' THEN NULL ELSE cleaned_at END
              WHERE id = ?3 RETURNING *",
        )
        .bind(state)
        .bind(now())
        .bind(id)
        .fetch_optional(self.w())
        .await?
        .ok_or_else(|| not_found("pull_request", id))?;
        self.publish(Change::PullRequestUpdated(row.clone()));
        Ok(row)
    }
}
