//! Review repository.

use ariadne_core::ReviewVerdict;
use ariadne_core::id::new_id;

use crate::{Change, Result, Review, ReviewAuthor, Store, StoreError, now};

#[derive(Debug, Clone)]
pub struct NewReview {
    pub task_id: String,
    pub round: i64,
    /// Who the verdict is from: a profile of the task, or the forge whose
    /// reviewers the daemon relayed.
    pub author: ReviewAuthor,
    pub session_id: Option<String>,
    pub verdict: ReviewVerdict,
    pub body: Option<String>,
}

impl Store {
    /// Record a verdict. The uniqueness of an author within a round maps to
    /// `Conflict`: one verdict per reviewer per round, and one relay of what
    /// a published request says per round.
    pub async fn create_review(&self, new: NewReview) -> Result<Review> {
        let mut tx = self.w().begin().await?;
        let review = Self::insert_review_in_tx(&mut tx, &new).await?;
        tx.commit().await?;
        self.publish(Change::ReviewCreated(review.clone()));
        Ok(review)
    }

    /// Write one verdict inside the caller's transaction and read back the row
    /// it became, so a verdict that belongs with other writes commits with
    /// them — see [`Store::relay_pull_request_feedback`]. Announcing it is the
    /// caller's, after the commit: a row nobody has committed is not news.
    pub(crate) async fn insert_review_in_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        new: &NewReview,
    ) -> Result<Review> {
        let id = new_id();
        let (profile_id, author_role) = match &new.author {
            ReviewAuthor::Profile(profile_id) => (Some(profile_id.as_str()), None),
            ReviewAuthor::Role(role) => (None, Some(role.as_str())),
        };
        sqlx::query(
            "INSERT INTO reviews (id, task_id, round, reviewer_profile_id, author_role, session_id, verdict, body, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&new.task_id)
        .bind(new.round)
        .bind(profile_id)
        .bind(author_role)
        .bind(&new.session_id)
        .bind(new.verdict.as_str())
        .bind(&new.body)
        .bind(now())
        .execute(&mut **tx)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => StoreError::Conflict(
                format!(
                    "{} already submitted a verdict for round {} of task {}",
                    profile_id.map_or_else(
                        || format!("the {}", author_role.unwrap_or("author")),
                        |id| format!("reviewer {id}")
                    ),
                    new.round,
                    new.task_id
                ),
            ),
            other => StoreError::Db(other),
        })?;
        Ok(
            sqlx::query_as::<_, Review>("SELECT * FROM reviews WHERE id = ?")
                .bind(&id)
                .fetch_one(&mut **tx)
                .await?,
        )
    }

    pub async fn list_reviews(&self, task_id: &str, round: Option<i64>) -> Result<Vec<Review>> {
        let rows = match round {
            Some(r) => {
                sqlx::query_as::<_, Review>(
                    "SELECT * FROM reviews WHERE task_id = ? AND round = ? ORDER BY id",
                )
                .bind(task_id)
                .bind(r)
                .fetch_all(self.r())
                .await?
            }
            None => {
                sqlx::query_as::<_, Review>("SELECT * FROM reviews WHERE task_id = ? ORDER BY id")
                    .bind(task_id)
                    .fetch_all(self.r())
                    .await?
            }
        };
        Ok(rows)
    }
}
