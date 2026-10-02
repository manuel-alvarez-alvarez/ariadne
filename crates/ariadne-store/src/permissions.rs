//! Learned ACP permission choices: every user choice and every denial, one
//! row per repository, tool name, level and normalized input (021, rule 9).
//! A row is written for its own repository, but one widened to scope `all`
//! answers a matching request in any repository that holds none of its own.

use ariadne_core::id::new_id;
use serde_json::Value;

use crate::{Change, LearnedPermission, Result, Store, not_found, now};

#[derive(Debug, Clone)]
pub struct NewLearnedPermission {
    pub repository_id: String,
    pub tool_name: String,
    /// The normalized input the row answers for.
    pub key: String,
    /// `once`, `command` or `family`.
    pub level: String,
    /// The command family, else the tool name.
    pub family: String,
    /// The derived risk tags of the request, in `derive` order.
    pub risk_tags: Vec<String>,
    /// `repository` or `all`.
    pub scope: String,
    /// The ACP `toolCall`. Its `rawInput` is stored with sorted keys.
    pub tool_call: Value,
    pub options: Value,
    pub selected_option: String,
    pub target: String,
    pub output: Option<Value>,
}

impl Store {
    /// The repository's own row for this request's key at this level, when
    /// one exists; else a row of scope `all` from any repository with the
    /// same tool name, level and key.
    pub async fn find_learned_permission(
        &self,
        repository_id: &str,
        tool_name: &str,
        level: &str,
        key: &str,
    ) -> Result<Option<LearnedPermission>> {
        let own: Option<LearnedPermission> = sqlx::query_as(
            "SELECT * FROM learned_permissions
              WHERE repository_id = ? AND tool_name = ? AND level = ? AND key = ?",
        )
        .bind(repository_id)
        .bind(tool_name)
        .bind(level)
        .bind(key)
        .fetch_optional(self.r())
        .await?;
        if own.is_some() {
            return Ok(own);
        }
        Ok(sqlx::query_as(
            "SELECT * FROM learned_permissions
              WHERE scope = 'all' AND tool_name = ? AND level = ? AND key = ?
              ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(tool_name)
        .bind(level)
        .bind(key)
        .fetch_optional(self.r())
        .await?)
    }

    /// Widen or narrow one row to `repository` or `all`, and bump its
    /// `updated_at`.
    pub async fn update_learned_permission_scope(
        &self,
        id: &str,
        scope: &str,
    ) -> Result<LearnedPermission> {
        let ts = now();
        let result =
            sqlx::query("UPDATE learned_permissions SET scope = ?, updated_at = ? WHERE id = ?")
                .bind(scope)
                .bind(&ts)
                .bind(id)
                .execute(self.w())
                .await?;
        if result.rows_affected() == 0 {
            return Err(not_found("learned permission", id));
        }
        let row = self.get_learned_permission(id).await?;
        self.publish(Change::LearnedPermissionUpdated(row.clone()));
        Ok(row)
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

    /// Record one choice. A row with the same repository, tool name, level
    /// and key keeps its id and `created_at`, and takes everything else from
    /// this choice.
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
                (id, repository_id, tool_name, key, level, family, risk_tags, scope,
                 tool_call, options, selected_option, target, output, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (repository_id, tool_name, level, key)
             DO UPDATE SET
                family = excluded.family, risk_tags = excluded.risk_tags,
                scope = excluded.scope,
                tool_call = excluded.tool_call, options = excluded.options,
                selected_option = excluded.selected_option, target = excluded.target,
                output = excluded.output, updated_at = excluded.updated_at
             RETURNING id",
        )
        .bind(&id)
        .bind(&new.repository_id)
        .bind(&new.tool_name)
        .bind(&new.key)
        .bind(&new.level)
        .bind(&new.family)
        .bind(Value::from(new.risk_tags).to_string())
        .bind(&new.scope)
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
