//! An agent's flag that it waits for the user: `wait_for_user` (013), and the
//! user's answer to it (031).
//!
//! What comes here is the one-line hint the Needs attention list shows, and
//! nothing longer: a hint that tries to carry the question is refused with
//! the words that tell the agent where the question goes. The question
//! itself rides beside it, as text and the options the user can pick, and
//! the console draws it as a card.

use super::error::{ApiError, ApiResult, Json};
use super::{AppState, caller};
use ariadne_api::attention::{
    AgentQuestion, AgentRequestDto, AgentRequestResolution, AnswerAgentRequest, CreateAgentRequest,
};
use ariadne_api::events::{AgentEventDto, IngestEventRequest};
use ariadne_store::{AgentRequest, ClaimSettlement, Store};
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use serde_json::{Value, json};

/// The most characters a hint may run to. A hint is a line of the Needs
/// attention list, read beside a dozen others; the question it stands for
/// is in the console.
pub(crate) const HINT_MAX_CHARS: usize = 100;

/// The event kind recorded in the session's own stream when a request
/// opens, so a console shows the wait right where the question follows it.
pub(crate) const AGENT_REQUEST_EVENT: &str = "agent_request";

/// The event kind recorded when a request closes: answered, withdrawn or
/// expired. A console folds it into the request's card, so a card on a
/// reload or a replay never takes an answer again.
pub(crate) const AGENT_REQUEST_RESOLVED_EVENT: &str = "agent_request.resolved";

/// The error code of an answer to a request that is no longer pending.
const REQUEST_CLOSED: &str = "agent_request_closed";

#[utoipa::path(post, path = "/v1/sessions/{id}/agent-requests", tag = "sessions", request_body = CreateAgentRequest, params(("id" = String, Path)), responses((status = 200)))]
pub(crate) async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<CreateAgentRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    scoped_session(&state, &headers, &id).await?;
    let hint = request.summary.trim();
    if hint.is_empty() {
        return Err(ApiError::bad_request("the hint cannot be empty"));
    }
    if hint.lines().count() > 1 || hint.chars().count() > HINT_MAX_CHARS {
        return Err(ApiError::bad_request(format!(
            "the hint is one line of at most {HINT_MAX_CHARS} characters, not the question: \
             call `wait_for_user` with a short hint, and put the question in its `question` field"
        )));
    }
    if let Some(question) = &request.question {
        check_question(question)?;
    }
    let question = request
        .question
        .as_ref()
        .map(|question| serde_json::to_string(question).expect("a question serializes"));
    let turn_id = state.launcher.acp.agent_request_turn_id(&id).await;
    // The row and its opening event land together: no answer, withdrawal or
    // expiry can close the request before the console has the line it closes.
    let _requests = state.launcher.acp.agent_request_lock().await;
    let (row, created) = state
        .store
        .create_agent_request(&id, hint, question.as_deref(), turn_id.as_deref())
        .await?;
    // The wait is shown where the question follows it: one event in the
    // session's own stream, for the row this call opened — a repeat of a
    // pending hint is the same request, and adds no second line.
    if created {
        let mut payload = json!({"request_id": row.id, "hint": hint});
        if let Some(question) = &request.question {
            payload["question"] = json!(question);
        }
        super::ingest_event(
            &state.store,
            &IngestEventRequest {
                session_id: id.clone(),
                launch: None,
                kind: AGENT_REQUEST_EVENT.into(),
                payload,
            },
        )
        .await?;
    }
    Ok(Json(serde_json::json!({"id": row.id})))
}

/// The requests of a session that still wait for an answer, oldest first.
///
/// A console opens on the newest page of events (`GET /console`), and a
/// question asked before that page has no opening line in it. The console
/// reads this list with each snapshot, and draws every pending question it
/// does not already hold, so a reload or a reconnect never hides one.
#[utoipa::path(get, path = "/v1/sessions/{id}/agent-requests", tag = "sessions",
    operation_id = "list_agent_requests",
    params(("id" = String, Path)),
    responses((status = 200, body = [AgentRequestDto]), (status = 404)))]
pub(crate) async fn list(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<AgentRequestDto>>> {
    state.store.get_session(&id).await?;
    // Read under the request lock: a claim is settled under it too, so the
    // list never misses a request whose answer is on its way and then fails.
    let rows = {
        let _requests = state.launcher.acp.agent_request_lock().await;
        state.store.pending_session_agent_requests(&id).await?
    };
    Ok(Json(
        rows.into_iter()
            .map(|row| AgentRequestDto {
                question: question_of(&row),
                id: row.id,
                summary: row.summary,
                created_at: row.created_at,
            })
            .collect(),
    ))
}

/// A terminal console's snapshot with every pending question it lacks put
/// in front, as the line that opened it: the snapshot is the newest page of
/// events, and a question asked before it would otherwise not be drawn. The
/// line carries the request's own id, a ULID minted with it, so it sorts
/// before the page and never meets a stored event's id.
pub(super) async fn with_pending_questions(
    store: &Store,
    acp: &crate::acp::AcpRuntime,
    session_id: &str,
    snapshot: Vec<AgentEventDto>,
) -> Result<Vec<AgentEventDto>, ariadne_store::StoreError> {
    let shown: std::collections::HashSet<&str> = snapshot
        .iter()
        .filter(|event| event.kind == AGENT_REQUEST_EVENT)
        .filter_map(|event| event.payload.get("request_id")?.as_str())
        .collect();
    // Read under the request lock, as the pending list is: a claim whose
    // answer then fails to reach the agent is never seen gone.
    let pending = {
        let _requests = acp.agent_request_lock().await;
        store.pending_session_agent_requests(session_id).await?
    };
    let mut restored: Vec<AgentEventDto> = pending
        .into_iter()
        .filter(|row| row.question.is_some() && !shown.contains(row.id.as_str()))
        .map(|row| {
            let payload = json!({
                "request_id": row.id,
                "hint": row.summary,
                "question": question_of(&row),
            });
            AgentEventDto {
                summary: super::classify::summarize(AGENT_REQUEST_EVENT, &payload),
                id: row.id,
                session_id: Some(row.session_id),
                task_id: None,
                kind: AGENT_REQUEST_EVENT.into(),
                payload,
                created_at: row.created_at,
            }
        })
        .collect();
    if restored.is_empty() {
        return Ok(snapshot);
    }
    restored.extend(snapshot);
    Ok(restored)
}

/// A question the console can draw and the user can answer: text, options
/// with distinct ids and labels, and a recommendation that names one of them.
fn check_question(question: &AgentQuestion) -> ApiResult<()> {
    if question.text.trim().is_empty() {
        return Err(ApiError::bad_request("the question text cannot be empty"));
    }
    let mut ids = std::collections::HashSet::new();
    for option in &question.options {
        if option.id.trim().is_empty() || option.label.trim().is_empty() {
            return Err(ApiError::bad_request(
                "each option needs an `id` and a `label`",
            ));
        }
        if !ids.insert(option.id.as_str()) {
            return Err(ApiError::bad_request(format!(
                "the option id `{}` is given twice",
                option.id
            )));
        }
    }
    if let Some(recommended) = &question.recommended_option_id
        && !ids.contains(recommended.as_str())
    {
        return Err(ApiError::bad_request(format!(
            "`recommended_option_id` names `{recommended}`, which is not an option"
        )));
    }
    Ok(())
}

#[utoipa::path(post, path = "/v1/sessions/{session_id}/agent-requests/{request_id}", tag = "sessions", params(("session_id" = String, Path), ("request_id" = String, Path)), responses((status = 204)))]
pub(crate) async fn withdraw(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((session_id, request_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    scoped_session(&state, &headers, &session_id).await?;
    let _requests = state.launcher.acp.agent_request_lock().await;
    if state
        .store
        .withdraw_agent_request(&request_id, &session_id)
        .await?
    {
        record_resolution(
            &state.store,
            &session_id,
            &request_id,
            AgentRequestResolution::Withdrawn,
            None,
        )
        .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Answer one request: the option of its question the user picked, or the
/// text the user typed.
///
/// The answer reaches the agent as a fresh console prompt — sent at once if
/// the agent is between turns, or queued behind the turn that runs — and is
/// never read as the answer to a pending permission. A request that is no
/// longer pending (answered, withdrawn, or expired with the turn that asked
/// it) is refused with `409` and the code `agent_request_closed`, and sends
/// nothing. Where the prompt cannot reach the agent, the request opens again
/// so the user can retry.
#[utoipa::path(post, path = "/v1/sessions/{session_id}/agent-requests/{request_id}/answer",
    tag = "sessions",
    operation_id = "answer_agent_request",
    request_body = AnswerAgentRequest,
    params(("session_id" = String, Path), ("request_id" = String, Path)),
    responses((status = 204, description = "The answer was recorded and sent to the agent"),
        (status = 400), (status = 404), (status = 409)))]
pub(crate) async fn answer(
    State(state): State<AppState>,
    Path((session_id, request_id)): Path<(String, String)>,
    Json(request): Json<AnswerAgentRequest>,
) -> ApiResult<StatusCode> {
    let session = state.store.get_session(&session_id).await?;
    if !session.status().is_live() {
        return Err(ApiError::conflict(format!(
            "session {session_id} is {} and cannot take an answer",
            session.status
        )));
    }
    let row = state
        .store
        .session_agent_request(&request_id, &session_id)
        .await?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "not_found",
                format!("session {session_id} has no request {request_id}"),
            )
        })?;
    if row.answered_at.is_some() {
        return Err(closed(&row));
    }
    let question = question_of(&row);
    let answer = match (request.option_id.as_deref(), request.text.as_deref()) {
        (Some(option_id), None) => {
            let option = question
                .as_ref()
                .and_then(|question| question.options.iter().find(|o| o.id == option_id))
                .ok_or_else(|| {
                    ApiError::bad_request(format!(
                        "the question of request {request_id} has no option `{option_id}`"
                    ))
                })?;
            Answer::Option {
                id: option.id.clone(),
                label: option.label.clone(),
                description: option.description.clone(),
            }
        }
        (None, Some(text)) if !text.trim().is_empty() => Answer::Text(text.trim().to_string()),
        _ => {
            return Err(ApiError::bad_request(
                "an answer gives either `option_id` or a non-empty `text`",
            ));
        }
    };
    let _requests = state.launcher.acp.agent_request_lock().await;
    let Some(row) = state
        .store
        .claim_agent_request(&request_id, &session_id, &answer.record().to_string())
        .await?
    else {
        // Another answer, a withdrawal or the turn's end closed it first.
        let row = state
            .store
            .session_agent_request(&request_id, &session_id)
            .await?;
        return Err(row.as_ref().map_or_else(
            || {
                ApiError::new(
                    StatusCode::CONFLICT,
                    REQUEST_CLOSED,
                    "the request is closed",
                )
            },
            closed,
        ));
    };
    let prompt = answer.prompt(&row, question.as_ref());
    deliver_claimed(&state, &row, &answer, prompt).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Send a claimed request's answer to its agent as a console prompt, never
/// as a permission reply, and settle the claim: answered where it was sent;
/// where it was not, open again for a retry, or expired where a turn ended
/// meanwhile that would have expired it. Both the answer route and typed
/// console input deliver through here, holding the request lock from the
/// claim on.
pub(super) async fn deliver_claimed(
    state: &AppState,
    claimed: &AgentRequest,
    answer: &Answer,
    prompt: String,
) -> ApiResult<()> {
    let session_id = claimed.session_id.as_str();
    let sent = state
        .launcher
        .acp
        .send_prompt_input(session_id, prompt, Some(claimed.id.clone()));
    let settled = state
        .store
        .settle_agent_request_claim(claimed, sent.is_ok())
        .await?;
    match (sent, settled) {
        (Ok(()), _) => {
            record_resolution(
                &state.store,
                session_id,
                &claimed.id,
                AgentRequestResolution::Answered,
                Some(answer.record()),
            )
            .await?;
            state.store.clear_session_attention(session_id).await?;
            state.notify_scheduler_session(session_id);
            Ok(())
        }
        (Err(error), ClaimSettlement::Expired) => {
            record_resolution(
                &state.store,
                session_id,
                &claimed.id,
                AgentRequestResolution::Expired,
                None,
            )
            .await?;
            Err(ApiError::conflict(error.to_string()))
        }
        (Err(error), _) => Err(ApiError::conflict(error.to_string())),
    }
}

/// The refusal of an answer to a request that has closed, in words that say
/// how it closed.
fn closed(row: &AgentRequest) -> ApiError {
    let how = row.resolution.as_deref().unwrap_or("answered");
    let how = match how {
        "answered" | "claimed" | "claimed_expired" => "already answered",
        "withdrawn" => "withdrawn by the agent",
        "expired" => "expired: the turn that asked it ended",
        other => other,
    };
    ApiError::new(
        StatusCode::CONFLICT,
        REQUEST_CLOSED,
        format!("request {} is {how}", row.id),
    )
}

/// The question a request carries, where the agent asked one.
pub(super) fn question_of(row: &AgentRequest) -> Option<AgentQuestion> {
    row.question
        .as_deref()
        .and_then(|question| serde_json::from_str(question).ok())
}

/// What the user answered: an option of the question, or text.
pub(super) enum Answer {
    Option {
        id: String,
        label: String,
        description: Option<String>,
    },
    Text(String),
}

impl Answer {
    /// The answer as the request row and its event keep it.
    pub(super) fn record(&self) -> Value {
        match self {
            Self::Option { id, label, .. } => json!({"option_id": id, "text": label}),
            Self::Text(text) => json!({"text": text}),
        }
    }

    /// The prompt that carries the answer to the agent: the question it
    /// answers, then what the user picked and what that means, or the text
    /// the user typed.
    pub(super) fn prompt(&self, row: &AgentRequest, question: Option<&AgentQuestion>) -> String {
        let asked = question.map_or(row.summary.as_str(), |question| question.text.as_str());
        let answer = match self {
            Self::Option {
                id,
                label,
                description: Some(description),
            } => format!("{label} (option `{id}`): {description}"),
            Self::Option {
                id,
                label,
                description: None,
            } => format!("{label} (option `{id}`)"),
            Self::Text(text) => text.clone(),
        };
        format!("The user answered your question.\nQuestion: {asked}\nAnswer: {answer}")
    }

    /// What typed console text answers: the option it names by its number
    /// in the list, its id or its label, or else the text itself.
    pub(super) fn typed(question: Option<&AgentQuestion>, text: &str) -> Self {
        let typed = text.trim();
        let options = question.map_or(&[][..], |question| question.options.as_slice());
        let picked = typed
            .parse::<usize>()
            .ok()
            .and_then(|number| number.checked_sub(1))
            .and_then(|index| options.get(index))
            .or_else(|| {
                options
                    .iter()
                    .find(|option| option.id == typed || option.label.eq_ignore_ascii_case(typed))
            });
        match picked {
            Some(option) => Self::Option {
                id: option.id.clone(),
                label: option.label.clone(),
                description: option.description.clone(),
            },
            None => Self::Text(text.to_string()),
        }
    }
}

/// Record that a request closed, in the session's own stream.
pub(crate) async fn record_resolution(
    store: &Store,
    session_id: &str,
    request_id: &str,
    resolution: AgentRequestResolution,
    answer: Option<Value>,
) -> ApiResult<()> {
    let mut payload = json!({"request_id": request_id, "resolution": resolution.as_str()});
    if let Some(answer) = answer {
        payload["answer"] = answer;
    }
    super::ingest_event(
        store,
        &IngestEventRequest {
            session_id: session_id.to_string(),
            launch: None,
            kind: AGENT_REQUEST_RESOLVED_EVENT.into(),
            payload,
        },
    )
    .await?;
    Ok(())
}

async fn scoped_session(state: &AppState, headers: &HeaderMap, id: &str) -> ApiResult<()> {
    let context = caller::call_ctx(&state.store, headers).await?;
    if context.session.as_ref().map(|session| session.id.as_str()) != Some(id) {
        return Err(ApiError::forbidden(
            "agent requests belong to their own session",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariadne_api::attention::AgentQuestionOption;

    fn question() -> AgentQuestion {
        AgentQuestion {
            text: "Which base branch?".into(),
            options: vec![
                AgentQuestionOption {
                    id: "main".into(),
                    label: "Main".into(),
                    description: Some("Land on the trunk.".into()),
                },
                AgentQuestionOption {
                    id: "next".into(),
                    label: "Next".into(),
                    description: None,
                },
            ],
            recommended_option_id: Some("main".into()),
        }
    }

    /// Typed console text picks an option by its number, its id or its
    /// label, and is the user's own answer otherwise.
    #[test]
    fn typed_text_picks_an_option_by_number_id_or_label() {
        let question = question();
        for typed in ["2", "next", "NEXT", " next "] {
            assert!(
                matches!(Answer::typed(Some(&question), typed), Answer::Option { id, .. } if id == "next"),
                "{typed}"
            );
        }
        for typed in ["3", "0", "use next week"] {
            assert!(
                matches!(Answer::typed(Some(&question), typed), Answer::Text(text) if text == typed),
                "{typed}"
            );
        }
        assert!(matches!(Answer::typed(None, "1"), Answer::Text(_)));
    }

    /// The agent reads the question the answer is for, and what the picked
    /// option means.
    #[test]
    fn the_prompt_names_the_question_and_the_meaning_of_the_answer() {
        let row = AgentRequest {
            id: "01RQ".into(),
            session_id: "s".into(),
            summary: "Pick a base".into(),
            created_at: String::new(),
            answered_at: None,
            last_raised_turn_id: None,
            question: None,
            resolution: None,
            answer: None,
        };
        let question = question();
        assert_eq!(
            Answer::typed(Some(&question), "1").prompt(&row, Some(&question)),
            "The user answered your question.\nQuestion: Which base branch?\nAnswer: Main (option `main`): Land on the trunk."
        );
        assert_eq!(
            Answer::Text("release".into()).prompt(&row, None),
            "The user answered your question.\nQuestion: Pick a base\nAnswer: release"
        );
    }

    /// A store with one orchestrator session to raise requests under.
    async fn session_store() -> (Store, String, tempfile::TempDir) {
        use ariadne_store::{AgentPin, NewGoal, NewRepository, NewSession};
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("test.db")).await.unwrap();
        let repository = store
            .create_repository(NewRepository {
                default_workflow: None,
                path: "/tmp/probe".into(),
                base_branch: "main".into(),
                description: None,
                permission_mode: None,
            })
            .await
            .unwrap();
        let goal = store
            .create_goal(NewGoal {
                workflow: None,
                issue_url: None,
                title: "probe".into(),
                description: String::new(),
                repository_ids: vec![repository.id.clone()],
                pin: AgentPin {
                    model: "stub:test-model".into(),
                    effort: None,
                },
            })
            .await
            .unwrap();
        let session = store
            .create_session(NewSession {
                goal_id: Some(goal.id.clone()),
                task_id: None,
                seat: Some(ariadne_core::Seat::Orchestrator),
                task_agent_id: None,
                model: "stub:test-model".into(),
                effort: None,
                worktree_path: None,
                pull_request_id: None,
            })
            .await
            .unwrap();
        (store, session.id, dir)
    }

    fn later_event(id: &str) -> AgentEventDto {
        AgentEventDto {
            id: id.into(),
            session_id: None,
            task_id: None,
            kind: "agent_message".into(),
            payload: json!({"text": "later"}),
            summary: "later".into(),
            created_at: String::new(),
        }
    }

    /// A terminal console opens on a page that holds no line of a question
    /// still pending: the question is put in front, whole, and the CLI fold
    /// draws it open. One the page already holds, and a hint-only request,
    /// add nothing.
    #[tokio::test]
    async fn a_terminal_snapshot_restores_a_pending_question_older_than_its_page() {
        let (store, session, _dir) = session_store().await;
        let asked = serde_json::to_string(&question()).unwrap();
        let (row, _) = store
            .create_agent_request(&session, "Pick a base", Some(&asked), None)
            .await
            .unwrap();
        store
            .create_agent_request(&session, "Hint only", None, None)
            .await
            .unwrap();
        let page: Vec<AgentEventDto> = (0..201)
            .map(|index| later_event(&format!("ZZ{index:04}")))
            .collect();

        let acp = crate::acp::AcpRuntime::new(store.clone());
        let restored = with_pending_questions(&store, &acp, &session, page.clone())
            .await
            .unwrap();
        assert_eq!(restored.len(), 202);
        assert_eq!(restored[0].kind, AGENT_REQUEST_EVENT);
        assert_eq!(restored[0].payload["request_id"], row.id.as_str());
        assert_eq!(restored[0].payload["question"], json!(question()));
        let items = ariadne_console::transcript::fold(&restored);
        assert!(
            matches!(&items[0], ariadne_console::transcript::TranscriptItem::UserQuestion { closed: None, request_id, .. } if *request_id == row.id),
            "{:?}",
            items[0]
        );

        let again = with_pending_questions(&store, &acp, &session, restored.clone())
            .await
            .unwrap();
        assert_eq!(again.len(), restored.len());
    }

    /// A terminal snapshot raced against an answer that claims a pending
    /// question and fails to reach the agent always restores the question:
    /// the snapshot reads under the lock the claim is settled under.
    #[tokio::test]
    async fn a_terminal_snapshot_racing_a_failed_answer_keeps_the_question() {
        let (store, session, _dir) = session_store().await;
        let acp = crate::acp::AcpRuntime::new(store.clone());
        let asked = serde_json::to_string(&question()).unwrap();
        let (row, _) = store
            .create_agent_request(&session, "Pick a base", Some(&asked), None)
            .await
            .unwrap();
        let page = vec![later_event("ZZ0001")];
        for round in 0..32 {
            let answer = async {
                let _requests = acp.agent_request_lock().await;
                let claimed = store
                    .claim_agent_request(&row.id, &session, r#"{"text":"x"}"#)
                    .await
                    .unwrap()
                    .expect("the question is pending");
                tokio::task::yield_now().await;
                let sent = acp.send_prompt_input(&session, "x".into(), None);
                assert!(sent.is_err(), "no agent runs here");
                store
                    .settle_agent_request_claim(&claimed, false)
                    .await
                    .unwrap()
            };
            let (settled, restored) = tokio::join!(
                answer,
                with_pending_questions(&store, &acp, &session, page.clone())
            );
            assert_eq!(settled, ariadne_store::ClaimSettlement::Reopened);
            let restored = restored.unwrap();
            assert_eq!(restored.len(), 2, "round {round}: {restored:?}");
            assert_eq!(restored[0].payload["request_id"], row.id.as_str());
        }
    }

    /// A recommendation must name an option, and option ids are distinct.
    #[test]
    fn a_question_is_refused_where_its_options_do_not_add_up() {
        assert!(check_question(&question()).is_ok());
        let mut unknown = question();
        unknown.recommended_option_id = Some("other".into());
        assert!(check_question(&unknown).is_err());
        let mut twice = question();
        twice.options[1].id = "main".into();
        assert!(check_question(&twice).is_err());
        let mut empty = question();
        empty.text = " ".into();
        assert!(check_question(&empty).is_err());
        let typed = AgentQuestion {
            text: "Name the release.".into(),
            options: Vec::new(),
            recommended_option_id: None,
        };
        assert!(check_question(&typed).is_ok());
    }
}
