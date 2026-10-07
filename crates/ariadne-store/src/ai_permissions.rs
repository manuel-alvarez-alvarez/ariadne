//! The one AI permission settings row (022): what the user chose, and what the last
//! install made of it.

use crate::{AiPermissionSettings, Result, Store, not_found, now};

/// Every column a write may move; an absent field stays as it was.
///
/// One update for two writers: the HTTP handler, which moves what the user
/// chose, and the installer, which moves what it found. They never write the
/// same column, so neither has to read the other's first.
#[derive(Debug, Clone, Default)]
pub struct AiPermissionSettingsUpdate {
    pub enabled: Option<bool>,
    pub allow_threshold: Option<f64>,
    pub deny_threshold: Option<f64>,
    pub thresholds_hand_set: Option<bool>,
    /// `0.8b`, `4b`, `9b` or `27b`.
    pub flavour: Option<String>,
    /// `mlx`, `cuda` or `cpu`.
    pub device: Option<String>,
    /// `disabled`, `installing`, `ready` or `failed`.
    pub state: Option<String>,
    /// Move `state` only where the row is still enabled; a row turned off
    /// keeps its `disabled`. The install writes its states this way: the model can
    /// be turned off while an install runs, and the check and the write are
    /// one statement, so no turn-off lands between them.
    pub state_while_enabled: bool,
    pub installed_release: Option<Option<String>>,
    pub latest_release: Option<Option<String>>,
    pub weights_present: Option<bool>,
    pub last_refresh_at: Option<Option<String>>,
    pub last_error: Option<Option<String>>,
}

const COLUMNS: &str = "enabled, allow_threshold, deny_threshold, thresholds_hand_set, \
                       flavour, device, state, \
                       installed_release, latest_release, weights_present, \
                       last_refresh_at, last_error, updated_at";

impl Store {
    /// The AI permission settings as they stand. The row is seeded by the migration,
    /// so every database has one.
    pub async fn ai_permission_settings(&self) -> Result<AiPermissionSettings> {
        sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM ai_permission_settings WHERE id = 1"
        )))
        .fetch_optional(self.r())
        .await?
        .ok_or_else(|| not_found("ai permission settings", "1"))
    }

    /// Move whatever `update` names, and answer the row as it now stands.
    pub async fn update_ai_permission_settings(
        &self,
        update: AiPermissionSettingsUpdate,
    ) -> Result<AiPermissionSettings> {
        let mut sets: Vec<&str> = Vec::new();
        if update.enabled.is_some() {
            sets.push("enabled = ?");
        }
        if update.allow_threshold.is_some() {
            sets.push("allow_threshold = ?");
        }
        if update.deny_threshold.is_some() {
            sets.push("deny_threshold = ?");
        }
        if update.thresholds_hand_set.is_some() {
            sets.push("thresholds_hand_set = ?");
        }
        if update.flavour.is_some() {
            sets.push("flavour = ?");
        }
        if update.device.is_some() {
            sets.push("device = ?");
        }
        if update.state.is_some() {
            sets.push(match update.state_while_enabled {
                true => "state = CASE WHEN enabled = 1 THEN ? ELSE state END",
                false => "state = ?",
            });
        }
        if update.installed_release.is_some() {
            sets.push("installed_release = ?");
        }
        if update.latest_release.is_some() {
            sets.push("latest_release = ?");
        }
        if update.weights_present.is_some() {
            sets.push("weights_present = ?");
        }
        if update.last_refresh_at.is_some() {
            sets.push("last_refresh_at = ?");
        }
        if update.last_error.is_some() {
            sets.push("last_error = ?");
        }
        sets.push("updated_at = ?");

        let mut query = sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE ai_permission_settings SET {} WHERE id = 1",
            sets.join(", ")
        )));
        if let Some(enabled) = update.enabled {
            query = query.bind(enabled);
        }
        if let Some(threshold) = update.allow_threshold {
            query = query.bind(threshold);
        }
        if let Some(threshold) = update.deny_threshold {
            query = query.bind(threshold);
        }
        if let Some(hand_set) = update.thresholds_hand_set {
            query = query.bind(hand_set);
        }
        if let Some(flavour) = update.flavour {
            query = query.bind(flavour);
        }
        if let Some(device) = update.device {
            query = query.bind(device);
        }
        if let Some(state) = update.state {
            query = query.bind(state);
        }
        if let Some(release) = update.installed_release {
            query = query.bind(release);
        }
        if let Some(release) = update.latest_release {
            query = query.bind(release);
        }
        if let Some(present) = update.weights_present {
            query = query.bind(present);
        }
        if let Some(at) = update.last_refresh_at {
            query = query.bind(at);
        }
        if let Some(error) = update.last_error {
            query = query.bind(error);
        }
        query.bind(now()).execute(self.w()).await?;

        self.ai_permission_settings().await
    }
}
