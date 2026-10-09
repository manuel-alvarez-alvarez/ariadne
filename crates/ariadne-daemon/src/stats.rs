//! The facts the daemon writes to the stats ledger (023), and the one helper
//! that fills a fact in from the session it is about.

use chrono::DateTime;
use serde_json::json;
use tracing::warn;

use ariadne_store::{AgentSession, NewStatFact, Result, Store};

/// The fact one run of a session writes as it ends.
const SESSION_ENDED: &str = "session_ended";

/// The fact a session writes as it leaves one model for another.
const SWITCH: &str = "switch";

/// A fact of `kind` about one session, carrying `data`: the repository, goal,
/// task, launch, seat, model and effort are the session's own, and the skills
/// are the ones its staffed agent loads.
///
/// The repository is the task's, or the goal's where the goal works in one
/// repository alone. A loose session, and an orchestrator over several
/// repositories, name none.
pub async fn session_fact(
    store: &Store,
    session_id: &str,
    kind: &str,
    data: serde_json::Value,
) -> Result<NewStatFact> {
    let session = store.get_session(session_id).await?;
    fact_of(store, session, kind, data).await
}

/// [`session_fact`] off a session row the caller already holds.
async fn fact_of(
    store: &Store,
    session: AgentSession,
    kind: &str,
    mut data: serde_json::Value,
) -> Result<NewStatFact> {
    if session.seat() == Some(ariadne_core::Seat::Agent)
        && let Some(id) = &session.task_agent_id
    {
        data["step"] = serde_json::json!(store.get_task_agent(id).await?.step);
    }
    let repo_id = match (&session.task_id, &session.goal_id) {
        (Some(task_id), _) => Some(store.get_task(task_id).await?.repo_id),
        (None, Some(goal_id)) => match store.list_goal_repositories(goal_id).await?.as_slice() {
            [only] => Some(only.id.clone()),
            _ => None,
        },
        (None, None) => None,
    };
    let skills = match &session.task_agent_id {
        Some(agent_id) => store
            .agent_skills(agent_id)
            .await?
            .into_iter()
            .map(|skill| skill.name)
            .collect(),
        None => Vec::new(),
    };
    Ok(NewStatFact {
        kind: kind.to_string(),
        repo_id,
        goal_id: session.goal_id,
        task_id: session.task_id,
        session_id: Some(session.id),
        launch_id: session.launch_id,
        seat: session.seat,
        model: Some(session.model),
        effort: session.effort,
        skills,
        data,
    })
}

/// Write the `session_ended` fact of a session whose run has ended, once per
/// launch: a session restarted under its own id ends once for each run.
///
/// Called wherever a session's status moves to `exited` or `failed`. A session
/// that is still live, or a run that already has its fact, writes nothing. A
/// failure is logged and goes no further: the ledger is a record, and the
/// session ending is what the caller came to do.
pub async fn record_session_end(store: &Store, session_id: &str) {
    match store.get_session(session_id).await {
        Ok(session) => record_run_end(store, &session).await,
        Err(e) => {
            warn!(session = %session_id, error = %e, "could not record the end of a session")
        }
    }
}

/// [`record_session_end`] off the row as it stood when the run ended: who
/// the run was, how it ended and when are the row's, and the turns and the
/// tokens are read now, so a caller that waits for the agent's last report
/// counts it.
pub(crate) async fn record_run_end(store: &Store, session: &AgentSession) {
    if let Err(e) = run_end(store, session).await {
        warn!(session = %session.id, error = %e, "could not record the end of a session");
    }
}

async fn run_end(store: &Store, session: &AgentSession) -> Result<()> {
    let session_id = session.id.as_str();
    let Some(ended_at) = session.ended_at.as_deref() else {
        return Ok(());
    };
    if session.status().is_live() {
        return Ok(());
    }
    let lifetime_secs = match (
        DateTime::parse_from_rfc3339(&session.created_at),
        DateTime::parse_from_rfc3339(ended_at),
    ) {
        (Ok(created), Ok(ended)) => (ended - created).num_seconds().max(0),
        _ => 0,
    };
    let usage = store.session_usage(session_id).await?;
    let data = json!({
        "status": session.status,
        "attention_reason": session.attention_reason,
        "lifetime_secs": lifetime_secs,
        "turns": store.count_session_events(session_id, "stop").await?,
        "input_tokens": usage.input_tokens,
        "cached_input_tokens": usage.cached_input_tokens,
        "output_tokens": usage.output_tokens,
    });
    let fact = fact_of(store, session.clone(), SESSION_ENDED, data).await?;
    store.record_fact_once_per_launch(fact).await?;
    Ok(())
}

/// Write the `switch` fact of a session that is leaving one model for
/// another, off the row as it stood before the switch moved it: `session`
/// is the model left.
///
/// Called next to the `session.switched` event `Launcher::switch_session`
/// writes, before anything of the switch it is about is undone. A failure is
/// logged and goes no further: a switch already in flight — a pin moved, an
/// agent killed, a successor created — is not something a ledger write can
/// still refuse.
pub(crate) async fn record_switch(store: &Store, session: &AgentSession, data: serde_json::Value) {
    if let Err(e) = switch_fact(store, session, data).await {
        warn!(session = %session.id, error = %e, "could not record a switch");
    }
}

async fn switch_fact(store: &Store, session: &AgentSession, data: serde_json::Value) -> Result<()> {
    let fact = fact_of(store, session.clone(), SWITCH, data).await?;
    store.record_fact(fact).await?;
    Ok(())
}
