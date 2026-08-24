//! Profile prompt repository.
//!
//! A profile owns its whole prompt set: the system prompt on the profile row
//! and one `profile_prompts` row per [`PromptKind`] of its role. Every prompt
//! is editable and every prompt can be put back to the constant it was seeded
//! from (see [`crate::defaults`]). Editing is free apart from one rule: a
//! briefing may only name the `{placeholder}`s its kind knows how to fill in,
//! since anything else would reach the agent as literal text.

use std::collections::HashMap;
use std::str::FromStr;

use ariadne_core::{PromptKind, Role};

use crate::defaults::{BUILTIN_PROFILES, default_prompt, default_prompts, default_system_prompt};
use crate::{Change, Profile, ProfilePrompt, Result, Store, StoreError, not_found, now};

/// Parse a prompt kind arriving from outside (an HTTP path, a CLI argument)
/// into a store error naming what is accepted.
pub fn parse_prompt_kind(kind: &str) -> Result<PromptKind> {
    PromptKind::from_str(kind).map_err(|_| {
        let known = PromptKind::ALL
            .iter()
            .map(|k| k.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        StoreError::Invalid(format!(
            "unknown prompt kind: {kind} (expected one of {known})"
        ))
    })
}

impl Store {
    /// Seed the built-in profiles and their default prompts into an empty
    /// database.
    ///
    /// Emptiness is the only trigger: once a database has profiles, deleting a
    /// built-in stays deleted and an edited prompt stays edited.
    pub(crate) async fn seed_builtin_profiles(&self) -> Result<()> {
        let mut tx = self.w().begin().await?;
        let profiles: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM profiles")
            .fetch_one(&mut *tx)
            .await?;
        if profiles > 0 {
            return Ok(());
        }
        let ts = now();
        for builtin in &BUILTIN_PROFILES {
            sqlx::query(
                "INSERT INTO profiles (id, name, role, agent_kind, model, system_prompt, created_at, updated_at)
                 VALUES (?, ?, ?, NULL, NULL, ?, ?, ?)",
            )
            .bind(builtin.id)
            .bind(builtin.name)
            .bind(builtin.role.as_str())
            .bind(default_system_prompt(builtin.role))
            .bind(&ts)
            .bind(&ts)
            .execute(&mut *tx)
            .await?;
            Self::insert_prompts(&mut tx, builtin.id, builtin.role, &ts, &HashMap::new()).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Give a freshly created profile the prompts it starts from — its
    /// role's — with `overrides` replacing the default text of the kinds they
    /// name.
    pub(crate) async fn insert_prompts(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        profile_id: &str,
        role: Role,
        ts: &str,
        overrides: &HashMap<PromptKind, String>,
    ) -> Result<()> {
        for (kind, default) in default_prompts(role) {
            let content = overrides.get(&kind).map_or(default, String::as_str);
            sqlx::query(
                "INSERT INTO profile_prompts (profile_id, kind, content, updated_at)
                 VALUES (?, ?, ?, ?)",
            )
            .bind(profile_id)
            .bind(kind.as_str())
            .bind(content)
            .bind(ts)
            .execute(&mut **tx)
            .await?;
        }
        Ok(())
    }

    /// A profile's prompts, in briefing order.
    pub async fn list_profile_prompts(&self, profile_id: &str) -> Result<Vec<ProfilePrompt>> {
        let profile = self.get_profile(profile_id).await?;
        let rows = sqlx::query_as::<_, ProfilePrompt>(
            "SELECT * FROM profile_prompts WHERE profile_id = ?",
        )
        .bind(profile_id)
        .fetch_all(self.r())
        .await?;
        let order = PromptKind::for_role(profile.role());
        let mut rows = rows;
        rows.sort_by_key(|p| {
            order
                .iter()
                .position(|k| k.as_str() == p.kind)
                .unwrap_or(usize::MAX)
        });
        Ok(rows)
    }

    pub async fn get_profile_prompt(
        &self,
        profile_id: &str,
        kind: PromptKind,
    ) -> Result<ProfilePrompt> {
        let profile = self.get_profile(profile_id).await?;
        check_kind(&profile, kind)?;
        self.fetch_prompt(profile_id, kind).await
    }

    /// Replace the text of one prompt.
    ///
    /// A template naming a `{placeholder}` the kind has no value for is
    /// refused here rather than at spawn time: rendering would carry the token
    /// through to the agent as literal text, and the save is the last moment
    /// anyone is looking.
    pub async fn update_profile_prompt(
        &self,
        profile_id: &str,
        kind: PromptKind,
        content: &str,
    ) -> Result<ProfilePrompt> {
        let profile = self.get_profile(profile_id).await?;
        check_kind(&profile, kind)?;
        check_placeholders(kind, content)?;
        self.write_prompt(profile_id, kind, content).await
    }

    /// Put one prompt back to the default this profile starts from.
    pub async fn reset_profile_prompt(
        &self,
        profile_id: &str,
        kind: PromptKind,
    ) -> Result<ProfilePrompt> {
        let profile = self.get_profile(profile_id).await?;
        let content = check_kind(&profile, kind)?;
        self.write_prompt(profile_id, kind, content).await
    }

    /// Put the profile's system prompt back to the default it starts from.
    pub async fn reset_system_prompt(&self, profile_id: &str) -> Result<Profile> {
        let profile = self.get_profile(profile_id).await?;
        sqlx::query("UPDATE profiles SET system_prompt = ?, updated_at = ? WHERE id = ?")
            .bind(default_system_prompt(profile.role()))
            .bind(now())
            .bind(profile_id)
            .execute(self.w())
            .await?;
        let profile = self.get_profile(profile_id).await?;
        self.publish(Change::ProfileUpdated(profile.clone()));
        Ok(profile)
    }

    /// Upsert: a profile created before a kind existed has no row yet, and a
    /// reset must work either way.
    async fn write_prompt(
        &self,
        profile_id: &str,
        kind: PromptKind,
        content: &str,
    ) -> Result<ProfilePrompt> {
        sqlx::query(
            "INSERT INTO profile_prompts (profile_id, kind, content, updated_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT (profile_id, kind) DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at",
        )
        .bind(profile_id)
        .bind(kind.as_str())
        .bind(content)
        .bind(now())
        .execute(self.w())
        .await?;
        self.fetch_prompt(profile_id, kind).await
    }

    async fn fetch_prompt(&self, profile_id: &str, kind: PromptKind) -> Result<ProfilePrompt> {
        sqlx::query_as::<_, ProfilePrompt>(
            "SELECT * FROM profile_prompts WHERE profile_id = ? AND kind = ?",
        )
        .bind(profile_id)
        .bind(kind.as_str())
        .fetch_optional(self.r())
        .await?
        .ok_or_else(|| not_found("profile prompt", &format!("{profile_id}/{}", kind.as_str())))
    }
}

/// The default text of `kind` for this profile, or an error naming the role
/// that owns the kind instead.
fn check_kind(profile: &Profile, kind: PromptKind) -> Result<&'static str> {
    check_role_kind(
        kind,
        profile.role(),
        &format!("{} ({})", profile.name, profile.role),
    )
}

/// Reject a template using a `{placeholder}` its kind cannot fill in, with the
/// offending tokens and the whole allowed set in the message — one save is
/// enough to learn what a briefing may say.
pub(crate) fn check_placeholders(kind: PromptKind, content: &str) -> Result<()> {
    kind.validate_template(content)
        .map_err(|e| StoreError::Invalid(e.to_string()))
}

/// The default text of `kind` for a profile of `role`, or an error naming the
/// role that owns the kind instead. `whose` names the profile in the message.
pub(crate) fn check_role_kind(kind: PromptKind, role: Role, whose: &str) -> Result<&'static str> {
    default_prompt(role, kind).ok_or_else(|| {
        let owners = kind
            .roles()
            .iter()
            .map(|r| r.as_str())
            .collect::<Vec<_>>()
            .join("/");
        StoreError::Invalid(format!(
            "prompt {} belongs to a {owners} profile, not to {whose}",
            kind.as_str(),
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewProfile;
    use crate::defaults::INTEGRATOR_ID;

    /// The engineer's system prompt as it read before migration 0012 — what an
    /// install seeded by an older Ariadne still has in its database, merge step
    /// and all.
    const OLD_ENGINEER_SYSTEM_PROMPT: &str = r##"You own one Ariadne task, from its first commit to its merge. Ariadne coordinates planner, engineer and reviewer agents over shared goals and tasks; you reach it only through the `ariadne` MCP tools: `post_message` to talk to the reviewers, the planner and the user, `list_messages` to read your task's conversation. A message reaches one person in particular when you give `post_message` a `to` — the planner or one of your reviewers, by profile name or by the id `get_task` gives, or "user" to ask the human — and that recipient is woken to read it; with no `to` it waits in the thread for whoever reads it next. Every operation named in backticks here or in your briefings — `request_review`, `get_reviews`, `mark_merged` and the rest — is a tool on that MCP server: invoke it as an MCP tool call, never as a shell command or a message. Work autonomously: do not wait for a human unless a message asks you to. A human may attach to this terminal at any time and type follow-ups.

You work in a dedicated git worktree already checked out on your task branch; the briefing names the branch, its base, the repository and the worktree path. Never switch branches, never touch another worktree, and never touch the primary checkout except for the merge you are told to make. Do not commit generated or unrelated files.

1. Read the task description, its acceptance criteria and the task conversation, for what the planner, the reviewers and the user require; ask rather than guess when something is unclear or blocked.
2. Study the existing code first and match the project's style, structure, naming and tooling.
3. Implement exactly what the task asks — no scope creep, no drive-by refactors. Commit in small steps with clear messages. Make the project's build, tests and linters pass where they exist, and add tests when the task or its conventions call for them.
4. When the work is complete and verified, call the `request_review` MCP tool with a summary: what changed, why, and how you verified it.
5. Reviewers answer with approvals or change requests and you are resumed with their feedback (the `get_reviews` MCP tool has every round). Apply it on the same branch and call `request_review` again; argue with `post_message` when you disagree, never silently ignore a requested change.
6. When you are told to merge, follow those instructions exactly — rebase your branch onto its base, squash it into one commit, fast-forward the base from the primary checkout — then call the `mark_merged` MCP tool with the real commit sha, which the daemon verifies itself. Report it truthfully.
"##;

    /// The integrator's, seeded by migration 0011 as a placeholder: the role
    /// existed, its lifecycle did not.
    const OLD_INTEGRATOR_SYSTEM_PROMPT: &str = r##"You are the integrator of an Ariadne task: once its reviewers have approved it, the task is yours to land on its base branch.

Ariadne coordinates planner, engineer, reviewer and integrator agents over shared goals and tasks; you reach it only through the `ariadne` MCP tools. Nothing starts an integrator session yet, so there is nothing here to do: the playbook that says how a change is landed comes with the lifecycle that runs it.
"##;

    /// The merge instructions as migration 0009 left them: the prompt kind
    /// migration 0012 retires.
    const OLD_MERGE_INSTRUCTIONS: &str = "Your task has been approved. Merge it now.";

    /// Put the database back into the shape an install upgrading to this
    /// release is in: engineers still carrying the merge duty, an integrator
    /// with the placeholder prompt and none of the briefings its role now owns.
    async fn as_an_older_install(store: &Store, engineers: &[&str], integrator: &str) {
        for id in engineers {
            sqlx::query("UPDATE profiles SET system_prompt = ? WHERE id = ?")
                .bind(OLD_ENGINEER_SYSTEM_PROMPT)
                .bind(id)
                .execute(store.w())
                .await
                .unwrap();
            sqlx::query(
                "INSERT INTO profile_prompts (profile_id, kind, content, updated_at)
                 VALUES (?, 'merge_instructions', ?, ?)",
            )
            .bind(id)
            .bind(OLD_MERGE_INSTRUCTIONS)
            .bind(now())
            .execute(store.w())
            .await
            .unwrap();
        }
        sqlx::query("UPDATE profiles SET system_prompt = ? WHERE id = ?")
            .bind(OLD_INTEGRATOR_SYSTEM_PROMPT)
            .bind(integrator)
            .execute(store.w())
            .await
            .unwrap();
        sqlx::query("DELETE FROM profile_prompts WHERE profile_id = ?")
            .bind(integrator)
            .execute(store.w())
            .await
            .unwrap();
    }

    async fn migrate_0012(store: &Store) {
        sqlx::raw_sql(include_str!("../migrations/0012_integrator_prompts.sql"))
            .execute(store.w())
            .await
            .unwrap();
    }

    /// The migrations after it, which rewrite the same rows again: 0015 gives
    /// the integrator's playbook the opening that says which repositories it
    /// is for and the name that says the same, and 0016 merges the three of
    /// them back into one. Replayed together with 0012 because an install
    /// upgrading across all three runs all three, and what it ends up with is
    /// today's default.
    async fn migrate_0015(store: &Store) {
        sqlx::raw_sql(include_str!("../migrations/0015_assigned_integrator.sql"))
            .execute(store.w())
            .await
            .unwrap();
    }

    async fn migrate_0016(store: &Store) {
        sqlx::raw_sql(include_str!("../migrations/0016_one_integrator.sql"))
            .execute(store.w())
            .await
            .unwrap();
    }

    /// Migration 0012 moves the merge duty across the roles of an install that
    /// already exists: the engineer's playbook loses it, the integrator's
    /// placeholder becomes the real one, the briefings its lifecycle needs are
    /// written, and the prompt kind nobody owns any more is dropped.
    ///
    /// What the migrations write is the wording of their own release, not
    /// today's defaults — those have been rewritten since, and reseeding a
    /// database that already exists is its own migration. So what is checked
    /// here is the move itself: the duty left the engineer, and the integrator
    /// ended up with a playbook covering all three ways it lands a task and
    /// with every briefing its role owns.
    #[tokio::test]
    async fn the_migration_hands_the_merge_duty_to_the_integrator() {
        let store = Store::open_in_memory().await.unwrap();
        let engineer = store.get_profile_by_name("Engineer").await.unwrap();
        let integrator = store.get_profile(INTEGRATOR_ID).await.unwrap();
        as_an_older_install(&store, &[&engineer.id], &integrator.id).await;

        migrate_0012(&store).await;
        migrate_0015(&store).await;
        migrate_0016(&store).await;

        let rewritten = store.get_profile(&engineer.id).await.unwrap().system_prompt;
        assert_ne!(
            rewritten, OLD_ENGINEER_SYSTEM_PROMPT,
            "the engineer's playbook was rewritten"
        );
        for merging in ["mark_merged", "git rebase", "--ff-only"] {
            assert!(
                !rewritten.contains(merging),
                "the engineer's playbook no longer ends in a merge: {merging}"
            );
        }
        let landed = store.get_profile(&integrator.id).await.unwrap();
        assert_ne!(
            landed.system_prompt, OLD_INTEGRATOR_SYSTEM_PROMPT,
            "the integrator's placeholder became its playbook"
        );
        for landing in ["github.com remote", "GitLab remote", "git alone"] {
            assert!(
                landed.system_prompt.contains(landing),
                "the integrator's playbook has no {landing}"
            );
        }
        assert_eq!(
            landed.name, "Integrator",
            "and 0015 renamed it beside the two forge ones, 0016 back again"
        );
        // The two briefings the integrator's lifecycle needed at the time;
        // the wake instruction and the message notice it owns today are
        // migration 0021's, replayed nowhere here.
        for kind in [
            PromptKind::IntegrationInstructions,
            PromptKind::IntegrationResume,
        ] {
            let written = store
                .get_profile_prompt(&integrator.id, kind)
                .await
                .unwrap()
                .content;
            assert!(
                !written.trim().is_empty(),
                "the {} briefing was written",
                kind.as_str()
            );
        }
        let left: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM profile_prompts WHERE kind = 'merge_instructions'",
        )
        .fetch_one(store.r())
        .await
        .unwrap();
        assert_eq!(left, 0, "the retired kind is gone from every profile");
    }

    /// And only where nothing was edited: a profile whose system prompt its
    /// user rewrote keeps it, exactly as migration 0009 kept an edited
    /// briefing.
    #[tokio::test]
    async fn the_migration_leaves_an_edited_system_prompt_alone() {
        let store = Store::open_in_memory().await.unwrap();
        let integrator = store.get_profile(INTEGRATOR_ID).await.unwrap();
        let mine = store
            .create_profile(NewProfile {
                name: "Engineer (mine)".into(),
                role: Role::Engineer,
                agent_kind: None,
                model: None,
                system_prompt: "You are an engineer. Merge it yourself.".into(),
                prompts: vec![],
            })
            .await
            .unwrap();
        as_an_older_install(&store, &[], &integrator.id).await;
        // The merge instructions this one had are still dropped: the kind is
        // gone whatever its text said.
        sqlx::query(
            "INSERT INTO profile_prompts (profile_id, kind, content, updated_at)
             VALUES (?, 'merge_instructions', 'just merge it', ?)",
        )
        .bind(&mine.id)
        .bind(now())
        .execute(store.w())
        .await
        .unwrap();

        migrate_0012(&store).await;
        migrate_0015(&store).await;
        migrate_0016(&store).await;

        assert_eq!(
            store.get_profile(&mine.id).await.unwrap().system_prompt,
            "You are an engineer. Merge it yourself."
        );
        let left: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM profile_prompts WHERE profile_id = ? AND kind = 'merge_instructions'",
        )
        .bind(&mine.id)
        .fetch_one(store.r())
        .await
        .unwrap();
        assert_eq!(left, 0);
    }
}
