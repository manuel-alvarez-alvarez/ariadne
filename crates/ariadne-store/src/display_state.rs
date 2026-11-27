//! What an ACP agent last said about its session, kept for display (021
//! rule 24): the session modes, the config options, the agent's title for
//! the session, its cost and the commands it offers.
//!
//! The state belongs to one launch. A report from a launch that the session
//! has replaced changes nothing, and the first report of a new launch starts
//! from an empty state, so no value of the earlier launch is carried into it.
//! Once the session names another launch, the earlier launch's state is read
//! as empty, even where the new launch never reports.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Result, Store};

/// The display state of one session, as its agent last reported it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionDisplayState {
    /// The ACP session modes, where the agent offers them.
    pub mode: Option<SessionModes>,
    /// Each ACP config option, as the JSON the agent sent.
    pub config_options: Vec<Value>,
    /// The agent's own title for the session.
    pub title: Option<String>,
    /// What the session has cost, as the agent counts it.
    pub cost: Option<SessionCost>,
    /// Each command the agent offers, as the JSON the agent sent.
    pub commands: Vec<Value>,
}

/// The mode a session runs in, and every mode the agent offers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionModes {
    pub current_id: Option<String>,
    /// Each ACP session mode, as the JSON the agent sent.
    pub available: Vec<Value>,
}

/// A cost and its ISO 4217 currency code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionCost {
    pub amount: f64,
    pub currency: String,
}

impl Store {
    /// The display state that the session's current launch reported: empty
    /// where that launch reported none.
    pub async fn session_display_state(&self, id: &str) -> Result<SessionDisplayState> {
        let row: Option<(Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT launch_id, display_launch_id, display_state FROM agent_sessions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(self.r())
        .await?;
        let Some((current, written_by, stored)) = row else {
            return Err(crate::not_found("session", id));
        };
        if current.is_none() || current != written_by {
            return Ok(SessionDisplayState::default());
        }
        Ok(stored
            .and_then(|state| serde_json::from_str(&state).ok())
            .unwrap_or_default())
    }

    /// Change the display state as `launch_id` reports it, and answer with
    /// the state that results. `None` where the session runs another launch
    /// now: that report changes nothing.
    pub async fn update_session_display_state(
        &self,
        id: &str,
        launch_id: &str,
        change: impl FnOnce(&mut SessionDisplayState),
    ) -> Result<Option<SessionDisplayState>> {
        let mut tx = self.w().begin().await?;
        let row: Option<(Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT launch_id, display_launch_id, display_state FROM agent_sessions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((current, written_by, stored)) = row else {
            return Err(crate::not_found("session", id));
        };
        if current.as_deref() != Some(launch_id) {
            return Ok(None);
        }
        let mut state = match written_by.as_deref() == Some(launch_id) {
            true => stored
                .and_then(|state| serde_json::from_str(&state).ok())
                .unwrap_or_default(),
            false => SessionDisplayState::default(),
        };
        change(&mut state);
        let json = serde_json::to_string(&state).expect("a display state is JSON");
        sqlx::query(
            "UPDATE agent_sessions SET display_launch_id = ?, display_state = ? WHERE id = ?",
        )
        .bind(launch_id)
        .bind(json)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(state))
    }
}
