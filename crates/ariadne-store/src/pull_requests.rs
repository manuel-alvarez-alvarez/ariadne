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
            "INSERT INTO pull_requests (id, repository_id, number, url, title, author_login, tracked_by, state, draft, head_branch, head_sha, head_repo, base_branch, checks, review_decision, unanswered_comments, origin_task_id, opened_at, role, ready, last_seen_at, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
                updated_at = excluded.updated_at
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
    pub async fn close_disabled_pull_requests(&self, repository_id: &str) -> Result<()> {
        let rows: Vec<PullRequest> = sqlx::query_as("UPDATE pull_requests SET state = 'closed', updated_at = ? WHERE repository_id = ? AND state = 'open' AND NOT EXISTS (SELECT 1 FROM forge_integrations WHERE repository_id = ? AND enabled = 1) RETURNING *")
            .bind(now()).bind(repository_id).bind(repository_id).fetch_all(self.w()).await?;
        for row in rows {
            self.publish(Change::PullRequestUpdated(row));
        }
        Ok(())
    }
}
