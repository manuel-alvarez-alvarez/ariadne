//! Review repository.

use ariadne_core::ReviewVerdict;
use ariadne_core::id::new_id;

use crate::{Change, Result, Review, Store, StoreError, now};

#[derive(Debug, Clone)]
pub struct NewReview {
    pub task_id: String,
    pub round: i64,
    pub reviewer_profile_id: String,
    pub session_id: Option<String>,
    pub verdict: ReviewVerdict,
    pub body: Option<String>,
}

impl Store {
    /// Record a verdict. The UNIQUE(task, round, reviewer) constraint maps to
    /// `Conflict`: one verdict per reviewer per round.
    pub async fn create_review(&self, new: NewReview) -> Result<Review> {
        let id = new_id();
        sqlx::query(
            "INSERT INTO reviews (id, task_id, round, reviewer_profile_id, session_id, verdict, body, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&new.task_id)
        .bind(new.round)
        .bind(&new.reviewer_profile_id)
        .bind(&new.session_id)
        .bind(new.verdict.as_str())
        .bind(&new.body)
        .bind(now())
        .execute(self.w())
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => StoreError::Conflict(
                format!(
                    "reviewer {} already submitted a verdict for round {} of task {}",
                    new.reviewer_profile_id, new.round, new.task_id
                ),
            ),
            other => StoreError::Db(other),
        })?;
        let review = sqlx::query_as::<_, Review>("SELECT * FROM reviews WHERE id = ?")
            .bind(&id)
            .fetch_one(self.r())
            .await?;
        self.publish(Change::ReviewCreated(review.clone()));
        Ok(review)
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
