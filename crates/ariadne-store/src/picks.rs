//! The pick that ends a several-author task's review.
//!
//! A task staffed with several authors runs one review per author, and every
//! author can end up approved — approval says the change is sound, not that
//! it is the one to land. The pick is the second judgement: once every author
//! is approved, each reviewer names the author whose branch should land, and
//! the author with the most picks wins.
//!
//! A pick is a decision recorded on the task, like a transition, rather than
//! a message on the channel: nothing is said to anyone by it, and the schema
//! is what holds a reviewer to one.

use std::collections::HashSet;

use ariadne_core::MessageKind;

use crate::{Result, Store, StoreError, TaskAgent, TaskPick, now};

impl Store {
    /// Whether every author of a task stands approved: each one has an open
    /// review request, no reviewer asked it for changes since, and every
    /// staffed reviewer approved it. This is the gate the pick waits behind.
    pub async fn authors_all_approved(&self, task_id: &str) -> Result<bool> {
        let authors = self.list_task_authors(task_id).await?;
        let reviewers = self.list_task_reviewers(task_id).await?;
        if authors.is_empty() {
            return Ok(false);
        }
        for author in &authors {
            if self
                .open_review_request_of(task_id, &author.id)
                .await?
                .is_none()
            {
                return Ok(false);
            }
            let verdicts = self.open_verdicts_of(task_id, &author.id).await?;
            if verdicts
                .iter()
                .any(|m| m.kind() == Some(MessageKind::RequestChanges))
            {
                return Ok(false);
            }
            let approved: HashSet<&str> = verdicts
                .iter()
                .filter(|m| m.kind() == Some(MessageKind::Approve))
                .filter_map(|m| m.from_agent_id.as_deref())
                .collect();
            if approved.len() < reviewers.len() {
                return Ok(false);
            }
        }
        Ok(true)
    }
    /// Record one reviewer's pick. The schema holds a reviewer to one pick
    /// per task, and a second one is refused by the reviewer's name.
    ///
    /// Whether the pick may happen at all — a task under review, every author
    /// approved — is the daemon's gate, checked where the call comes in; what
    /// is held here is the shape of the row.
    pub async fn record_pick(
        &self,
        task_id: &str,
        reviewer_agent_id: &str,
        author_agent_id: &str,
    ) -> Result<TaskPick> {
        sqlx::query(
            "INSERT INTO task_picks (task_id, reviewer_agent_id, author_agent_id, created_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(task_id)
        .bind(reviewer_agent_id)
        .bind(author_agent_id)
        .bind(now())
        .execute(self.w())
        .await
        .map_err(|e| one_pick(e, task_id, reviewer_agent_id))?;
        self.publish_task_update(task_id, 1).await?;
        Ok(
            sqlx::query_as("SELECT * FROM task_picks WHERE task_id = ? AND reviewer_agent_id = ?")
                .bind(task_id)
                .bind(reviewer_agent_id)
                .fetch_one(self.r())
                .await?,
        )
    }

    /// The picks recorded on a task, oldest first.
    pub async fn list_task_picks(&self, task_id: &str) -> Result<Vec<TaskPick>> {
        Ok(
            sqlx::query_as("SELECT * FROM task_picks WHERE task_id = ? ORDER BY created_at")
                .bind(task_id)
                .fetch_all(self.r())
                .await?,
        )
    }

    /// Record which author the pick settled on. The winner's branch is what
    /// the task lands from here.
    pub async fn set_task_picked(&self, task_id: &str, agent_id: &str) -> Result<()> {
        let n = sqlx::query("UPDATE tasks SET picked_agent_id = ?, updated_at = ? WHERE id = ?")
            .bind(agent_id)
            .bind(now())
            .bind(task_id)
            .execute(self.w())
            .await?
            .rows_affected();
        self.publish_task_update(task_id, n).await
    }

    /// Forget the picks of a task, and the winner they settled on: a task
    /// retried starts its review over, and so must the pick.
    pub async fn clear_task_picks(&self, task_id: &str) -> Result<()> {
        let mut tx = self.w().begin().await?;
        let picks = sqlx::query("DELETE FROM task_picks WHERE task_id = ?")
            .bind(task_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        let winner = sqlx::query(
            "UPDATE tasks SET picked_agent_id = NULL, updated_at = ?
             WHERE id = ? AND picked_agent_id IS NOT NULL",
        )
        .bind(now())
        .bind(task_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        self.publish_task_update(task_id, picks + winner).await
    }
}

/// The author the picks name once every reviewer has picked: the one with the
/// most picks, and on a tie the one the orchestrator listed first.
pub fn picked_winner<'a>(authors: &'a [TaskAgent], picks: &[TaskPick]) -> Option<&'a TaskAgent> {
    let mut winner: Option<(&TaskAgent, usize)> = None;
    for author in authors {
        let votes = picks
            .iter()
            .filter(|p| p.author_agent_id == author.id)
            .count();
        // Strictly more: on a tie the author already held — the one listed
        // first — stays the winner.
        if winner.is_none_or(|(_, best)| votes > best) {
            winner = Some((author, votes));
        }
    }
    winner.map(|(author, _)| author)
}

/// The primary key that holds a reviewer to one pick per task, said in the
/// terms the caller used.
fn one_pick(e: sqlx::Error, task_id: &str, reviewer: &str) -> StoreError {
    match e {
        sqlx::Error::Database(ref db) if db.is_unique_violation() => StoreError::Conflict(format!(
            "reviewer {reviewer} has already picked a winner for task {task_id}"
        )),
        other => StoreError::Db(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn author(id: &str, ordinal: i64) -> TaskAgent {
        TaskAgent {
            id: id.into(),
            task_id: "01task".into(),
            seat: "author".into(),
            ordinal,
            model: "stub:test-model".into(),
            effort: None,
            brief: None,
        }
    }

    fn pick(reviewer: &str, author: &str) -> TaskPick {
        TaskPick {
            task_id: "01task".into(),
            reviewer_agent_id: reviewer.into(),
            author_agent_id: author.into(),
            created_at: String::new(),
        }
    }

    /// The winner is the author with the most picks; a tie goes to the one
    /// the orchestrator listed first, and no picks at all names that one too.
    #[test]
    fn the_most_picked_author_wins_and_a_tie_goes_to_the_first_listed() {
        let authors = [author("01a", 0), author("01b", 1), author("01c", 2)];

        let majority = [
            pick("01r1", "01b"),
            pick("01r2", "01b"),
            pick("01r3", "01a"),
        ];
        assert_eq!(picked_winner(&authors, &majority).unwrap().id, "01b");

        let tied = [pick("01r1", "01c"), pick("01r2", "01b")];
        assert_eq!(picked_winner(&authors, &tied).unwrap().id, "01b");

        assert_eq!(picked_winner(&authors, &[]).unwrap().id, "01a");
        assert!(picked_winner(&[], &[]).is_none());
    }
}
