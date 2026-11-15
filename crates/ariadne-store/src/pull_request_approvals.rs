//! Ariadne's own approval of a request (029): the head its review session
//! approved. Keyed by repository and number, so it outlives the request's
//! row, and cleared once the head moves. No forge approval is written.
use crate::{Change, Result, Store, now};

/// One approval: the exact head it was given on, and when.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct PullRequestApproval {
    pub repository_id: String,
    pub number: i64,
    pub head_sha: String,
    pub approved_at: String,
}

impl Store {
    /// Approve request `number` on `head_sha`. An approval of the same head
    /// keeps its first `approved_at`; one of another head replaces it.
    pub async fn approve_pull_request(
        &self,
        repository_id: &str,
        number: i64,
        head_sha: &str,
    ) -> Result<PullRequestApproval> {
        let approval: PullRequestApproval = sqlx::query_as(
            "INSERT INTO pull_request_approvals (repository_id, number, head_sha, approved_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT (repository_id, number) DO UPDATE SET
                 approved_at = CASE WHEN pull_request_approvals.head_sha = excluded.head_sha
                                    THEN pull_request_approvals.approved_at
                                    ELSE excluded.approved_at END,
                 head_sha = excluded.head_sha
             RETURNING *",
        )
        .bind(repository_id)
        .bind(number)
        .bind(head_sha)
        .bind(now())
        .fetch_one(self.w())
        .await?;
        self.publish(Change::PullRequestsChanged(repository_id.to_string()));
        Ok(approval)
    }

    /// Withdraw the approval of request `number`. True where one was held.
    pub async fn withdraw_pull_request_approval(
        &self,
        repository_id: &str,
        number: i64,
    ) -> Result<bool> {
        let done = sqlx::query(
            "DELETE FROM pull_request_approvals WHERE repository_id = ? AND number = ?",
        )
        .bind(repository_id)
        .bind(number)
        .execute(self.w())
        .await?;
        let withdrawn = done.rows_affected() > 0;
        if withdrawn {
            self.publish(Change::PullRequestsChanged(repository_id.to_string()));
        }
        Ok(withdrawn)
    }

    /// The approval of request `number`, whatever head it names.
    pub async fn pull_request_approval(
        &self,
        repository_id: &str,
        number: i64,
    ) -> Result<Option<PullRequestApproval>> {
        Ok(sqlx::query_as(
            "SELECT * FROM pull_request_approvals WHERE repository_id = ? AND number = ?",
        )
        .bind(repository_id)
        .bind(number)
        .fetch_optional(self.r())
        .await?)
    }

    /// Delete each approval whose request a read now finds on another head
    /// (`heads` is number and head). A head that comes back later finds
    /// nothing to restore. Answers how many went.
    pub async fn clear_moved_pull_request_approvals(
        &self,
        repository_id: &str,
        heads: &[(i64, &str)],
    ) -> Result<u64> {
        // One read first: a fetch runs this on every round, and most rounds
        // hold no approval to clear, so they write nothing.
        let held: Vec<PullRequestApproval> =
            sqlx::query_as("SELECT * FROM pull_request_approvals WHERE repository_id = ?")
                .bind(repository_id)
                .fetch_all(self.r())
                .await?;
        let mut cleared = 0;
        for approval in held {
            let Some((_, head_sha)) = heads.iter().find(|(n, _)| *n == approval.number) else {
                continue;
            };
            cleared += sqlx::query(
                "DELETE FROM pull_request_approvals
                  WHERE repository_id = ? AND number = ? AND head_sha <> ?",
            )
            .bind(repository_id)
            .bind(approval.number)
            .bind(head_sha)
            .execute(self.w())
            .await?
            .rows_affected();
        }
        if cleared > 0 {
            self.publish(Change::PullRequestsChanged(repository_id.to_string()));
        }
        Ok(cleared)
    }
}
