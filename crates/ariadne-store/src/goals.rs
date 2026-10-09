//! Goal repository.

use ariadne_core::GoalStatus;
use ariadne_core::id::new_id;
use chrono::DateTime;

use crate::{AgentPin, Change, Goal, Repository, Result, Store, StoreError, not_found, now};

/// The fact a goal writes once it moves to `completed` or `cancelled`.
const GOAL_ENDED: &str = "goal_ended";

#[derive(Debug, Clone)]
pub struct NewGoal {
    /// The workflow every task of the goal runs on. None = the default of
    /// its first repository. Fixed once the goal is created: its columns are
    /// snapshotted into `goal_steps`.
    pub workflow: Option<String>,
    pub title: String,
    pub description: String,
    pub issue_url: Option<String>,
    /// Ids of registered repositories the goal works in; each must exist.
    /// The goal reads them live, so editing one moves the goal with it.
    pub repository_ids: Vec<String>,
    /// What this goal's orchestrator runs on: its model,
    /// `<agent>:<model>`, and the effort where one was chosen.
    pub pin: AgentPin,
}

/// The workflow a new goal gets: its creator's choice or the default of the
/// first repository in the order goals show.
fn goal_workflow(requested: Option<String>, repositories: &[Repository]) -> Result<String> {
    if let Some(name) = requested {
        return Ok(name);
    }
    repositories
        .iter()
        .min_by_key(|repository| (&repository.path, &repository.base_branch))
        .map(|repository| repository.default_workflow.clone())
        .ok_or_else(|| StoreError::Invalid("a goal needs at least one repo".into()))
}

impl Store {
    pub async fn create_goal(&self, new: NewGoal) -> Result<Goal> {
        self.create_goal_as(new, GoalStatus::Planning, true).await
    }

    async fn create_goal_as(
        &self,
        new: NewGoal,
        status: GoalStatus,
        orchestrated: bool,
    ) -> Result<Goal> {
        if new.repository_ids.is_empty() {
            return Err(StoreError::Invalid("a goal needs at least one repo".into()));
        }
        // Validated before the goal row is written, so an unknown id leaves
        // nothing behind. The same repository named twice is one reference.
        let mut repository_ids: Vec<String> = Vec::with_capacity(new.repository_ids.len());
        let mut repositories = Vec::with_capacity(new.repository_ids.len());
        for id in &new.repository_ids {
            let repository = self.get_repository(id).await?;
            if !repository_ids.contains(id) {
                repository_ids.push(id.clone());
                repositories.push(repository);
            }
        }
        let workflow = goal_workflow(new.workflow, &repositories)?;
        // The columns are read now and kept with the goal: a later edit of
        // the catalog reaches later goals alone.
        let steps =
            ariadne_core::workflow::parse(self.get_workflow(&workflow).await?.document_text())
                .map_err(|e| StoreError::Invalid(e.to_string()))?
                .steps;
        let id = new_id();
        let ts = now();
        let mut tx = self.w().begin().await?;
        let (model, effort) = AgentPin::columns(&new.pin);
        sqlx::query(
            "INSERT INTO goals (id, title, description, issue_url, status,
                                orchestrated, model, effort, workflow, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&new.title)
        .bind(&new.description)
        .bind(&new.issue_url)
        .bind(status.as_str())
        .bind(orchestrated)
        .bind(&model)
        .bind(&effort)
        .bind(&workflow)
        .bind(&ts)
        .bind(&ts)
        .execute(&mut *tx)
        .await?;
        for (ordinal, step) in steps.iter().enumerate() {
            sqlx::query("INSERT INTO goal_steps (goal_id, ordinal, id, title, description, skills, rank, gate) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(&id).bind(ordinal as i64).bind(&step.id).bind(&step.title)
                .bind(&step.description).bind(serde_json::to_string(&step.skills).expect("skills serialize"))
                .bind(step.rank.map(|r| r.as_str())).bind(step.gate.map(|g| g.as_str()))
                .execute(&mut *tx).await?;
        }
        for repository_id in &repository_ids {
            sqlx::query("INSERT INTO goal_repositories (goal_id, repository_id) VALUES (?, ?)")
                .bind(&id)
                .bind(repository_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        let goal = self.get_goal(&id).await?;
        self.publish(Change::GoalCreated(goal.clone()));
        Ok(goal)
    }

    /// The columns of a goal's workflow as they were when the goal was
    /// created, in column order.
    pub async fn goal_steps(&self, id: &str) -> Result<Vec<crate::GoalStep>> {
        Ok(
            sqlx::query_as("SELECT * FROM goal_steps WHERE goal_id = ? ORDER BY ordinal")
                .bind(id)
                .fetch_all(self.r())
                .await?,
        )
    }

    pub async fn get_goal(&self, id: &str) -> Result<Goal> {
        self.fetch_by("goal", "goals", "id", id).await
    }

    /// List goals, narrowed to `statuses` (a goal matches any one of them).
    /// An empty slice means no status filter at all.
    pub async fn list_goals(&self, statuses: &[GoalStatus]) -> Result<Vec<Goal>> {
        let mut sql = String::from("SELECT * FROM goals");
        if !statuses.is_empty() {
            sql.push_str(" WHERE status IN (");
            for i in 0..statuses.len() {
                if i > 0 {
                    sql.push_str(", ");
                }
                sql.push('?');
            }
            sql.push(')');
        }
        sql.push_str(" ORDER BY id");
        // Safe: only fixed clause fragments are appended; values are bound.
        let mut q = sqlx::query_as::<_, Goal>(sqlx::AssertSqlSafe(sql));
        for status in statuses {
            q = q.bind(status.as_str());
        }
        Ok(q.fetch_all(self.r()).await?)
    }

    pub async fn set_goal_status(&self, id: &str, status: GoalStatus) -> Result<Goal> {
        let mut tx = self.w().begin().await?;
        let goal: Goal = Self::fetch_by_in_tx(&mut tx, "goal", "goals", id).await?;
        let from = goal.status();
        sqlx::query("UPDATE goals SET status = ?, updated_at = ? WHERE id = ?")
            .bind(status.as_str())
            .bind(now())
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if from != status && matches!(status, GoalStatus::Completed | GoalStatus::Cancelled) {
            Self::record_goal_ended_in_tx(&mut tx, &goal, status).await?;
        }
        tx.commit().await?;
        let goal = self.get_goal(id).await?;
        self.publish(Change::GoalUpdated(goal.clone()));
        Ok(goal)
    }

    /// Write the `goal_ended` fact (023): one row for a goal that moves to
    /// `completed` or `cancelled`, inside the same transaction as the status
    /// write. `data` holds its status, its lead time, its task counts and
    /// its workflow; the model and the effort are its orchestrator pin.
    async fn record_goal_ended_in_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        goal: &Goal,
        status: GoalStatus,
    ) -> Result<()> {
        let repo_ids: Vec<String> =
            sqlx::query_scalar("SELECT repository_id FROM goal_repositories WHERE goal_id = ?")
                .bind(&goal.id)
                .fetch_all(&mut **tx)
                .await?;
        let repo_id = match repo_ids.as_slice() {
            [one] => Some(one.clone()),
            _ => None,
        };
        let tasks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE goal_id = ?")
            .bind(&goal.id)
            .fetch_one(&mut **tx)
            .await?;
        let tasks_finished: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tasks WHERE goal_id = ? AND status = 'finished'",
        )
        .bind(&goal.id)
        .fetch_one(&mut **tx)
        .await?;
        let ended_at = now();
        let lead_time_secs = match (
            DateTime::parse_from_rfc3339(&goal.created_at),
            DateTime::parse_from_rfc3339(&ended_at),
        ) {
            (Ok(created), Ok(ended)) => (ended - created).num_seconds().max(0),
            _ => 0,
        };
        let data = serde_json::json!({
            "status": status.as_str(),
            "lead_time_secs": lead_time_secs,
            "tasks": tasks,
            "tasks_finished": tasks_finished,
            "workflow": goal.workflow,
        });
        sqlx::query(
            "INSERT INTO stat_facts (id, kind, created_at, repo_id, goal_id, task_id, session_id,
                                      launch_id, seat, model, effort, skills, data)
             VALUES (?, ?, ?, ?, ?, NULL, NULL, NULL, NULL, ?, ?, '[]', ?)",
        )
        .bind(new_id())
        .bind(GOAL_ENDED)
        .bind(&ended_at)
        .bind(&repo_id)
        .bind(&goal.id)
        .bind(&goal.model)
        .bind(&goal.effort)
        .bind(data.to_string())
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    /// Move a goal's orchestrator onto another model and effort: the pin its
    /// next spawn and resume run on.
    pub async fn set_goal_pin(&self, id: &str, pin: &AgentPin) -> Result<Goal> {
        let (model, effort) = AgentPin::columns(pin);
        let n = sqlx::query("UPDATE goals SET model = ?, effort = ?, updated_at = ? WHERE id = ?")
            .bind(&model)
            .bind(&effort)
            .bind(now())
            .bind(id)
            .execute(self.w())
            .await?
            .rows_affected();
        if n == 0 {
            return Err(not_found("goal", id));
        }
        let goal = self.get_goal(id).await?;
        self.publish(Change::GoalUpdated(goal.clone()));
        Ok(goal)
    }

    /// Announce a goal as it now stands, for a write that changed something
    /// a goal is read with rather than the goal row itself — the usage of a
    /// session under it, which rides in the goal's own fat event.
    pub(crate) async fn publish_goal_update(&self, id: &str) -> Result<()> {
        let goal = self.get_goal(id).await?;
        self.publish(Change::GoalUpdated(goal));
        Ok(())
    }

    /// Hard-delete a goal and, via ON DELETE CASCADE, all its children. The
    /// normal lifecycle uses cancel; deleting drops a finished goal for good,
    /// so nothing is left to refetch and the event carries the id alone.
    pub async fn delete_goal(&self, id: &str) -> Result<()> {
        let n = sqlx::query("DELETE FROM goals WHERE id = ?")
            .bind(id)
            .execute(self.w())
            .await?
            .rows_affected();
        if n == 0 {
            return Err(not_found("goal", id));
        }
        self.publish(Change::GoalDeleted(id.to_string()));
        Ok(())
    }

    /// The repositories a goal works in, as they stand right now: the goal
    /// holds references, not copies. Ordered like
    /// [`Store::list_repositories`].
    pub async fn list_goal_repositories(&self, goal_id: &str) -> Result<Vec<Repository>> {
        let mut repositories = sqlx::query_as::<_, Repository>(
            "SELECT r.* FROM goal_repositories gr
               JOIN repositories r ON r.id = gr.repository_id
              WHERE gr.goal_id = ?
              ORDER BY r.path, r.base_branch",
        )
        .bind(goal_id)
        .fetch_all(self.r())
        .await?;
        self.attach_forges(&mut repositories).await?;
        Ok(repositories)
    }
}
