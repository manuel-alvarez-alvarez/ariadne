//! Attach/logs helpers: resolve an Ariadne id to a session's console.
//!
//! The id is a session, task or goal id — a task or goal one resolves to the
//! session of the wanted seat (the current column's agent for tasks, the
//! orchestrator for goals). With no live session for it, attach revives the most recent
//! matching session (`POST /v1/sessions/{id}/resume`) and attaches to the
//! fresh agent that resumes the same conversation.

use anyhow::{Result, bail};

use ariadne_api::sessions::{SessionDto, SessionEntryDto, SessionPageDto, SessionPageQuery};
use ariadne_api::tasks::TaskDto;
use ariadne_client::{Client, ClientError};
use ariadne_core::Seat;
use ariadne_core::models::agent_of;

use crate::commands::query_path;
use crate::output::{style, view};

/// A hint about what this command is doing on the caller's behalf — reviving
/// a session, attaching to one — never the payload itself, so it is dimmed
/// the way every other line that is context rather than content is.
fn hint(message: &str) -> String {
    style::paint(view().color, style::META, message)
}

/// Sessions matching the id, plus the seat to attach to: task first (default
/// agent), then goal (default orchestrator).
///
/// With no sessions on either side the id itself decides the wording — a task
/// without sessions used to be reported as a missing *orchestrator* session,
/// and an id naming nothing at all got the same message as a real task.
async fn candidates(
    client: &Client,
    id: &str,
    seat: Option<Seat>,
) -> Result<(Vec<SessionEntryDto>, Seat)> {
    for (query, default) in [("task", Seat::Agent), ("goal", Seat::Orchestrator)] {
        let page: SessionPageDto = client
            .get_json(&query_path(
                "/v1/sessions",
                &SessionPageQuery {
                    all: Some(true),
                    task: (query == "task").then(|| id.to_string()),
                    goal: (query == "goal").then(|| id.to_string()),
                    ..SessionPageQuery::default()
                },
            )?)
            .await?;
        let mut sessions = page.sessions;
        if sessions.is_empty() {
            continue;
        }
        let wanted = seat.unwrap_or(default);
        // Every column's agent sits in the one `agent` seat, and the agents
        // of the columns a task has left keep their sessions, live or not. A
        // task's console is its current column's, so the listing is narrowed
        // to that column's agent before a session is picked from it.
        if query == "task"
            && wanted == Seat::Agent
            && let Some(agent) = current_column_agent(client, id).await?
        {
            sessions.retain(|s| s.task_agent_id.as_deref() == Some(agent.as_str()));
        }
        return Ok((sessions, wanted));
    }
    for (kind, default) in [("tasks", Seat::Agent), ("goals", Seat::Orchestrator)] {
        if found::<serde_json::Value>(client, &format!("/v1/{kind}/{id}"))
            .await?
            .is_some()
        {
            return Ok((vec![], seat.unwrap_or(default)));
        }
    }
    bail!("no such task, goal or session: {id}")
}

/// The agent of the column a task is in, which is the agent its console
/// belongs to: none while the task has not entered a column, or where the
/// column has no agent yet.
async fn current_column_agent(client: &Client, task_id: &str) -> Result<Option<String>> {
    let Some(task) = found::<TaskDto>(client, &format!("/v1/tasks/{task_id}")).await? else {
        return Ok(None);
    };
    let Some(step) = task.step.as_deref() else {
        return Ok(None);
    };
    Ok(task
        .agents
        .iter()
        .find(|a| a.step == step)
        .map(|a| a.id.clone()))
}

/// What a GET on `path` answers with, or nothing at all when it 404s.
async fn found<T: serde::de::DeserializeOwned>(client: &Client, path: &str) -> Result<Option<T>> {
    match client.get_json::<T>(path).await {
        Ok(value) => Ok(Some(value)),
        Err(ClientError::Api { status, .. }) if status == http::StatusCode::NOT_FOUND => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Find the live session for a task or goal seat. Its persisted status is
/// the liveness check: the daemon's runtime owns the agent, and the liveness
/// sweep keeps the row honest.
pub(crate) async fn resolve_live(
    client: &Client,
    id: &str,
    seat: Option<Seat>,
) -> Result<SessionDto> {
    let (sessions, wanted) = candidates(client, id, seat).await?;
    let session = sessions
        .into_iter()
        .find(|s| s.seat == Some(wanted) && s.status.is_some_and(|status| status.is_live()))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no live {} session found for {id} (is the agent running?)",
                wanted.as_str()
            )
        })?;
    client
        .get_json(&format!("/v1/sessions/{}", session.id))
        .await
        .map_err(Into::into)
}

/// No live session: revive the most recent resumable session of the wanted
/// seat.
async fn revive(client: &Client, id: &str, seat: Option<Seat>) -> Result<SessionDto> {
    let (sessions, wanted) = candidates(client, id, seat).await?;
    let target = sessions
        .into_iter()
        .find(|s| s.seat == Some(wanted) && s.internal_session_id.is_some())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no {} session (live or finished) found for {id} that can be resumed",
                wanted.as_str()
            )
        })?;
    eprintln!(
        "{}",
        hint(&format!(
            "no live session for {id} — reviving session {} ({})",
            target.id, target.agent_id
        ))
    );
    client
        .post_empty(&format!("/v1/sessions/{}/resume", target.id))
        .await
        .map_err(Into::into)
}

/// Attach to one specific session: its console when it is live, else revive
/// it first.
async fn attach_session(client: &Client, session: SessionDto) -> Result<()> {
    let session = if session.status.is_live() {
        session
    } else {
        eprintln!(
            "{}",
            hint(&format!(
                "session {} has ended — reviving it ({})",
                session.id,
                agent_of(&session.model)
            ))
        );
        client
            .post_empty(&format!("/v1/sessions/{}/resume", session.id))
            .await?
    };
    attach_to(client, &session).await
}

/// The session of one named workflow column of a stepped task, whether or
/// not it is the current one — unlike [`resolve_live`], `--step` reaches a
/// column idle between its turns, which carries a session but no live one.
pub(crate) async fn resolve_step(client: &Client, task_id: &str, step: &str) -> Result<SessionDto> {
    let t: TaskDto = client.get_json(&format!("/v1/tasks/{task_id}")).await?;
    let agent = t
        .agents
        .iter()
        .find(|a| a.step == step)
        .ok_or_else(|| anyhow::anyhow!("task {task_id} staffs no agent on column {step}"))?;
    let session_id = agent.session_id.as_deref().ok_or_else(|| {
        anyhow::anyhow!("no session recorded yet for column {step} of task {task_id}")
    })?;
    client
        .get_json(&format!("/v1/sessions/{session_id}"))
        .await
        .map_err(Into::into)
}

/// `task attach --step <id>`: the console of the named column's agent.
pub(crate) async fn attach_step(client: &Client, task_id: &str, step: &str) -> Result<()> {
    let session = resolve_step(client, task_id, step).await?;
    attach_to(client, &session).await
}

/// Attach to a task or goal id: the live session of the wanted seat, or the
/// most recent resumable session of that seat revived.
pub(crate) async fn attach(client: &Client, id: &str, seat: Option<Seat>) -> Result<()> {
    let session = match resolve_live(client, id, seat).await {
        Ok(session) => session,
        Err(_) => revive(client, id, seat).await?,
    };
    attach_to(client, &session).await
}

/// `ariadne attach <id>`: session, task or goal id.
pub(crate) async fn attach_any(client: &Client, id: &str, seat: Option<Seat>) -> Result<()> {
    // Which of the three it is decides everything below, so a short id that
    // names one of each is refused here rather than resolved to whichever
    // list happens to be probed first.
    let id = &crate::commands::resolve::attachable(client, id).await?;
    if let Some(session) = found::<SessionDto>(client, &format!("/v1/sessions/{id}")).await? {
        if seat.is_some() {
            bail!(
                "--seat does not apply to a session id: {id} is already the {} session of that agent \
                 (pass the task or goal id to pick a seat)",
                session.seat.map_or("-", |seat| seat.as_str())
            );
        }
        return attach_session(client, session).await;
    }
    attach(client, id, seat).await
}

async fn attach_to(client: &Client, session: &SessionDto) -> Result<()> {
    eprintln!(
        "{}",
        hint(&format!(
            "attaching to the console ({} / {})",
            session.seat.map_or("-", |seat| seat.as_str()),
            agent_of(&session.model)
        ))
    );
    crate::commands::console::attach(client, &session.id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    use ariadne_core::SessionStatus;

    use crate::commands::fixtures;
    use crate::commands::fixtures::session;

    fn entry(id: &str, agent_id: &str) -> SessionEntryDto {
        SessionEntryDto {
            id: id.into(),
            agent_id: agent_id.into(),
            title: Some("Continue this work".into()),
            goal_id: Some("01GOAL".into()),
            task_id: None,
            seat: Some(Seat::Orchestrator),
            task_agent_id: None,
            model: Some("codex-acp:model".into()),
            effort: None,
            internal_session_id: Some(id.into()),
            working_directory: Some("/work/api".into()),
            status: Some(SessionStatus::Starting),
            attention_reason: None,
            attention_since: None,
            last_activity_at: Some("2026-09-12T12:00:00Z".into()),
            usage: Some(Default::default()),
            context_used: None,
            context_size: None,
            created_at: Some("2026-09-12T12:00:00Z".into()),
            ended_at: None,
            pull_request_id: None,
        }
    }

    /// A task's console is its current column's: with the sessions of two
    /// columns live, the one listed first by its latest activity being the
    /// column the task has left, attach and logs pick the current column's,
    /// and so does the revive of a task with no live session.
    #[tokio::test]
    async fn resolve_live_and_revive_pick_the_current_columns_session() {
        use std::sync::{Arc, Mutex};

        use axum::extract::{Path, State};
        use axum::routing::{get, post};
        use axum::{Json, Router};

        #[derive(Clone)]
        struct Api {
            live: bool,
            resumed: Arc<Mutex<Vec<String>>>,
        }

        async fn task() -> Json<TaskDto> {
            Json(TaskDto {
                step: Some("review".into()),
                agents: vec![
                    fixtures::agent("01A", "develop", &["coding"]),
                    fixtures::agent("01B", "review", &["code-review"]),
                ],
                ..fixtures::task("01TASK", "01GOAL")
            })
        }

        fn column_session(id: &str, agent: &str, live: bool) -> SessionEntryDto {
            let mut entry = entry(id, "codex-acp");
            entry.goal_id = Some("01GOAL".into());
            entry.task_id = Some("01TASK".into());
            entry.seat = Some(Seat::Agent);
            entry.task_agent_id = Some(agent.into());
            entry.status = Some(if live {
                SessionStatus::Idle
            } else {
                SessionStatus::Exited
            });
            entry.model = Some("codex-acp:model".into());
            entry
        }

        async fn listed(State(api): State<Api>) -> Json<SessionPageDto> {
            // The develop column's session reported last, so it is listed
            // first; the task is in its review column all the same.
            Json(SessionPageDto {
                sessions: vec![
                    column_session("01DEVELOP", "01A", api.live),
                    column_session("01REVIEW", "01B", api.live),
                ],
                next_cursor: None,
                total: 2,
            })
        }

        async fn one_session(Path(id): Path<String>) -> Json<SessionDto> {
            Json(session(&id, "01GOAL", Some("01TASK")))
        }

        async fn resume(State(api): State<Api>, Path(id): Path<String>) -> Json<SessionDto> {
            api.resumed.lock().unwrap().push(id.clone());
            Json(session(&id, "01GOAL", Some("01TASK")))
        }

        for live in [true, false] {
            let api = Api {
                live,
                resumed: Arc::new(Mutex::new(Vec::new())),
            };
            let app = Router::new()
                .route("/v1/tasks/01TASK", get(task))
                .route("/v1/sessions", get(listed))
                .route("/v1/sessions/{id}", get(one_session))
                .route("/v1/sessions/{id}/resume", post(resume))
                .with_state(api.clone());
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let client = Client::tcp(format!("http://{address}"));

            if live {
                let found = resolve_live(&client, "01TASK", None).await.unwrap();
                assert_eq!(
                    found.id, "01REVIEW",
                    "the current column, not the latest row"
                );
            } else {
                resolve_live(&client, "01TASK", None)
                    .await
                    .expect_err("nothing live");
                let revived = revive(&client, "01TASK", None).await.unwrap();
                assert_eq!(revived.id, "01REVIEW");
                assert_eq!(*api.resumed.lock().unwrap(), ["01REVIEW"]);
            }
            server.abort();
        }
    }

    /// `--step` finds the agent staffed on the named column and resolves its
    /// session, whether or not that column is the task's current one.
    #[tokio::test]
    async fn resolve_step_finds_the_named_columns_session() {
        use axum::extract::Path;
        use axum::routing::get;
        use axum::{Json, Router};

        async fn task() -> Json<TaskDto> {
            Json(TaskDto {
                step: Some("review".into()),
                agents: vec![
                    ariadne_api::tasks::TaskAgentDto {
                        session_id: Some("01DEVELOP".into()),
                        ..fixtures::agent("01A", "develop", &["coding"])
                    },
                    ariadne_api::tasks::TaskAgentDto {
                        session_id: Some("01REVIEW".into()),
                        ..fixtures::agent("01B", "review", &["code-review"])
                    },
                ],
                ..fixtures::task("01TASK", "01GOAL")
            })
        }

        async fn one_session(Path(id): Path<String>) -> Json<SessionDto> {
            Json(session(&id, "01GOAL", Some("01TASK")))
        }

        let app = Router::new()
            .route("/v1/tasks/01TASK", get(task))
            .route("/v1/sessions/{id}", get(one_session));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = Client::tcp(format!("http://{address}"));
        let found = resolve_step(&client, "01TASK", "develop").await.unwrap();
        assert_eq!(
            found.id, "01DEVELOP",
            "the idle column, not the current one"
        );

        let err = resolve_step(&client, "01TASK", "merge")
            .await
            .expect_err("no agent on that column");
        server.abort();
        assert!(err.to_string().contains("merge"), "{err}");
    }

    #[tokio::test]
    async fn revival_uses_the_newest_resumable_session() {
        use axum::extract::{Path, State};
        use axum::routing::{get, post};
        use axum::{Json, Router};

        #[derive(Clone)]
        struct Api(std::sync::Arc<std::sync::Mutex<Option<String>>>);

        fn ended(id: &str) -> SessionEntryDto {
            let mut entry = entry(id, "codex-acp");
            entry.status = Some(SessionStatus::Exited);
            entry.seat = Some(Seat::Agent);
            entry.model = Some("codex-acp:model".into());
            entry
        }

        async fn listed() -> Json<SessionPageDto> {
            Json(SessionPageDto {
                sessions: vec![ended("newest"), ended("oldest")],
                next_cursor: None,
                total: 2,
            })
        }

        async fn resumed(State(api): State<Api>, Path(id): Path<String>) -> Json<SessionDto> {
            *api.0.lock().unwrap() = Some(id.clone());
            Json(SessionDto {
                status: SessionStatus::Running,
                ..session(&id, "01GOAL", Some("01TASK"))
            })
        }

        let resumed_id = std::sync::Arc::new(std::sync::Mutex::new(None));
        let app = Router::new()
            .route("/v1/sessions", get(listed))
            .route("/v1/sessions/{id}/resume", post(resumed))
            .with_state(Api(resumed_id.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let resumed = revive(&Client::tcp(format!("http://{address}")), "01TASK", None)
            .await
            .unwrap();
        server.abort();

        assert_eq!(resumed.id, "newest");
        assert_eq!(resumed_id.lock().unwrap().as_deref(), Some("newest"));
    }
}
