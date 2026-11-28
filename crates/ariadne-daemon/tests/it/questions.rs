//! An agent asks the user with its own question tool: an ACP
//! `elicitation/create` in form mode (021). The daemon holds it open until a
//! person answers it at `/v1/sessions/{id}/questions`, and the agent reads the
//! answer as the result of its request.
//!
//! The stub agent asks the question its turn script names and sends the
//! answer back as the turn's text.

use crate::common;

use serde_json::{Value, json};

use ariadne_api::attention::AttentionListDto;
use ariadne_api::sessions::QuestionDto;
use ariadne_core::{PermissionMode, SessionStatus};
use ariadne_store::{AgentSession, EventFilter};
use axum::http::StatusCode;

use common::{Cast, Harness, TIMEOUT, eventually, get, harness, post_json};

/// The three field shapes each adapter sends, as codex-acp sent them live:
/// a single-select `string` with `oneOf`, a multi-select `array` with
/// `items.anyOf`, and a plain `string` for free text.
fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "base": {"type": "string", "title": "Which base branch?",
                     "oneOf": [{"const": "main", "title": "Main"},
                               {"const": "next", "title": "Next — the release line"}]},
            "checks": {"type": "array", "title": "Which checks?",
                       "items": {"anyOf": [{"const": "fmt", "title": "Format"},
                                           {"const": "clippy", "title": "Clippy"}]}},
            "note": {"type": "string", "title": "Anything else?"},
        },
        "required": ["base"],
        "_meta": {"codex": {"isOther": true}},
    })
}

/// The first turn asks the question; the turn after it says nothing.
fn asking(turn: Value) -> Value {
    let mut scripted = common::acp::script();
    let mut first = json!({
        "elicitation": {"message": "Codex needs your input to continue.",
                        "requestedSchema": schema(), "toolCallId": "call-ask"},
        "updates": [],
    });
    if let Some(extra) = turn.as_object() {
        for (key, value) in extra {
            first[key] = value.clone();
        }
    }
    scripted["prompts"] = json!([first, {"updates": []}]);
    scripted
}

/// A task in a real repository, whose agent the test starts itself.
async fn cast(h: &Harness) -> Cast {
    h.git_repo("repo");
    h.cast().await
}

async fn asked(h: &Harness, cast: &Cast) -> (AgentSession, QuestionDto) {
    let session = h.start_agent(&cast.task, "develop").await;
    let question = open_question(h, &session).await;
    (session, question)
}

async fn open_question(h: &Harness, session: &AgentSession) -> QuestionDto {
    let uri = format!("/v1/sessions/{}/questions", session.id);
    eventually(TIMEOUT, "the question to be open", || async {
        !h.get::<Vec<QuestionDto>>(&uri).await.is_empty()
    })
    .await;
    let mut questions: Vec<QuestionDto> = h.get(&uri).await;
    assert_eq!(questions.len(), 1, "{questions:?}");
    questions.remove(0)
}

fn answer_uri(session: &AgentSession, question: &QuestionDto) -> String {
    format!(
        "/v1/sessions/{}/questions/{}/answer",
        session.id, question.id
    )
}

/// The result the agent read for its `elicitation/create`.
fn reply_of(h: &Harness) -> Option<Value> {
    h.agent
        .messages()
        .into_iter()
        .find(|m| m.get("method").is_none() && m["id"] == "elicitation-1")
        .map(|m| m["result"].clone())
}

async fn payloads(h: &Harness, session: &AgentSession, kind: &str) -> Vec<Value> {
    h.store
        .list_events(EventFilter {
            session_id: Some(session.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap()
        .into_iter()
        .filter(|event| event.kind == kind)
        .map(|event| serde_json::from_str(&event.payload).unwrap())
        .collect()
}

/// No permission mode answers a question: in each one it waits for a person,
/// the pending route gives the form as the agent sent it, and the answer is
/// the result the agent reads. A second answer is refused.
#[tokio::test]
async fn a_question_waits_for_a_person_in_every_permission_mode() {
    for mode in [
        PermissionMode::Auto,
        PermissionMode::Ask,
        PermissionMode::Learn,
        PermissionMode::Ai,
    ] {
        let h = harness().await;
        h.agent.reprogram(asking(json!({})));
        let cast = cast(&h).await;
        h.set_permission_mode(&cast.repo, mode).await;
        let (session, question) = asked(&h, &cast).await;

        assert_eq!(question.session_id, session.id);
        assert_eq!(question.message, "Codex needs your input to continue.");
        assert_eq!(question.schema, schema(), "the form is the agent's own");
        assert_eq!(reply_of(&h), None, "{mode:?} answered the question itself");

        let content = json!({"base": "next", "checks": ["fmt", "clippy"], "note": "ship it"});
        let (status, _) = h
            .send(post_json(
                &answer_uri(&session, &question),
                json!({"action": "accept", "content": content}),
            ))
            .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let error = h
            .error(
                post_json(
                    &answer_uri(&session, &question),
                    json!({"action": "decline"}),
                ),
                StatusCode::CONFLICT,
            )
            .await;
        assert_eq!(error.error.code, "question_closed");

        eventually(TIMEOUT, "the answer to reach the agent", || async {
            reply_of(&h).is_some()
        })
        .await;
        assert_eq!(
            reply_of(&h).unwrap(),
            json!({"action": "accept", "content": content})
        );
        assert!(
            h.get::<Vec<QuestionDto>>(&format!("/v1/sessions/{}/questions", session.id))
                .await
                .is_empty()
        );
    }
}

/// The session's events record the question and how it closed: what a
/// reload, a reconnect and a replay all draw from.
#[tokio::test]
async fn the_events_record_the_question_and_its_result() {
    let h = harness().await;
    h.agent.reprogram(asking(json!({})));
    let cast = cast(&h).await;
    let (session, question) = asked(&h, &cast).await;

    let opened = payloads(&h, &session, "question").await;
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0]["question"], json!(question));
    assert_eq!(opened[0]["tool_call_id"], "call-ask");

    h.send(post_json(
        &answer_uri(&session, &question),
        json!({"action": "decline"}),
    ))
    .await;
    eventually(TIMEOUT, "the turn to end", || async {
        h.session_status(&session).await == SessionStatus::Idle
    })
    .await;
    let resolved = payloads(&h, &session, "question.resolved").await;
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0]["question_id"], json!(question.id));
    assert_eq!(resolved[0]["action"], "decline");
    assert_eq!(resolved[0]["closed_by"], "user");
    assert_eq!(reply_of(&h).unwrap(), json!({"action": "decline"}));
}

/// An answer is `accept` with content, or `decline` or `cancel` without; a
/// question the session never asked is not found.
#[tokio::test]
async fn a_malformed_or_unknown_answer_is_refused() {
    let h = harness().await;
    h.agent.reprogram(asking(json!({})));
    let cast = cast(&h).await;
    let (session, question) = asked(&h, &cast).await;

    h.error(
        post_json(
            &answer_uri(&session, &question),
            json!({"action": "accept"}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    h.error(
        post_json(
            &answer_uri(&session, &question),
            json!({"action": "cancel", "content": {"base": "main"}}),
        ),
        StatusCode::BAD_REQUEST,
    )
    .await;
    h.error(
        post_json(
            &format!("/v1/sessions/{}/questions/unknown/answer", session.id),
            json!({"action": "cancel"}),
        ),
        StatusCode::NOT_FOUND,
    )
    .await;
    assert_eq!(open_question(&h, &session).await, question);
}

/// Text typed at the console is a prompt, never the answer: it waits behind
/// the turn, and the question stays open until it is answered.
#[tokio::test]
async fn typed_console_input_waits_behind_the_turn_and_leaves_the_question_open() {
    let h = harness().await;
    h.agent.reprogram(asking(json!({})));
    let cast = cast(&h).await;
    let (session, question) = asked(&h, &cast).await;

    let (status, _) = h
        .send(post_json(
            &format!("/v1/sessions/{}/console/input", session.id),
            json!({"text": "typed while asked"}),
        ))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(open_question(&h, &session).await, question);
    assert_eq!(reply_of(&h), None);

    h.send(post_json(
        &answer_uri(&session, &question),
        json!({"action": "cancel"}),
    ))
    .await;
    eventually(TIMEOUT, "the typed prompt to follow the turn", || async {
        h.agent
            .prompts_for(&session.id)
            .iter()
            .any(|prompt| prompt.contains("typed while asked"))
    })
    .await;
    assert_eq!(reply_of(&h).unwrap(), json!({"action": "cancel"}));
}

/// An open question is one Needs attention item on its session's console,
/// and the item goes when the question closes.
#[tokio::test]
async fn an_open_question_is_one_attention_item_until_it_closes() {
    let h = harness().await;
    h.agent.reprogram(asking(json!({})));
    let cast = cast(&h).await;
    let (session, question) = asked(&h, &cast).await;

    let list: AttentionListDto = h.get("/v1/attention").await;
    let items: Vec<_> = list
        .items
        .iter()
        .filter(|item| item.id.starts_with("question:"))
        .collect();
    assert_eq!(items.len(), 1, "{:?}", list.items);
    assert_eq!(items[0].id, format!("question:{}", question.id));
    assert_eq!(
        serde_json::to_value(&items[0].target).unwrap(),
        json!({"kind": "console", "session_id": session.id})
    );

    h.send(post_json(
        &answer_uri(&session, &question),
        json!({"action": "decline"}),
    ))
    .await;
    let list: AttentionListDto = h.get("/v1/attention").await;
    assert!(
        list.items
            .iter()
            .all(|item| !item.id.starts_with("question:")),
        "{:?}",
        list.items
    );
}

/// A cancelled turn answers its open question `cancel`, as ACP has the
/// client do.
#[tokio::test]
async fn a_cancelled_turn_answers_the_open_question_cancel() {
    let h = harness().await;
    h.agent.reprogram(asking(json!({})));
    let cast = cast(&h).await;
    let (session, question) = asked(&h, &cast).await;

    let (status, _) = h
        .send(common::post(&format!(
            "/v1/sessions/{}/console/cancel",
            session.id
        )))
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    eventually(TIMEOUT, "the cancel to reach the agent", || async {
        reply_of(&h).is_some()
    })
    .await;
    assert_eq!(reply_of(&h).unwrap(), json!({"action": "cancel"}));
    eventually(TIMEOUT, "the close to be recorded", || async {
        !payloads(&h, &session, "question.resolved").await.is_empty()
    })
    .await;
    let resolved = payloads(&h, &session, "question.resolved").await;
    assert_eq!(resolved[0]["question_id"], json!(question.id));
    assert_eq!(resolved[0]["action"], "cancel");
}

/// An agent that cancels its own request closes the question: nobody is
/// asked any more.
#[tokio::test]
async fn an_agent_that_cancels_its_request_closes_the_question() {
    let h = harness().await;
    h.agent
        .reprogram(asking(json!({"withdraw_elicitation": true})));
    let cast = cast(&h).await;
    let session = h.start_agent(&cast.task, "develop").await;

    eventually(TIMEOUT, "the withdrawal to be recorded", || async {
        !payloads(&h, &session, "question.resolved").await.is_empty()
    })
    .await;
    let resolved = payloads(&h, &session, "question.resolved").await;
    assert_eq!(resolved[0]["closed_by"], "agent");
    assert!(
        h.get::<Vec<QuestionDto>>(&format!("/v1/sessions/{}/questions", session.id))
            .await
            .is_empty()
    );
    // The withdrawal is recorded before the agent reads its reply.
    eventually(TIMEOUT, "the cancel to reach the agent", || async {
        reply_of(&h).is_some()
    })
    .await;
    assert_eq!(reply_of(&h).unwrap(), json!({"action": "cancel"}));
}

/// The questions of a session that is not there are not found.
#[tokio::test]
async fn the_questions_of_an_unknown_session_are_not_found() {
    let h = harness().await;
    h.error(get("/v1/sessions/nobody/questions"), StatusCode::NOT_FOUND)
        .await;
}
