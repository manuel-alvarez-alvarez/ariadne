//! Profile prompt repository.
//!
//! A prompt is stored only when someone sets it: a profile answers with the
//! text set on it or, while none is, with the default its kind ships (see
//! [`crate::defaults`]) — which is what a reset goes back to by deleting the
//! row. Editing is free apart from one rule: a briefing may only name the
//! `{placeholder}`s its kind knows how to fill in, since anything else would
//! reach the agent as literal text.

use std::str::FromStr;

use ariadne_core::{PromptKind, Role};

use crate::defaults::{BUILTIN_PROFILES, default_prompt, default_prompt_text};
use crate::{Change, Profile, ProfilePrompt, Result, Store, StoreError, now};

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
    /// Seed the built-in profiles into an empty database, on the defaults of
    /// their roles, so a rewritten default reaches them without any database
    /// being touched.
    ///
    /// Emptiness is the only trigger: once a database has profiles, deleting
    /// a built-in stays deleted and an edited prompt stays edited.
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
                 VALUES (?, ?, ?, NULL, NULL, NULL, ?, ?)",
            )
            .bind(builtin.id)
            .bind(builtin.name)
            .bind(builtin.role.as_str())
            .bind(&ts)
            .bind(&ts)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// A profile's prompts, in briefing order: every kind its role owns, each
    /// one as it takes effect.
    pub async fn list_profile_prompts(&self, profile_id: &str) -> Result<Vec<ProfilePrompt>> {
        let profile = self.get_profile(profile_id).await?;
        let rows = sqlx::query_as::<_, PromptRow>(
            "SELECT kind, content, updated_at FROM profile_prompts WHERE profile_id = ?",
        )
        .bind(profile_id)
        .fetch_all(self.r())
        .await?;
        Ok(PromptKind::for_role(profile.role())
            .iter()
            .map(|kind| match rows.iter().find(|r| r.kind == kind.as_str()) {
                Some(row) => row.clone().into_prompt(profile_id),
                None => default_prompt_of(profile_id, *kind),
            })
            .collect())
    }

    /// One prompt of a profile as it takes effect.
    pub async fn get_profile_prompt(
        &self,
        profile_id: &str,
        kind: PromptKind,
    ) -> Result<ProfilePrompt> {
        let profile = self.get_profile(profile_id).await?;
        check_kind(&profile, kind)?;
        self.fetch_prompt(profile_id, kind).await
    }

    /// Set the text of one prompt, which is what makes it the profile's own.
    ///
    /// A template naming a `{placeholder}` the kind has no value for is
    /// refused here rather than at spawn time: rendering would carry the token
    /// through to the agent as text, and the save is the last moment anyone is
    /// looking.
    pub async fn update_profile_prompt(
        &self,
        profile_id: &str,
        kind: PromptKind,
        content: &str,
    ) -> Result<ProfilePrompt> {
        let profile = self.get_profile(profile_id).await?;
        check_kind(&profile, kind)?;
        check_placeholders(kind, content)?;
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

    /// Put one prompt back on the default of its kind, by dropping the text
    /// set on the profile: what is left is the default itself.
    pub async fn reset_profile_prompt(
        &self,
        profile_id: &str,
        kind: PromptKind,
    ) -> Result<ProfilePrompt> {
        let profile = self.get_profile(profile_id).await?;
        check_kind(&profile, kind)?;
        sqlx::query("DELETE FROM profile_prompts WHERE profile_id = ? AND kind = ?")
            .bind(profile_id)
            .bind(kind.as_str())
            .execute(self.w())
            .await?;
        Ok(default_prompt_of(profile_id, kind))
    }

    /// Put the profile's system prompt back on the default of its role, the
    /// same way: the text set on it goes, the default stands.
    pub async fn reset_system_prompt(&self, profile_id: &str) -> Result<Profile> {
        self.get_profile(profile_id).await?;
        sqlx::query("UPDATE profiles SET system_prompt = NULL, updated_at = ? WHERE id = ?")
            .bind(now())
            .bind(profile_id)
            .execute(self.w())
            .await?;
        let profile = self.get_profile(profile_id).await?;
        self.publish(Change::ProfileUpdated(profile.clone()));
        Ok(profile)
    }

    async fn fetch_prompt(&self, profile_id: &str, kind: PromptKind) -> Result<ProfilePrompt> {
        Ok(sqlx::query_as::<_, PromptRow>(
            "SELECT kind, content, updated_at FROM profile_prompts WHERE profile_id = ? AND kind = ?",
        )
        .bind(profile_id)
        .bind(kind.as_str())
        .fetch_optional(self.r())
        .await?
        .map_or_else(|| default_prompt_of(profile_id, kind), |row| row.into_prompt(profile_id)))
    }
}

/// A stored prompt as the table holds it: the rows that exist are the ones
/// somebody set.
#[derive(Clone, sqlx::FromRow)]
struct PromptRow {
    kind: String,
    content: String,
    updated_at: String,
}

impl PromptRow {
    fn into_prompt(self, profile_id: &str) -> ProfilePrompt {
        ProfilePrompt {
            profile_id: profile_id.to_string(),
            kind: self.kind,
            content: self.content,
            is_default: false,
            updated_at: Some(self.updated_at),
        }
    }
}

/// What a profile with nothing stored for `kind` is briefed with.
fn default_prompt_of(profile_id: &str, kind: PromptKind) -> ProfilePrompt {
    ProfilePrompt {
        profile_id: profile_id.to_string(),
        kind: kind.as_str().to_string(),
        content: default_prompt_text(kind).to_string(),
        is_default: true,
        updated_at: None,
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
