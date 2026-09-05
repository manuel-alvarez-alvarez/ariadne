//! Skill repository.
//!
//! A skill is one document that tells a generic agent how to do one kind of
//! work. Ariadne ships a catalog of them (`defaults::BUILTIN_SKILLS`) and the
//! user adds and edits their own; between them they are the whole of what an
//! agent below the orchestrator can be.
//!
//! A built-in is stored with a `NULL` document while it runs on the text
//! Ariadne ships, so a reworded skill reaches every database without a
//! migration, and a reset is a `NULL` rather than a copy of the default.

use crate::defaults::BUILTIN_SKILLS;
use crate::query::Filtered;
use crate::{Change, Result, Skill, Store, StoreError, now};

#[derive(Debug, Clone)]
pub struct NewSkill {
    /// Kebab-case; how an agent loads the skill and how a task names it.
    pub name: String,
    /// The whole `SKILL.md`, frontmatter included.
    pub document: String,
}

impl Store {
    /// Seed the shipped skills into an empty database, each with a NULL
    /// document, so a rewritten default reaches them without a row being
    /// touched.
    ///
    /// Emptiness is the only trigger: once a database has skills, a deleted
    /// built-in stays deleted and an edited document stays edited.
    pub(crate) async fn seed_builtin_skills(&self) -> Result<()> {
        let mut tx = self.w().begin().await?;
        let skills: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM skills")
            .fetch_one(&mut *tx)
            .await?;
        if skills > 0 {
            return Ok(());
        }
        let ts = now();
        for builtin in &BUILTIN_SKILLS {
            sqlx::query(
                "INSERT INTO skills (name, document, builtin, created_at, updated_at)
                 VALUES (?, NULL, 1, ?, ?)",
            )
            .bind(builtin.name)
            .bind(&ts)
            .bind(&ts)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Create a skill of the user's own. It carries its own document: nothing
    /// ships under its name for it to fall back to.
    pub async fn create_skill(&self, new: NewSkill) -> Result<Skill> {
        let ts = now();
        sqlx::query(
            "INSERT INTO skills (name, document, builtin, created_at, updated_at)
             VALUES (?, ?, 0, ?, ?)",
        )
        .bind(&new.name)
        .bind(&new.document)
        .bind(&ts)
        .bind(&ts)
        .execute(self.w())
        .await
        .map_err(|e| taken(e, &new.name))?;
        let skill = self.get_skill(&new.name).await?;
        self.publish(Change::SkillCreated(skill.clone()));
        Ok(skill)
    }

    pub async fn get_skill(&self, name: &str) -> Result<Skill> {
        self.fetch_by("skill", "skills", "name", name).await
    }

    pub async fn list_skills(&self) -> Result<Vec<Skill>> {
        Filtered::new("skills")
            .fetch(self, " ORDER BY name", &[])
            .await
    }

    /// Write a new document over a skill's. A built-in keeps its default
    /// behind it, which [`Store::reset_skill`] goes back to.
    pub async fn set_skill_document(&self, name: &str, document: &str) -> Result<Skill> {
        self.get_skill(name).await?;
        sqlx::query("UPDATE skills SET document = ?, updated_at = ? WHERE name = ?")
            .bind(document)
            .bind(now())
            .bind(name)
            .execute(self.w())
            .await?;
        let skill = self.get_skill(name).await?;
        self.publish(Change::SkillUpdated(skill.clone()));
        Ok(skill)
    }

    /// Put a built-in back on the text Ariadne ships, by dropping the document
    /// written over it: what is left is the shipped one itself.
    ///
    /// A skill of the user's own has nothing behind it, so there is nothing to
    /// reset it to and the call is refused rather than emptying it.
    pub async fn reset_skill(&self, name: &str) -> Result<Skill> {
        let skill = self.get_skill(name).await?;
        if !skill.is_builtin() {
            return Err(StoreError::Conflict(format!(
                "skill {name} is not one Ariadne ships, so it has no default to go back to"
            )));
        }
        sqlx::query("UPDATE skills SET document = NULL, updated_at = ? WHERE name = ?")
            .bind(now())
            .bind(name)
            .execute(self.w())
            .await?;
        let skill = self.get_skill(name).await?;
        self.publish(Change::SkillUpdated(skill.clone()));
        Ok(skill)
    }

    /// Delete a skill; refused while any staffed agent still loads it, and
    /// refused for a built-in, which is reset rather than removed.
    pub async fn delete_skill(&self, name: &str) -> Result<()> {
        let skill = self.get_skill(name).await?;
        if skill.is_builtin() {
            return Err(StoreError::Conflict(format!(
                "skill {name} is one Ariadne ships; reset it instead of deleting it"
            )));
        }
        let agents: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM task_agent_skills WHERE skill_name = ?")
                .bind(name)
                .fetch_one(self.r())
                .await?;
        if agents > 0 {
            let plural = if agents == 1 { "agent" } else { "agents" };
            return Err(StoreError::Conflict(format!(
                "skill {name} is still loaded by {agents} {plural}"
            )));
        }
        sqlx::query("DELETE FROM skills WHERE name = ?")
            .bind(name)
            .execute(self.w())
            .await?;
        self.publish(Change::SkillDeleted(name.to_string()));
        Ok(())
    }
}

/// The `PRIMARY KEY (name)` violation, said in the terms the caller used.
fn taken(e: sqlx::Error, name: &str) -> StoreError {
    match e {
        sqlx::Error::Database(ref db) if db.is_unique_violation() => {
            StoreError::Conflict(format!("skill name already exists: {name}"))
        }
        other => StoreError::Db(other),
    }
}

/// "2 goals, 1 agent session": the non-zero holders, counted and named. What
/// a repository's delete refusal is built from.
pub(crate) fn plural_list(holders: &[(i64, &str, &str)]) -> String {
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
