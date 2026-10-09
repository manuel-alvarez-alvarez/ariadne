//! What the database keeps of the comments of a request Ariadne works on
//! (026): a mark per comment, and nothing the forge holds. A mark says the
//! request's session was told of the comment, or that an Ariadne review
//! posted it (029). The comment itself — its text, its author, its thread —
//! is read off the forge on every fetch, and its identity is the forge's id.
use crate::{CommentMark, PullRequestTold, Result, Store, now};

/// One comment as the forge reports it, read on every fetch and never
/// stored.
#[derive(Debug, Clone)]
pub struct NewPullRequestComment {
    pub forge_id: String,
    pub thread_id: String,
    /// `review_comment`, `issue_comment` or `review`.
    pub kind: String,
    pub author_login: String,
    pub author_is_bot: bool,
    pub body: String,
    pub path: Option<String>,
    pub line: Option<i64>,
    pub in_reply_to: Option<String>,
    pub created_at: String,
    pub resolved: bool,
    /// Posted by an Ariadne review session (029): the daemon's mark, read
    /// beside what the forge reports.
    pub from_review: bool,
}

impl Store {
    /// Every mark of a request's comments.
    pub async fn pull_request_comment_marks(
        &self,
        pull_request_id: &str,
    ) -> Result<Vec<CommentMark>> {
        Ok(
            sqlx::query_as("SELECT * FROM pull_request_comment_marks WHERE pull_request_id = ?")
                .bind(pull_request_id)
                .fetch_all(self.r())
                .await?,
        )
    }

    /// Mark comments an Ariadne review session posted (029): under the
    /// user's login, yet the review's, so on a request of the user's own
    /// the task's author answers them.
    pub async fn mark_review_comments(
        &self,
        pull_request_id: &str,
        forge_ids: &[String],
    ) -> Result<()> {
        let mut tx = self.w().begin().await?;
        for forge_id in forge_ids {
            sqlx::query(
                "INSERT INTO pull_request_comment_marks (pull_request_id, forge_id, from_review)
                 VALUES (?, ?, 1)
                 ON CONFLICT (pull_request_id, forge_id) DO UPDATE SET from_review = 1",
            )
            .bind(pull_request_id)
            .bind(forge_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Claim a pull request's news, right before the prompt that tells it
    /// goes out (026): its comments are marked told, and the row takes the
    /// told mark the news leaves and the time it was told.
    ///
    /// A compare and set, in one transaction: the claim holds only while the
    /// row still carries `before`, the mark the news was computed from, and
    /// every comment it names is still untold. A news computed from a mark
    /// that has moved since — told by a claim that won, or recorded with no
    /// prompt — is refused, and answers false: its prompt is skipped, and
    /// the next pass computes the news again from the mark as it stands.
    pub async fn claim_pull_request_news(
        &self,
        pull_request_id: &str,
        comment_ids: &[String],
        before: &PullRequestTold,
        told: &PullRequestTold,
    ) -> Result<bool> {
        let mut tx = self.w().begin().await?;
        let unmoved: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pull_requests
                             WHERE id = ? AND told_checks = ? AND told_behind_base = ?
                               AND COALESCE(told_review_decision, 'none') = ?
                               AND COALESCE(told_state, 'open') = ?
                               AND COALESCE(told_check_state, 'none') = ?
                               AND COALESCE(told_head_sha, '') = ?)",
        )
        .bind(pull_request_id)
        .bind(serde_json::to_string(&before.checks).unwrap_or_else(|_| "[]".into()))
        .bind(before.behind_base)
        .bind(&before.review_decision)
        .bind(&before.state)
        .bind(&before.check_state)
        .bind(&before.head_sha)
        .fetch_one(&mut *tx)
        .await?;
        if !unmoved {
            return Ok(false);
        }
        let at = now();
        for id in comment_ids {
            let stamped = sqlx::query(
                "INSERT INTO pull_request_comment_marks (pull_request_id, forge_id, told_at)
                 VALUES (?, ?, ?)
                 ON CONFLICT (pull_request_id, forge_id) DO UPDATE SET told_at = excluded.told_at
                  WHERE pull_request_comment_marks.told_at IS NULL",
            )
            .bind(pull_request_id)
            .bind(id)
            .bind(&at)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            // Dropped without a commit, so nothing this claim wrote stays.
            if stamped != 1 {
                return Ok(false);
            }
        }
        write_told(&mut tx, pull_request_id, told).await?;
        sqlx::query("UPDATE pull_requests SET news_told_at = ? WHERE id = ?")
            .bind(&at)
            .bind(pull_request_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Give a claim back: the prompt it was made for never reached the
    /// agent, so its comments are untold again and the row takes back the
    /// mark it had. The next live agent is told the same news.
    pub async fn release_pull_request_news(
        &self,
        pull_request_id: &str,
        comment_ids: &[String],
        before: &PullRequestTold,
    ) -> Result<()> {
        let mut tx = self.w().begin().await?;
        for id in comment_ids {
            sqlx::query(
                "UPDATE pull_request_comment_marks SET told_at = NULL
                  WHERE pull_request_id = ? AND forge_id = ?",
            )
            .bind(pull_request_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
        write_told(&mut tx, pull_request_id, before).await?;
        tx.commit().await?;
        Ok(())
    }
}

/// Write a request's told mark inside a transaction.
pub(crate) async fn write_told(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    pull_request_id: &str,
    told: &PullRequestTold,
) -> Result<()> {
    sqlx::query(
        "UPDATE pull_requests SET told_checks = ?, told_behind_base = ?,
                told_review_decision = ?, told_state = ?, told_check_state = ?,
                told_head_sha = NULLIF(?, '')
          WHERE id = ?",
    )
    .bind(serde_json::to_string(&told.checks).unwrap_or_else(|_| "[]".into()))
    .bind(told.behind_base)
    .bind(&told.review_decision)
    .bind(&told.state)
    .bind(&told.check_state)
    .bind(&told.head_sha)
    .bind(pull_request_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
