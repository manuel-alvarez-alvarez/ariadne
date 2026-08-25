//! Review repository.

use ariadne_core::ReviewVerdict;
use ariadne_core::id::new_id;

use crate::query::Filtered;
use crate::{Change, Result, Review, Store, StoreError, now};

#[derive(Debug, Clone)]
pub struct NewReview {
    pub task_id: String,
    pub round: i64,
    /// The reviewer of the round whose verdict this is.
    pub reviewer_profile_id: String,
    pub session_id: Option<String>,
    pub verdict: ReviewVerdict,
    pub body: Option<String>,
}

impl Store {
    /// Record a verdict. The uniqueness of a reviewer within a round maps to
    /// `Conflict`: one verdict per reviewer per round.
    pub async fn create_review(&self, new: NewReview) -> Result<Review> {
        let mut tx = self.w().begin().await?;
        let review = Self::insert_review_in_tx(&mut tx, &new).await?;
        tx.commit().await?;
        self.publish(Change::ReviewCreated(review.clone()));
        Ok(review)
    }

    /// Write one verdict inside the caller's transaction and read back the row
    /// it became, so a verdict that belongs with other writes commits with
    /// them. Announcing it is the caller's, after the commit: a row nobody has
    /// committed is not news.
    pub(crate) async fn insert_review_in_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        new: &NewReview,
    ) -> Result<Review> {
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
        .execute(&mut **tx)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                StoreError::Conflict(format!(
                    "reviewer {} already submitted a verdict for round {} of task {}",
                    new.reviewer_profile_id, new.round, new.task_id
                ))
            }
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
        Filtered::new("reviews")
            .maybe(" AND task_id = ?", Some(task_id))
            .maybe(" AND round = ?", round.map(|r| r.to_string()))
            .fetch(self, " ORDER BY id", &[])
            .await
    }
}
