//! Learned ACP permission choices, scoped to one repository: every user
//! choice and every denial, one row per repository, tool name and canonical
//! `rawInput`.

use ariadne_core::id::new_id;
use serde_json::Value;

use crate::{Change, LearnedPermission, Result, Store, not_found, now};

#[derive(Debug, Clone)]
pub struct NewLearnedPermission {
    pub repository_id: String,
    pub tool_name: String,
    /// The ACP `toolCall`. Its `rawInput` is stored with sorted keys.
    pub tool_call: Value,
    pub options: Value,
    pub selected_option: String,
    pub target: String,
    pub output: Option<Value>,
}

impl Store {
    /// The row for this request's key, when one exists.
    pub async fn find_learned_permission(
        &self,
        repository_id: &str,
        tool_name: &str,
        raw_input: &Value,
    ) -> Result<Option<LearnedPermission>> {
        Ok(sqlx::query_as(
            "SELECT * FROM learned_permissions
              WHERE repository_id = ? AND tool_name = ?
                AND ifnull(tool_call -> '$.rawInput', 'null') = json(?)",
        )
        .bind(repository_id)
        .bind(tool_name)
        .bind(canonical(raw_input).to_string())
        .fetch_optional(self.r())
        .await?)
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

    /// Record one choice. A row with the same key keeps its id and
    /// `created_at`, and takes everything else from this choice.
    pub async fn record_learned_permission(
        &self,
        new: NewLearnedPermission,
    ) -> Result<LearnedPermission> {
        let mut tool_call = new.tool_call;
        if let Some(raw_input) = tool_call.get_mut("rawInput") {
            *raw_input = canonical(raw_input);
        }
        let id = new_id();
        let ts = now();
        let stored: String = sqlx::query_scalar(
            "INSERT INTO learned_permissions
                (id, repository_id, tool_name, tool_call, options, selected_option, target,
                 output, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (repository_id, tool_name, ifnull(tool_call -> '$.rawInput', 'null'))
             DO UPDATE SET
                tool_call = excluded.tool_call, options = excluded.options,
                selected_option = excluded.selected_option, target = excluded.target,
                output = excluded.output, updated_at = excluded.updated_at
             RETURNING id",
        )
        .bind(&id)
        .bind(&new.repository_id)
        .bind(&new.tool_name)
        .bind(tool_call.to_string())
        .bind(new.options.to_string())
        .bind(&new.selected_option)
        .bind(&new.target)
        .bind(new.output.as_ref().map(Value::to_string))
        .bind(&ts)
        .bind(&ts)
        .fetch_one(self.w())
        .await?;
        let row = self.get_learned_permission(&stored).await?;
        self.publish(if stored == id {
            Change::LearnedPermissionCreated(row.clone())
        } else {
            Change::LearnedPermissionUpdated(row.clone())
        });
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

/// The value with the keys of every object sorted, whatever order the
/// agent sent them in.
fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key.clone(), canonical(value)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}
