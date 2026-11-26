//! Source references and disposable worker progress. No execution rows live here.
use ariadne_core::{GoalStatus, id::new_id};
use serde_json::Value;

use crate::{Change, Result, Store, StoreError, Task, now};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SourceTaskReference {
    pub id: String,
    pub goal_id: String,
    pub repo_id: String,
    pub host: String,
    pub worker_id: String,
    pub request: Option<String>,
    pub cache: String,
    pub stale: i64,
    pub cancel_requested: i64,
    pub retry_requested: i64,
    pub dependency_failure: Option<String>,
    pub failure_operation_id: Option<String>,
    pub failure_revision: Option<i64>,
    pub authorization_revision: Option<i64>,
    pub progress_id: Option<String>,
    pub progress_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Store {
    pub async fn source_task_reference(&self, id: &str) -> Result<Option<SourceTaskReference>> {
        Ok(
            sqlx::query_as("SELECT * FROM source_task_references WHERE id = ?")
                .bind(id)
                .fetch_optional(self.r())
                .await?,
        )
    }

    pub async fn source_task_references(&self, host: &str) -> Result<Vec<SourceTaskReference>> {
        Ok(
            sqlx::query_as("SELECT * FROM source_task_references WHERE host = ? ORDER BY id")
                .bind(host)
                .fetch_all(self.r())
                .await?,
        )
    }

    /// Persist the stable identity before the first network write.
    pub async fn reserve_source_task(
        &self,
        reference: SourceTaskReference,
        dependencies: &[String],
    ) -> Result<()> {
        let mut tx = self.w().begin().await?;
        if !reference.goal_id.is_empty() {
            let status: String = sqlx::query_scalar("SELECT status FROM goals WHERE id = ?")
                .bind(&reference.goal_id)
                .fetch_one(&mut *tx)
                .await?;
            if ![GoalStatus::Planning.as_str(), GoalStatus::Active.as_str()]
                .contains(&status.as_str())
            {
                return Err(StoreError::Conflict("the goal takes no new tasks".into()));
            }
        }
        sqlx::query("INSERT INTO source_task_references (id, goal_id, repo_id, host, worker_id, request, cache, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&reference.id).bind(&reference.goal_id).bind(&reference.repo_id).bind(&reference.host)
            .bind(&reference.worker_id).bind(&reference.request).bind(&reference.cache)
            .bind(&reference.created_at).bind(&reference.updated_at).execute(&mut *tx).await?;
        if !reference.goal_id.is_empty() {
            Self::insert_dependencies(&mut tx, &reference.goal_id, &reference.id, dependencies)
                .await?;
        } else if !dependencies.is_empty() {
            return Err(StoreError::Invalid(
                "a goal-less task takes no dependencies".into(),
            ));
        }
        tx.commit().await?;
        if !reference.goal_id.is_empty() {
            self.publish_goal_update(&reference.goal_id).await?;
        }
        Ok(())
    }

    /// Cache only an answer from the persistent execution owner.
    pub async fn cache_source_task(&self, id: &str, worker_id: &str, cache: &Value) -> Result<()> {
        let source_id = self.daemon_id().await?;
        let mut tx = self.w().begin().await?;
        let reference: SourceTaskReference =
            sqlx::query_as("SELECT * FROM source_task_references WHERE id=?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| crate::not_found("source task reference", id))?;
        if reference.worker_id != worker_id
            || cache["id"].as_str() != Some(id)
            || cache["goal_id"].as_str()
                != (!reference.goal_id.is_empty()).then_some(reference.goal_id.as_str())
            || cache["execution_context"]["execution_owner"].as_str() != Some(worker_id)
        {
            return Err(StoreError::Conflict(
                "worker identity or task identity changed; restore the original worker".into(),
            ));
        }
        if cache["execution_context"]["source_goal"]["daemon_id"]
            .as_str()
            .or_else(|| cache["execution_context"]["source_daemon_id"].as_str())
            != Some(source_id.as_str())
        {
            return Err(StoreError::Conflict(
                "worker response belongs to another source daemon".into(),
            ));
        }
        let old: Value = serde_json::from_str(&reference.cache)
            .map_err(|e| StoreError::Invalid(e.to_string()))?;
        // A buffered event can precede a newer reconnection read or control response.
        if reference.request.is_none()
            && old["updated_at"]
                .as_str()
                .zip(cache["updated_at"].as_str())
                .is_some_and(|(old, new)| old > new)
            || old["execution_context"]["revision"]
                .as_i64()
                .zip(cache["execution_context"]["revision"].as_i64())
                .is_some_and(|(old, new)| old > new)
            || matches!(old["status"].as_str(), Some("finished" | "cancelled"))
                && old["status"] != cache["status"]
        {
            return Ok(());
        }
        if old == *cache && reference.stale == 0 && reference.request.is_none() {
            return Ok(());
        }
        let changed = old["status"] != cache["status"];
        let progress_id = if changed {
            Some(new_id())
        } else {
            reference.progress_id
        };
        let progress_at = if changed {
            cache["updated_at"].as_str().map(str::to_owned)
        } else {
            reference.progress_at
        };
        let status = cache["status"].as_str();
        let resolved = matches!(status, Some("finished" | "cancelled"))
            || status == Some("pending") && reference.retry_requested != 0;
        let failure = if resolved || status == Some("failed") {
            None
        } else {
            reference.dependency_failure
        };
        let operation = if resolved {
            None
        } else {
            reference.failure_operation_id
        };
        let failure_revision = if resolved {
            None
        } else {
            reference.failure_revision
        };
        sqlx::query("UPDATE source_task_references SET cache=?,request=NULL,stale=0,updated_at=?,progress_id=?,progress_at=?,retry_requested=?,dependency_failure=?,failure_operation_id=?,failure_revision=? WHERE id=?")
            .bind(cache.to_string()).bind(now()).bind(progress_id).bind(progress_at)
            .bind(if status == Some("failed") { reference.retry_requested } else { 0 })
            .bind(failure).bind(operation).bind(failure_revision).bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
        let task = self.get_task(id).await?;
        if reference.request.is_some() {
            self.publish(Change::TaskCreated(task));
        } else {
            self.publish(Change::TaskUpdated {
                task,
                transition: None,
            });
        }
        if self.get_goal(&reference.goal_id).await.is_ok() {
            self.publish_goal_update(&reference.goal_id).await?;
        }
        Ok(())
    }

    pub async fn request_source_cancel(&self, id: &str) -> Result<Task> {
        let task = self.get_task(id).await?;
        if task.status().is_terminal() {
            return Ok(task);
        }
        sqlx::query("UPDATE source_task_references SET cancel_requested = 1 WHERE id = ?")
            .bind(id)
            .execute(self.w())
            .await?;
        let task = self.get_task(id).await?;
        self.publish(Change::TaskUpdated {
            task: task.clone(),
            transition: None,
        });
        Ok(task)
    }

    pub async fn request_source_retry(&self, id: &str) -> Result<Task> {
        let mut tx = self.w().begin().await?;
        let eligible: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM source_task_references r LEFT JOIN goals g ON g.id=r.goal_id WHERE r.id=? AND r.cancel_requested=0 AND (r.goal_id='' OR g.status='active') AND json_extract(r.cache,'$.status')='failed')")
            .bind(id).fetch_one(&mut *tx).await?;
        if !eligible {
            return Err(StoreError::Conflict(
                "only a failed task in an active source goal can retry".into(),
            ));
        }
        sqlx::query("UPDATE source_task_references SET retry_requested=1 WHERE id=?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        let task = self.get_task(id).await?;
        self.publish(Change::TaskUpdated {
            task: task.clone(),
            transition: None,
        });
        Ok(task)
    }

    pub async fn source_tasks_unreachable(&self, host: &str, stale: bool) -> Result<()> {
        let ids: Vec<String> = sqlx::query_scalar("UPDATE source_task_references SET stale = ? WHERE host = ? AND stale <> ? RETURNING id")
            .bind(i64::from(stale)).bind(host).bind(i64::from(stale)).fetch_all(self.w()).await?;
        for id in ids {
            self.publish(Change::TaskUpdated {
                task: self.get_task(&id).await?,
                transition: None,
            });
        }
        Ok(())
    }

    /// Claim authorization after checking source controls in one write transaction.
    pub async fn source_authorization(&self, id: &str, revision: i64) -> Result<bool> {
        let mut tx = self.w().begin().await?;
        let eligible: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM source_task_references r LEFT JOIN goals g ON g.id=r.goal_id WHERE r.id=? AND r.cancel_requested=0 AND r.dependency_failure IS NULL AND r.request IS NULL AND (r.goal_id='' OR g.status='active') AND NOT EXISTS(SELECT 1 FROM task_dependencies d JOIN task_index t ON t.id=d.depends_on_task_id WHERE d.task_id=r.id AND t.status<>'finished'))")
            .bind(id).fetch_one(&mut *tx).await?;
        if eligible {
            sqlx::query("UPDATE source_task_references SET authorization_revision=? WHERE id=?")
                .bind(revision)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(eligible)
    }

    /// Queue a dependency failure without inventing a worker transition.
    pub async fn fail_source_dependency(&self, id: &str, reason: &str) -> Result<()> {
        let n = sqlx::query("UPDATE source_task_references SET dependency_failure=?,failure_operation_id=?,failure_revision=json_extract(cache,'$.execution_context.revision') WHERE id=? AND dependency_failure IS NULL AND authorization_revision IS NULL AND json_extract(cache,'$.status')='pending' AND json_extract(cache,'$.execution_context.revision') IS NOT NULL AND EXISTS(SELECT 1 FROM task_dependencies d JOIN task_index t ON t.id=d.depends_on_task_id WHERE d.task_id=source_task_references.id AND t.status IN ('failed','cancelled'))")
            .bind(reason).bind(new_id()).bind(id).execute(self.w()).await?.rows_affected();
        if n != 0 {
            self.publish(Change::TaskUpdated {
                task: self.get_task(id).await?,
                transition: None,
            });
        }
        Ok(())
    }

    pub async fn clear_source_dependency_request(&self, id: &str, operation: &str) -> Result<()> {
        sqlx::query("UPDATE source_task_references SET dependency_failure=NULL,failure_operation_id=NULL,failure_revision=NULL WHERE id=? AND failure_operation_id=?")
            .bind(id).bind(operation).execute(self.w()).await?;
        self.publish(Change::TaskUpdated {
            task: self.get_task(id).await?,
            transition: None,
        });
        Ok(())
    }

    pub async fn remove_source_task_reference(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM source_task_references WHERE id = ?")
            .bind(id)
            .execute(self.w())
            .await?;
        Ok(())
    }
}

impl SourceTaskReference {
    pub(crate) fn cached_agents(&self) -> Result<Vec<crate::TaskAgent>> {
        let cache: Value =
            serde_json::from_str(&self.cache).map_err(|e| StoreError::Invalid(e.to_string()))?;
        Ok(cache["agents"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(ordinal, a)| crate::TaskAgent {
                id: a["id"].as_str().unwrap_or_default().into(),
                task_id: self.id.clone(),
                ordinal: ordinal as i64,
                step: a["step"].as_str().unwrap_or_default().into(),
                model: a["model"].as_str().unwrap_or_default().into(),
                effort: a["effort"].as_str().map(str::to_owned),
                brief: a["brief"].as_str().map(str::to_owned),
            })
            .collect())
    }
}
