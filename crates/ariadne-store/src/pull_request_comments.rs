//! The comments of a pull request, as the detail fetch stores them (026).
//!
//! The forge's comment id is the identity: a comment read again on a later
//! fetch updates its row and keeps the id and the `told_at` it has. Two
//! columns are the daemon's own, and both are computed here from the rows
//! of the request: `answered`, and the request's `unanswered_comments`.
use crate::{Change, PullRequest, PullRequestComment, PullRequestTold, Result, Store, now};
use ariadne_core::id::new_id;

/// One comment as the forge reports it.
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
}

impl Store {
    /// Store what one detail fetch read of a request's comments, then work
    /// out again which comments `login` answered and how many threads wait
    /// on it. Answers the request row as it now stands.
    pub async fn upsert_pull_request_comments(
        &self,
        pull_request_id: &str,
        comments: &[NewPullRequestComment],
        login: &str,
    ) -> Result<PullRequest> {
        let mut tx = self.w().begin().await?;
        let fetched = now();
        for comment in comments {
            sqlx::query(
                "INSERT INTO pull_request_comments (id, pull_request_id, forge_id, thread_id, kind,
                     author_login, author_is_bot, body, path, line, in_reply_to, created_at,
                     fetched_at, resolved)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (pull_request_id, forge_id) DO UPDATE SET
                     thread_id = excluded.thread_id,
                     kind = excluded.kind,
                     author_login = excluded.author_login,
                     author_is_bot = excluded.author_is_bot,
                     body = excluded.body,
                     path = excluded.path,
                     line = excluded.line,
                     in_reply_to = excluded.in_reply_to,
                     created_at = excluded.created_at,
                     fetched_at = excluded.fetched_at,
                     resolved = excluded.resolved",
            )
            .bind(new_id())
            .bind(pull_request_id)
            .bind(&comment.forge_id)
            .bind(&comment.thread_id)
            .bind(&comment.kind)
            .bind(&comment.author_login)
            .bind(comment.author_is_bot)
            .bind(&comment.body)
            .bind(&comment.path)
            .bind(comment.line)
            .bind(&comment.in_reply_to)
            .bind(&comment.created_at)
            .bind(&fetched)
            .bind(comment.resolved)
            .execute(&mut *tx)
            .await?;
        }
        // A thread is resolved or not as a whole: a reply stored by the
        // daemon carries no flag of its own, so it takes the thread's.
        sqlx::query(
            "UPDATE pull_request_comments AS c SET resolved = EXISTS (
                 SELECT 1 FROM pull_request_comments o
                  WHERE o.pull_request_id = c.pull_request_id AND o.thread_id = c.thread_id
                    AND o.resolved = 1)
              WHERE c.pull_request_id = ?",
        )
        .bind(pull_request_id)
        .execute(&mut *tx)
        .await?;
        let row = recount(&mut tx, pull_request_id, login).await?;
        tx.commit().await?;
        self.publish(Change::PullRequestUpdated(row.clone()));
        Ok(row)
    }

    /// Every stored comment of a request, oldest first, or only those of the
    /// threads that wait on an answer from `login`.
    pub async fn list_pull_request_comments(
        &self,
        pull_request_id: &str,
        unanswered_only: bool,
        login: &str,
    ) -> Result<Vec<PullRequestComment>> {
        Ok(sqlx::query_as(
            "SELECT * FROM pull_request_comments c
              WHERE c.pull_request_id = ?
                AND (? = 0 OR (c.resolved = 0 AND c.thread_id IN (
                     SELECT thread_id FROM pull_request_comments l
                      WHERE l.pull_request_id = c.pull_request_id
                        AND l.answered = 0 AND l.resolved = 0
                        AND lower(l.author_login) <> lower(?))))
              ORDER BY c.created_at, c.id",
        )
        .bind(pull_request_id)
        .bind(unanswered_only)
        .bind(login)
        .fetch_all(self.r())
        .await?)
    }

    /// One stored comment of a request, by its own id.
    pub async fn get_pull_request_comment(
        &self,
        pull_request_id: &str,
        id: &str,
    ) -> Result<PullRequestComment> {
        sqlx::query_as("SELECT * FROM pull_request_comments WHERE id = ? AND pull_request_id = ?")
            .bind(id)
            .bind(pull_request_id)
            .fetch_optional(self.r())
            .await?
            .ok_or_else(|| crate::not_found("pull_request_comment", id))
    }

    /// The comments the request's session has not been told of: by another
    /// login than `login`, in a thread nobody resolved, and not answered yet.
    /// With `opened_by_login`, only those of a thread `login` opened: what a
    /// reviewer session hears of (029).
    pub async fn untold_pull_request_comments(
        &self,
        pull_request_id: &str,
        login: &str,
        opened_by_login: bool,
    ) -> Result<Vec<PullRequestComment>> {
        Ok(sqlx::query_as(
            "SELECT * FROM pull_request_comments c
              WHERE c.pull_request_id = ? AND c.told_at IS NULL AND c.answered = 0
                AND c.resolved = 0 AND lower(c.author_login) <> lower(?)
                AND (? = 0 OR lower((SELECT o.author_login FROM pull_request_comments o
                                      WHERE o.pull_request_id = c.pull_request_id
                                        AND o.thread_id = c.thread_id
                                      ORDER BY o.created_at, o.id LIMIT 1)) = lower(?))
              ORDER BY c.created_at, c.id",
        )
        .bind(pull_request_id)
        .bind(login)
        .bind(opened_by_login)
        .bind(login)
        .fetch_all(self.r())
        .await?)
    }

    /// Claim a pull request's news, right before the prompt that tells it
    /// goes out (026): its comments are stamped told, and the row takes the
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
                "UPDATE pull_request_comments SET told_at = ?
                  WHERE id = ? AND pull_request_id = ? AND told_at IS NULL",
            )
            .bind(&at)
            .bind(id)
            .bind(pull_request_id)
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
                "UPDATE pull_request_comments SET told_at = NULL
                  WHERE id = ? AND pull_request_id = ?",
            )
            .bind(id)
            .bind(pull_request_id)
            .execute(&mut *tx)
            .await?;
        }
        write_told(&mut tx, pull_request_id, before).await?;
        tx.commit().await?;
        Ok(())
    }
}

/// Work out `answered` for every comment of the request and its
/// `unanswered_comments`, by the rule 026 states: a thread waits on `login`
/// while its last comment is by another login and nobody resolved it.
async fn recount(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    pull_request_id: &str,
    login: &str,
) -> Result<PullRequest> {
    sqlx::query(
        "UPDATE pull_request_comments AS c SET answered = EXISTS (
             SELECT 1 FROM pull_request_comments r
              WHERE r.pull_request_id = c.pull_request_id AND r.thread_id = c.thread_id
                AND lower(r.author_login) = lower(?)
                AND (r.created_at > c.created_at OR (r.created_at = c.created_at AND r.id > c.id)))
          WHERE c.pull_request_id = ?",
    )
    .bind(login)
    .bind(pull_request_id)
    .execute(&mut **tx)
    .await?;
    let waiting: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT thread_id) FROM pull_request_comments
          WHERE pull_request_id = ? AND resolved = 0 AND answered = 0
            AND lower(author_login) <> lower(?)",
    )
    .bind(pull_request_id)
    .bind(login)
    .fetch_one(&mut **tx)
    .await?;
    Ok(sqlx::query_as(
        "UPDATE pull_requests SET unanswered_comments = ?, updated_at = ? WHERE id = ? RETURNING *",
    )
    .bind(waiting)
    .bind(now())
    .bind(pull_request_id)
    .fetch_one(&mut **tx)
    .await?)
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
