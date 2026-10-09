//! Workflow repository.
//!
//! A workflow is a linear kanban of columns, each staffed with one agent,
//! that a task walks from its first commit to its landing. Every goal runs on
//! one. Ariadne ships two (`defaults::BUILTIN_WORKFLOWS`) and
//! the user adds and edits their own, the same way a skill is: a built-in is
//! stored with a `NULL` document while it runs on the text Ariadne ships, and
//! a reset is a `NULL` rather than a copy of the default.
//!
//! Goals snapshot their workflow columns. Goals and repository defaults keep
//! a reference to the catalog name, so a workflow in use cannot be deleted.

use ariadne_core::workflow::{self, WorkflowParseError};

use crate::defaults::{BUILTIN_WORKFLOWS, ORCHESTRATION_SKILL, PR_REVIEWER_SKILL};
use crate::query::Filtered;
use crate::{Change, Result, Store, StoreError, Workflow, now};

#[derive(Debug, Clone)]
pub struct NewWorkflow {
    /// Kebab-case; must equal the `workflow <name>` the document opens with.
    pub name: String,
    /// The whole workflow document.
    pub document: String,
}

impl Store {
    /// Seed the shipped workflows, by name, the way [`Store::seed_builtin_skills`]
    /// seeds a skill: a workflow the database lacks is inserted with a NULL
    /// document, no document the database holds is ever touched, and a
    /// built-in the catalog no longer holds is taken back out — adopted as a
    /// skill of the user's own where somebody wrote a document over it, or
    /// dropped outright where nothing did.
    pub(crate) async fn seed_builtin_workflows(&self) -> Result<()> {
        let mut tx = self.w().begin().await?;
        let ts = now();
        for builtin in &BUILTIN_WORKFLOWS {
            sqlx::query(
                "INSERT INTO workflows (name, document, builtin, created_at, updated_at)
                 VALUES (?, NULL, 1, ?, ?)
                 ON CONFLICT (name) DO UPDATE
                 SET builtin = 1, updated_at = excluded.updated_at
                 WHERE workflows.builtin = 0",
            )
            .bind(builtin.name)
            .bind(&ts)
            .bind(&ts)
            .execute(&mut *tx)
            .await?;
        }

        let shipped = vec!["?"; BUILTIN_WORKFLOWS.len()].join(", ");
        let mut adopted = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE workflows SET builtin = 0, updated_at = ?
             WHERE builtin = 1 AND document IS NOT NULL AND name NOT IN ({shipped})"
        )))
        .bind(&ts);
        for builtin in &BUILTIN_WORKFLOWS {
            adopted = adopted.bind(builtin.name);
        }
        adopted.execute(&mut *tx).await?;

        let mut dropped = sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM workflows WHERE builtin = 1 AND document IS NULL AND name NOT IN ({shipped})"
        )));
        for builtin in &BUILTIN_WORKFLOWS {
            dropped = dropped.bind(builtin.name);
        }
        dropped.execute(&mut *tx).await?;

        tx.commit().await?;
        Ok(())
    }

    /// Create a workflow of the user's own. It carries its own document:
    /// nothing ships under its name for it to fall back to.
    pub async fn create_workflow(&self, new: NewWorkflow) -> Result<Workflow> {
        let mut tx = self.w().begin().await?;
        Self::check_workflow_document_in_tx(&mut tx, &new.name, &new.document).await?;
        let ts = now();
        sqlx::query(
            "INSERT INTO workflows (name, document, builtin, created_at, updated_at)
             VALUES (?, ?, 0, ?, ?)",
        )
        .bind(&new.name)
        .bind(&new.document)
        .bind(&ts)
        .bind(&ts)
        .execute(&mut *tx)
        .await
        .map_err(|e| taken(e, &new.name))?;
        tx.commit().await?;
        let workflow = self.get_workflow(&new.name).await?;
        self.publish(Change::WorkflowCreated(workflow.clone()));
        Ok(workflow)
    }

    pub async fn get_workflow(&self, name: &str) -> Result<Workflow> {
        self.fetch_by("workflow", "workflows", "name", name).await
    }

    pub async fn list_workflows(&self) -> Result<Vec<Workflow>> {
        Filtered::new("workflows")
            .fetch(self, " ORDER BY name", &[])
            .await
    }

    /// Write a new document over a workflow's. A built-in keeps its default
    /// behind it, which [`Store::reset_workflow`] goes back to.
    pub async fn set_workflow_document(&self, name: &str, document: &str) -> Result<Workflow> {
        self.get_workflow(name).await?;
        let mut tx = self.w().begin().await?;
        Self::check_workflow_document_in_tx(&mut tx, name, document).await?;
        sqlx::query("UPDATE workflows SET document = ?, updated_at = ? WHERE name = ?")
            .bind(document)
            .bind(now())
            .bind(name)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        let workflow = self.get_workflow(name).await?;
        self.publish(Change::WorkflowUpdated(workflow.clone()));
        Ok(workflow)
    }

    /// Put a built-in back on the text Ariadne ships, by dropping the
    /// document written over it.
    ///
    /// A workflow of the user's own has nothing behind it, so there is
    /// nothing to reset it to and the call is refused rather than emptying
    /// it.
    pub async fn reset_workflow(&self, name: &str) -> Result<Workflow> {
        let workflow = self.get_workflow(name).await?;
        if !workflow.is_builtin() {
            return Err(StoreError::Conflict(format!(
                "workflow {name} is not one Ariadne ships, so it has no default to go back to"
            )));
        }
        sqlx::query("UPDATE workflows SET document = NULL, updated_at = ? WHERE name = ?")
            .bind(now())
            .bind(name)
            .execute(self.w())
            .await?;
        let workflow = self.get_workflow(name).await?;
        self.publish(Change::WorkflowUpdated(workflow.clone()));
        Ok(workflow)
    }

    /// Delete a workflow of the user's own; refused for a built-in, which is
    /// reset rather than removed.
    pub async fn delete_workflow(&self, name: &str) -> Result<()> {
        let workflow = self.get_workflow(name).await?;
        let used: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM goals WHERE workflow = ?1 UNION ALL SELECT 1 FROM repositories WHERE default_workflow = ?1)")
            .bind(name).fetch_one(self.r()).await?;
        if used {
            return Err(StoreError::WorkflowInUse(name.into()));
        }
        if workflow.is_builtin() {
            return Err(StoreError::Conflict(format!(
                "workflow {name} is one Ariadne ships; reset it instead of deleting it"
            )));
        }
        sqlx::query("DELETE FROM workflows WHERE name = ?")
            .bind(name)
            .execute(self.w())
            .await?;
        self.publish(Change::WorkflowDeleted(name.to_string()));
        Ok(())
    }

    /// Parse `document` and check every skill its columns name: it must
    /// answer to a row in the catalog, and it must not be the orchestrator's
    /// skill or the one a reviewer pull request session loads — neither
    /// staffs a task agent, which is what a workflow column does.
    /// `pr-babysit` is allowed: the `pr` column of `develop-review-pr` loads
    /// it.
    ///
    /// Read on `tx`'s own connection, inside the same transaction the save
    /// it clears the way for commits in: the store's one writer serializes
    /// every save and every skill deletion through that single connection,
    /// so nothing deletes a skill this save just found between the check
    /// and the write that acts on it.
    async fn check_workflow_document_in_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        name: &str,
        document: &str,
    ) -> Result<()> {
        let parsed = workflow::parse(document).map_err(invalid)?;
        if parsed.name != name {
            return Err(StoreError::Invalid(format!(
                "the document names the workflow {}, not {name}",
                parsed.name
            )));
        }
        for step in &parsed.steps {
            for skill in &step.skills {
                if skill == ORCHESTRATION_SKILL || skill == PR_REVIEWER_SKILL {
                    return Err(StoreError::Conflict(format!(
                        "skill {skill} cannot staff a workflow column"
                    )));
                }
                let exists: Option<String> =
                    sqlx::query_scalar("SELECT name FROM skills WHERE name = ?")
                        .bind(skill)
                        .fetch_optional(&mut **tx)
                        .await?;
                if exists.is_none() {
                    return Err(StoreError::Conflict(format!("no skill is called {skill}")));
                }
            }
        }
        Ok(())
    }
}

/// A [`WorkflowParseError`], said as invalid input naming the line it failed
/// on.
fn invalid(e: WorkflowParseError) -> StoreError {
    StoreError::Invalid(e.to_string())
}

/// The `PRIMARY KEY (name)` violation, said in the terms the caller used.
fn taken(e: sqlx::Error, name: &str) -> StoreError {
    match e {
        sqlx::Error::Database(ref db) if db.is_unique_violation() => {
            StoreError::Conflict(format!("workflow name already exists: {name}"))
        }
        other => StoreError::Db(other),
    }
}
