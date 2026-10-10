use std::collections::HashSet;

use ariadne_core::models::{ModelRank, agent_of};
use ariadne_core::{AttentionReason, Seat, SessionStatus};
use ariadne_store::{AgentSession, NewAgentEvent, Result, SessionFilter, Store};
use tracing::{info, warn};

use crate::attention;
use crate::launcher::Launcher;

use super::SPAWN_RETRY_BUDGET;

/// Whether an exhausted session has no automatic recovery left to try: the
/// same conditions [`Scheduler::auto_switch_exhausted`] gives up on, read as
/// a standing fact rather than acted on. What the "Needs attention" recovery
/// producer asks before raising a quota item — a session already switched,
/// or one still mid-switch, is not its business, only one automatic
/// recovery has spent its whole budget on.
///
/// Unlike the sweep, this propagates a store error rather than quietly
/// skipping: a producer that cannot tell a genuine exhaustion from a store
/// that would not answer must say so, not guess "no recovery left" from a
/// read that simply failed.
pub(crate) async fn recovery_exhausted(
    store: &Store,
    launcher: &Launcher,
    session: &AgentSession,
) -> Result<bool> {
    if store.switched_successor(&session.id).await?.is_some() {
        // A successor is already running this session's work: recovery
        // already acted, and the old row is not the one left to answer.
        return Ok(false);
    }
    if !launcher.cfg.auto_switch {
        return Ok(true);
    }
    let (used, switches) = switch_chain(store, session).await?;
    if switches >= SPAWN_RETRY_BUDGET {
        return Ok(true);
    }
    Ok(switch_target(store, launcher, session, &used)
        .await?
        .is_none())
}

#[derive(Clone)]
pub(super) struct ExhaustedNotice {
    goal_id: String,
    task_id: String,
    old_model: String,
    new_model: String,
}

impl super::Scheduler {
    pub(super) async fn exhausted_sweep(&mut self) {
        let Ok(sessions) = self
            .store
            .list_sessions(SessionFilter {
                attention_only: true,
                ..Default::default()
            })
            .await
        else {
            return;
        };
        for session in sessions
            .into_iter()
            .filter(|session| session.attention_reason() == Some(AttentionReason::Exhausted))
        {
            self.auto_switch_exhausted(&session).await;
        }
    }

    pub(super) async fn auto_switch_exhausted(&mut self, session: &AgentSession) {
        if session.status() != SessionStatus::Exited {
            return;
        }
        if session.attention_reason() != Some(AttentionReason::Exhausted)
            || !attention::work_is_active(&self.store, session).await
        {
            return;
        }
        if self
            .store
            .switched_successor(&session.id)
            .await
            .ok()
            .flatten()
            .is_some()
        {
            return;
        }
        if !self.launcher.cfg.auto_switch {
            return;
        }
        // A store that would not answer is waited out for the next sweep
        // (009 rule 26): neither call spends a switch attempt on it.
        let Ok((used, switches)) = switch_chain(&self.store, session).await else {
            return;
        };
        if switches >= SPAWN_RETRY_BUDGET {
            return;
        }
        let Ok(Some(model)) = switch_target(&self.store, &self.launcher, session, &used).await
        else {
            return;
        };
        match self
            .launcher
            .switch_session(&session.id, &model, None, "exhausted")
            .await
        {
            Ok(next) => {
                if next.id == session.id {
                    let _ = self
                        .store
                        .create_event(NewAgentEvent {
                            session_id: Some(session.id.clone()),
                            task_id: session.task_id.clone(),
                            kind: "session.auto_switch".into(),
                            payload: serde_json::json!({"model": session.model}),
                        })
                        .await;
                    let _ = self.store.clear_session_attention(&session.id).await;
                }
                info!(session = %session.id, to = %next.id, model, "switched an exhausted session");
                if let (Some(goal_id), Some(task_id), Some(Seat::Agent)) =
                    (&session.goal_id, &session.task_id, session.seat())
                {
                    self.exhausted_notices.insert(
                        format!("{}:{switches}", session.id),
                        ExhaustedNotice {
                            goal_id: goal_id.clone(),
                            task_id: task_id.clone(),
                            old_model: session.model.clone(),
                            new_model: model,
                        },
                    );
                    self.tell_exhausted_notices().await;
                }
            }
            Err(error) => {
                warn!(session = %session.id, error = %format!("{error:#}"), "switching an exhausted session failed");
            }
        }
    }

    pub(super) async fn tell_exhausted_notices(&mut self) {
        for (old_session, notice) in self.exhausted_notices.clone() {
            let Ok(orchestrators) = self
                .live_sessions(&notice.goal_id, None, Seat::Orchestrator)
                .await
            else {
                continue;
            };
            let Some(orchestrator) = orchestrators.first() else {
                continue;
            };
            let text = format!(
                "Task {} switched from model {} to {} because the old model was exhausted.",
                notice.task_id, notice.old_model, notice.new_model
            );
            if self.hand_prompt(orchestrator, text) {
                self.exhausted_notices.remove(&old_session);
            }
        }
    }
}

/// Every model this session's switch chain has already stood on, and how
/// many of its links were spent on an exhaustion — read from the chain's own
/// `session.switched`/`session.auto_switch` events rather than kept apart,
/// so nothing here can drift from what a switch actually recorded.
///
/// A store error on the chain's own events propagates; the walk up
/// `switched_from` stops early on one instead, the same as a chain that
/// simply ended, since a broken link here is not this function's to
/// diagnose.
async fn switch_chain(store: &Store, session: &AgentSession) -> Result<(HashSet<String>, u32)> {
    let mut used = HashSet::new();
    let mut switches = 0;
    let mut current = Some(session.clone());
    while let Some(row) = current {
        used.insert(row.model.clone());
        let events = store.list_session_events(&row.id).await?;
        for model in events.iter().filter_map(|event| {
            (event.kind == "session.auto_switch")
                .then(|| serde_json::from_str::<serde_json::Value>(&event.payload).ok())
                .flatten()
                .and_then(|payload| {
                    payload
                        .get("model")
                        .and_then(|model| model.as_str())
                        .map(str::to_string)
                })
        }) {
            used.insert(model);
        }
        switches += events
            .iter()
            .filter(|event| {
                event.kind == "session.switched"
                    && serde_json::from_str::<serde_json::Value>(&event.payload)
                        .ok()
                        .and_then(|payload| payload.get("reason").cloned())
                        .and_then(|reason| reason.as_str().map(str::to_string))
                        .as_deref()
                        == Some("exhausted")
            })
            .count() as u32;
        current = match row.switched_from.as_deref() {
            Some(id) => store.get_session(id).await.ok(),
            None => None,
        };
    }
    Ok((used, switches))
}

/// A store error on the catalog's own ranks or disabled list propagates; an
/// unranked model, or one ranked `local`, legitimately has no target and
/// answers `Ok(None)` rather than an error — the ranks read worked, the
/// model simply is not in it.
async fn switch_target(
    store: &Store,
    launcher: &Launcher,
    session: &AgentSession,
    used: &HashSet<String>,
) -> Result<Option<String>> {
    let ranks = store.model_ranks().await?;
    let Some(&rank) = ranks.get(&session.model) else {
        return Ok(None);
    };
    if rank == ModelRank::Local {
        return Ok(None);
    }
    let disabled = store.disabled_models().await?;
    let mut models = launcher.registry.models().await;
    models.retain(|model| !disabled.contains(&model.id) && !used.contains(&model.id));
    let own_agent = agent_of(&session.model);
    let mut rank_order = vec![rank];
    match rank {
        ModelRank::Fast => rank_order.push(ModelRank::Balanced),
        ModelRank::Balanced => {
            rank_order.push(ModelRank::Frontier);
            rank_order.push(ModelRank::Fast);
        }
        ModelRank::Frontier => rank_order.push(ModelRank::Balanced),
        ModelRank::Local => return Ok(None),
    }
    for wanted_rank in rank_order {
        for same_agent in [false, true] {
            if let Some(model) = models.iter().find(|model| {
                ranks.get(&model.id) == Some(&wanted_rank)
                    && (model.agent_id == own_agent) == same_agent
            }) {
                return Ok(Some(model.id.clone()));
            }
        }
    }
    Ok(None)
}
