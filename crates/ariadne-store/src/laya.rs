//! The one Laya settings row (022): what the user chose, and what the last
//! install made of it.

use crate::{LayaSettings, Result, Store, not_found, now};

/// Every column a write may move; an absent field stays as it was.
///
/// One update for two writers: the HTTP handler, which moves what the user
/// chose, and the installer, which moves what it found. They never write the
/// same column, so neither has to read the other's first.
#[derive(Debug, Clone, Default)]
pub struct LayaUpdate {
    pub enabled: Option<bool>,
    /// `english` or `all`.
    pub checkpoints: Option<String>,
    pub threshold: Option<f64>,
    /// `Some(None)` clears the daily refresh; `None` keeps it.
    pub schedule: Option<Option<String>>,
    /// `disabled`, `installing`, `ready` or `failed`.
    pub state: Option<String>,
    /// Move `state` only where the row is still enabled; a row turned off
    /// keeps its `disabled`. The install writes its states this way: Laya can
    /// be turned off while an install runs, and the check and the write are
    /// one statement, so no turn-off lands between them.
    pub state_while_enabled: bool,
    pub installed_release: Option<Option<String>>,
    pub latest_release: Option<Option<String>>,
    pub weights_present: Option<bool>,
    pub last_refresh_at: Option<Option<String>>,
    pub last_error: Option<Option<String>>,
}

const COLUMNS: &str = "enabled, checkpoints, threshold, schedule, state, \
                       installed_release, latest_release, weights_present, \
                       last_refresh_at, last_error, updated_at";

impl Store {
    /// The Laya settings as they stand. The row is seeded by the migration,
    /// so every database has one.
    pub async fn laya_settings(&self) -> Result<LayaSettings> {
        sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMNS} FROM laya_settings WHERE id = 1"
        )))
        .fetch_optional(self.r())
        .await?
        .ok_or_else(|| not_found("laya settings", "1"))
    }

    /// Move whatever `update` names, and answer the row as it now stands.
    pub async fn update_laya_settings(&self, update: LayaUpdate) -> Result<LayaSettings> {
        let mut sets: Vec<&str> = Vec::new();
        if update.enabled.is_some() {
            sets.push("enabled = ?");
        }
        if update.checkpoints.is_some() {
            sets.push("checkpoints = ?");
        }
        if update.threshold.is_some() {
            sets.push("threshold = ?");
        }
        if update.schedule.is_some() {
            sets.push("schedule = ?");
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
            "UPDATE laya_settings SET {} WHERE id = 1",
            sets.join(", ")
        )));
        if let Some(enabled) = update.enabled {
            query = query.bind(enabled);
        }
        if let Some(checkpoints) = update.checkpoints {
            query = query.bind(checkpoints);
        }
        if let Some(threshold) = update.threshold {
            query = query.bind(threshold);
        }
        if let Some(schedule) = update.schedule {
            query = query.bind(schedule);
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

        self.laya_settings().await
    }
}
