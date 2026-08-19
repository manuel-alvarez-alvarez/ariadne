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

    /// Give a freshly created profile the prompts of its role, with
    /// `overrides` replacing the default text of the kinds they name.
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

    /// Put one prompt back to the default of the profile's role.
    pub async fn reset_profile_prompt(
        &self,
        profile_id: &str,
        kind: PromptKind,
    ) -> Result<ProfilePrompt> {
        let profile = self.get_profile(profile_id).await?;
        let content = check_kind(&profile, kind)?;
        self.write_prompt(profile_id, kind, content).await
    }

    /// Put the profile's system prompt back to the default of its role.
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
        StoreError::Invalid(format!(
            "prompt {} belongs to a {} profile, not to {whose}",
            kind.as_str(),
            kind.role().as_str(),
        ))
    })
}
