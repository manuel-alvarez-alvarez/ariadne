//! Profile repository.

use ariadne_core::id::new_id;
use ariadne_core::{AgentKind, Role};

use crate::{Change, Profile, Result, Store, StoreError, not_found, now};

#[derive(Debug, Clone)]
pub struct NewProfile {
    pub name: String,
    pub role: Role,
    /// None = auto: resolved at spawn time to the first installed agent CLI.
    pub agent_kind: Option<AgentKind>,
    pub model: Option<String>,
    pub system_prompt: String,
    pub extra_flags: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ProfileUpdate {
    pub name: Option<String>,
    /// Some(None) clears back to auto.
    pub agent_kind: Option<Option<AgentKind>>,
    /// Some(None) clears back to the agent default.
    pub model: Option<Option<String>>,
    pub system_prompt: Option<String>,
    pub extra_flags: Option<Vec<String>>,
}

impl Store {
    /// Create a profile, seeded with the default prompts of its role: a new
    /// profile starts from the built-in briefings and is edited from there.
    pub async fn create_profile(&self, new: NewProfile) -> Result<Profile> {
        let id = new_id();
        let ts = now();
        let flags = serde_json::to_string(&new.extra_flags)
            .map_err(|e| StoreError::Invalid(e.to_string()))?;
        let mut tx = self.w().begin().await?;
        sqlx::query(
            "INSERT INTO profiles (id, name, role, agent_kind, model, system_prompt, extra_flags, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&new.name)
        .bind(new.role.as_str())
        .bind(new.agent_kind.map(|k| k.as_str()))
        .bind(&new.model)
        .bind(&new.system_prompt)
        .bind(&flags)
        .bind(&ts)
        .bind(&ts)
        .execute(&mut *tx)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                StoreError::Conflict(format!("profile name already exists: {}", new.name))
            }
            other => StoreError::Db(other),
        })?;
        Self::insert_default_prompts(&mut tx, &id, new.role, &ts).await?;
        tx.commit().await?;
        let profile = self.get_profile(&id).await?;
        self.publish(Change::ProfileCreated(profile.clone()));
        Ok(profile)
    }

    pub async fn get_profile(&self, id: &str) -> Result<Profile> {
        sqlx::query_as::<_, Profile>("SELECT * FROM profiles WHERE id = ?")
            .bind(id)
            .fetch_optional(self.r())
            .await?
            .ok_or_else(|| not_found("profile", id))
    }

    pub async fn get_profile_by_name(&self, name: &str) -> Result<Profile> {
        sqlx::query_as::<_, Profile>("SELECT * FROM profiles WHERE name = ?")
            .bind(name)
            .fetch_optional(self.r())
            .await?
            .ok_or_else(|| not_found("profile", name))
    }

    /// Resolve a profile by id or by unique name (CLI convenience).
    pub async fn resolve_profile(&self, id_or_name: &str) -> Result<Profile> {
        match self.get_profile(id_or_name).await {
            Err(StoreError::NotFound { .. }) => self.get_profile_by_name(id_or_name).await,
            other => other,
        }
    }

    pub async fn list_profiles(&self, role: Option<Role>) -> Result<Vec<Profile>> {
        let rows = match role {
            Some(r) => {
                sqlx::query_as::<_, Profile>("SELECT * FROM profiles WHERE role = ? ORDER BY name")
                    .bind(r.as_str())
                    .fetch_all(self.r())
                    .await?
            }
            None => {
                sqlx::query_as::<_, Profile>("SELECT * FROM profiles ORDER BY name")
                    .fetch_all(self.r())
                    .await?
            }
        };
        Ok(rows)
    }

    pub async fn update_profile(&self, id: &str, update: ProfileUpdate) -> Result<Profile> {
        let current = self.get_profile(id).await?;
        let name = update.name.unwrap_or(current.name);
        let agent_kind = match update.agent_kind {
            Some(k) => k.map(|k| k.as_str().to_string()),
            None => current.agent_kind,
        };
        let model = update.model.unwrap_or(current.model);
        let system_prompt = update.system_prompt.unwrap_or(current.system_prompt);
        let extra_flags = match update.extra_flags {
            Some(f) => serde_json::to_string(&f).map_err(|e| StoreError::Invalid(e.to_string()))?,
            None => current.extra_flags,
        };
        sqlx::query(
            "UPDATE profiles SET name = ?, agent_kind = ?, model = ?, system_prompt = ?, extra_flags = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&name)
        .bind(&agent_kind)
        .bind(&model)
        .bind(&system_prompt)
        .bind(&extra_flags)
        .bind(now())
        .bind(id)
        .execute(self.w())
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                StoreError::Conflict(format!("profile name already exists: {name}"))
            }
            other => StoreError::Db(other),
        })?;
        let profile = self.get_profile(id).await?;
        self.publish(Change::ProfileUpdated(profile.clone()));
        Ok(profile)
    }

    /// Delete a profile; fails with `Conflict` while anything references it.
    ///
    /// The refusal names what holds the profile — a bare count leaves the user
    /// with nowhere to look.
    pub async fn delete_profile(&self, id: &str) -> Result<()> {
        self.get_profile(id).await?;
        let (goals, tasks, reviews, sessions): (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM goals WHERE planner_profile_id = ?1),
                    (SELECT COUNT(*) FROM tasks WHERE engineer_profile_id = ?1),
                    (SELECT COUNT(*) FROM task_reviewers WHERE profile_id = ?1),
                    (SELECT COUNT(*) FROM agent_sessions WHERE profile_id = ?1)",
        )
        .bind(id)
        .fetch_one(self.r())
        .await?;
        let holders = [
            (goals, "goal", "goals"),
            (tasks, "task", "tasks"),
            (reviews, "task as a reviewer", "tasks as a reviewer"),
            (sessions, "agent session", "agent sessions"),
        ];
        let referenced = plural_list(&holders);
        if !referenced.is_empty() {
            return Err(StoreError::Conflict(format!(
                "profile {id} is still used by {referenced}"
            )));
        }
        sqlx::query("DELETE FROM profiles WHERE id = ?")
            .bind(id)
            .execute(self.w())
            .await?;
        self.publish(Change::ProfileDeleted(id.to_string()));
        Ok(())
    }
}

/// "2 goals, 1 agent session": the non-zero holders, counted and named.
fn plural_list(holders: &[(i64, &str, &str)]) -> String {
    holders
        .iter()
        .filter(|(n, _, _)| *n > 0)
        .map(|(n, one, many)| format!("{n} {}", if *n == 1 { one } else { many }))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::plural_list;

    #[test]
    fn only_the_holders_that_exist_are_named() {
        assert_eq!(
            plural_list(&[(2, "goal", "goals"), (0, "task", "tasks"), (1, "x", "xs")]),
            "2 goals, 1 x"
        );
        assert_eq!(plural_list(&[(0, "goal", "goals")]), "");
    }
}
