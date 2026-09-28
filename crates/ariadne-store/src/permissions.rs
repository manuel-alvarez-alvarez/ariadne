//! Learned ACP permission approvals, scoped to one repository.

use ariadne_core::id::new_id;

use crate::{Change, LearnedPermission, Result, Store, StoreError, not_found, now};

#[derive(Debug, Clone)]
pub struct NewLearnedPermission {
    pub repository_id: String,
    pub tool_name: String,
    pub kind: String,
    pub source: String,
    pub tool_call: Option<serde_json::Value>,
    pub options: Option<serde_json::Value>,
    pub selected_option: Option<String>,
    pub session_id: Option<String>,
    pub task_id: Option<String>,
    pub label: Option<String>,
    pub danger: Option<f64>,
    pub allow_threshold: Option<f64>,
    pub deny_threshold: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct LearnedPermissionUpdate {
    pub tool_name: Option<String>,
    pub kind: Option<String>,
}

impl Store {
    /// Whether this repository has an approval for this ACP tool signature.
    pub async fn has_learned_permission(
        &self,
        repository_id: &str,
        tool_name: &str,
        kind: &str,
    ) -> Result<bool> {
        let found: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM learned_permissions
              WHERE repository_id = ? AND tool_name = ? AND kind = ?",
        )
        .bind(repository_id)
        .bind(tool_name)
        .bind(kind)
        .fetch_optional(self.r())
        .await?;
        Ok(found.is_some())
    }

    pub async fn list_learned_permissions(
        &self,
        repository_id: Option<&str>,
    ) -> Result<Vec<LearnedPermission>> {
        Ok(match repository_id {
            Some(id) => sqlx::query_as("SELECT * FROM learned_permissions WHERE repository_id = ? ORDER BY created_at DESC, id DESC")
                .bind(id).fetch_all(self.r()).await?,
            None => sqlx::query_as("SELECT * FROM learned_permissions ORDER BY created_at DESC, id DESC")
                .fetch_all(self.r()).await?,
        })
    }

    pub async fn get_learned_permission(&self, id: &str) -> Result<LearnedPermission> {
        sqlx::query_as("SELECT * FROM learned_permissions WHERE id = ?")
            .bind(id)
            .fetch_optional(self.r())
            .await?
            .ok_or_else(|| not_found("learned permission", id))
    }

    /// Remember an approval. Repeating the same approval keeps the first row.
    pub async fn learn_permission(&self, new: NewLearnedPermission) -> Result<LearnedPermission> {
        self.insert_learned_permission(new, true).await
    }

    async fn insert_learned_permission(
        &self,
        new: NewLearnedPermission,
        ignore_duplicate: bool,
    ) -> Result<LearnedPermission> {
        let id = new_id();
        let ts = now();
        let statement = if ignore_duplicate {
            "INSERT OR IGNORE INTO learned_permissions
                (id, repository_id, tool_name, kind, source, tool_call, options, selected_option,
                 session_id, task_id, label, danger, allow_threshold, deny_threshold, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        } else {
            "INSERT INTO learned_permissions
                (id, repository_id, tool_name, kind, source, tool_call, options, selected_option,
                 session_id, task_id, label, danger, allow_threshold, deny_threshold, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        };
        let result = sqlx::query(statement)
            .bind(&id)
            .bind(&new.repository_id)
            .bind(&new.tool_name)
            .bind(&new.kind)
            .bind(&new.source)
            .bind(new.tool_call.as_ref().map(serde_json::Value::to_string))
            .bind(new.options.as_ref().map(serde_json::Value::to_string))
            .bind(&new.selected_option)
            .bind(&new.session_id)
            .bind(&new.task_id)
            .bind(&new.label)
            .bind(new.danger)
            .bind(new.allow_threshold)
            .bind(new.deny_threshold)
            .bind(&ts)
            .bind(&ts)
            .execute(self.w())
            .await
            .map_err(|error| match error {
                sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                    StoreError::Conflict("learned permission already exists".into())
                }
                other => StoreError::Db(other),
            })?;
        if result.rows_affected() == 1 {
            let row = self.get_learned_permission(&id).await?;
            self.publish(Change::LearnedPermissionCreated(row.clone()));
            return Ok(row);
        }
        Ok(sqlx::query_as("SELECT * FROM learned_permissions WHERE repository_id = ? AND tool_name = ? AND kind = ?")
            .bind(&new.repository_id).bind(&new.tool_name).bind(&new.kind).fetch_one(self.r()).await?)
    }

    pub async fn create_learned_permission(
        &self,
        new: NewLearnedPermission,
    ) -> Result<LearnedPermission> {
        self.insert_learned_permission(new, false).await
    }

    pub async fn update_learned_permission(
        &self,
        id: &str,
        update: LearnedPermissionUpdate,
    ) -> Result<LearnedPermission> {
        let current = self.get_learned_permission(id).await?;
        let tool_name = update.tool_name.unwrap_or(current.tool_name);
        let kind = update.kind.unwrap_or(current.kind);
        sqlx::query(
            "UPDATE learned_permissions SET tool_name = ?, kind = ?, updated_at = ? WHERE id = ?",
        )
        .bind(tool_name)
        .bind(kind)
        .bind(now())
        .bind(id)
        .execute(self.w())
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                StoreError::Conflict("learned permission already exists".into())
            }
            other => StoreError::Db(other),
        })?;
        let row = self.get_learned_permission(id).await?;
        self.publish(Change::LearnedPermissionUpdated(row.clone()));
        Ok(row)
    }

    pub async fn delete_learned_permission(&self, id: &str) -> Result<LearnedPermission> {
        let row = self.get_learned_permission(id).await?;
        sqlx::query("DELETE FROM learned_permissions WHERE id = ?")
            .bind(id)
            .execute(self.w())
            .await?;
        self.publish(Change::LearnedPermissionDeleted(row.clone()));
        Ok(row)
    }
}
