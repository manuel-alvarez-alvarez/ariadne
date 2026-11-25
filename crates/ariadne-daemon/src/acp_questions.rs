//! The questions an agent asks the user with ACP `elicitation/create` in
//! form mode (021).
//!
//! The daemon holds each request open until a person answers it, cancels it,
//! or the session ends. No permission mode answers one. The open questions of
//! a launch live here, beside its permission slot: the answer route takes the
//! reply channel out, so one answer closes one question.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use ariadne_api::sessions::{AnswerQuestionRequest, QuestionAction, QuestionDto};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;

/// The event that records an opened question.
pub(crate) const QUESTION_EVENT: &str = "question";
/// The event that records how a question closed.
pub(crate) const QUESTION_RESOLVED_EVENT: &str = "question.resolved";

struct OpenQuestion {
    question: QuestionDto,
    reply: oneshot::Sender<Value>,
}

#[derive(Default)]
struct Held {
    open: Vec<OpenQuestion>,
    /// The questions of this launch that closed, so a late answer to one is
    /// told it is closed rather than unknown.
    closed: HashSet<String>,
}

impl Held {
    fn take_all(&mut self) -> Vec<OpenQuestion> {
        let open = std::mem::take(&mut self.open);
        self.closed
            .extend(open.iter().map(|held| held.question.id.clone()));
        open
    }
}

/// How an answer to one question went.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Answered {
    /// The answer is on its way to the agent.
    Sent,
    /// The question closed before this answer.
    Closed,
    /// This launch never asked it.
    Unknown,
}

/// The open questions of one launch, oldest first.
#[derive(Clone, Default)]
pub(crate) struct Questions {
    held: Arc<Mutex<Held>>,
    /// Held while a question is opened and recorded: a reader waits for the
    /// question as its stored event has it.
    asking: Arc<tokio::sync::Mutex<()>>,
}

impl Questions {
    fn held(&self) -> std::sync::MutexGuard<'_, Held> {
        self.held.lock().expect("ACP questions lock")
    }

    /// Hold readers off while a question is opened and recorded.
    pub(crate) async fn asking(&self) -> tokio::sync::OwnedMutexGuard<()> {
        self.asking.clone().lock_owned().await
    }

    /// The open questions, once no question is half asked.
    pub(crate) async fn settled(&self) -> Vec<QuestionDto> {
        let _asking = self.asking.lock().await;
        self.list()
    }

    /// Hold `question` open. The receiver gets the ACP result to send.
    pub(crate) fn open(&self, question: QuestionDto) -> oneshot::Receiver<Value> {
        let (reply, receiver) = oneshot::channel();
        let mut held = self.held();
        // A replayed request is the same question: its old reply slot goes.
        held.open.retain(|open| open.question.id != question.id);
        held.closed.remove(&question.id);
        held.open.push(OpenQuestion { question, reply });
        receiver
    }

    /// Replace the open question of the same id with `question`.
    pub(crate) fn settle(&self, question: QuestionDto) {
        if let Some(open) = self
            .held()
            .open
            .iter_mut()
            .find(|open| open.question.id == question.id)
        {
            open.question = question;
        }
    }

    pub(crate) fn list(&self) -> Vec<QuestionDto> {
        self.held()
            .open
            .iter()
            .map(|open| open.question.clone())
            .collect()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.held().open.is_empty()
    }

    /// Close the question `id` with `result`. One answer closes one
    /// question: a second finds it closed.
    pub(crate) fn answer(&self, id: &str, result: Value) -> Answered {
        let open = {
            let mut held = self.held();
            let Some(at) = held.open.iter().position(|open| open.question.id == id) else {
                return match held.closed.contains(id) {
                    true => Answered::Closed,
                    false => Answered::Unknown,
                };
            };
            held.closed.insert(id.to_string());
            held.open.remove(at)
        };
        match open.reply.send(result) {
            Ok(()) => Answered::Sent,
            Err(_) => Answered::Closed,
        }
    }

    /// Take the question `id` out without an answer: the agent withdrew it.
    pub(crate) fn withdraw(&self, id: &str) {
        let mut held = self.held();
        held.open.retain(|open| open.question.id != id);
        held.closed.insert(id.to_string());
    }

    /// Answer every open question `cancel`, as a cancelled turn does.
    pub(crate) fn cancel_all(&self) {
        let open = self.held().take_all();
        for question in open {
            let _ = question.reply.send(result_of(QuestionAction::Cancel, None));
        }
    }

    /// Take every open question out without an answer: the session ended.
    pub(crate) fn drain(&self) -> Vec<QuestionDto> {
        self.held()
            .take_all()
            .into_iter()
            .map(|open| open.question)
            .collect()
    }
}

/// The id of the question one request asks: the same for the same request
/// of the same launch, so a request a relay replays keeps its question.
pub(crate) fn question_id(launch_id: &str, request_id: &str) -> String {
    let digest = Sha256::digest(format!("{launch_id}\n{request_id}"));
    hex::encode(&digest[..13])
}

/// The ACP result of an `elicitation/create`.
pub(crate) fn result_of(
    action: QuestionAction,
    content: Option<&serde_json::Map<String, Value>>,
) -> Value {
    match (action, content) {
        (QuestionAction::Accept, Some(content)) => {
            json!({"action": "accept", "content": content})
        }
        (action, _) => json!({"action": action.as_str()}),
    }
}

/// Why an answer is refused, where the request itself is well formed.
pub(crate) fn answer_problem(answer: &AnswerQuestionRequest) -> Option<&'static str> {
    match (answer.action, &answer.content) {
        (QuestionAction::Accept, None) => Some("an `accept` answer gives the form `content`"),
        (QuestionAction::Decline | QuestionAction::Cancel, Some(_)) => {
            Some("only an `accept` answer gives `content`")
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question(id: &str) -> QuestionDto {
        QuestionDto {
            id: id.into(),
            session_id: "s".into(),
            message: "Pick one".into(),
            schema: json!({"type": "object"}),
            created_at: "2026-11-25T00:00:00Z".into(),
        }
    }

    #[test]
    fn one_answer_closes_one_question_and_a_second_is_refused() {
        let questions = Questions::default();
        let mut first = questions.open(question("a"));
        let _second = questions.open(question("b"));

        assert_eq!(
            questions.answer("a", json!({"action": "decline"})),
            Answered::Sent
        );
        assert_eq!(
            questions.answer("a", json!({"action": "decline"})),
            Answered::Closed
        );
        assert_eq!(
            questions.answer("z", json!({"action": "decline"})),
            Answered::Unknown
        );
        assert_eq!(first.try_recv().unwrap(), json!({"action": "decline"}));
        assert_eq!(
            questions
                .list()
                .into_iter()
                .map(|q| q.id)
                .collect::<Vec<_>>(),
            ["b"]
        );
    }

    #[test]
    fn a_request_keeps_its_question_id_within_its_launch() {
        assert_eq!(question_id("launch", "7"), question_id("launch", "7"));
        assert_ne!(question_id("launch", "7"), question_id("launch", "8"));
        assert_ne!(question_id("launch", "7"), question_id("other", "7"));
    }

    #[test]
    fn content_goes_to_the_agent_with_accept_only() {
        let content = serde_json::Map::from_iter([("q".to_string(), json!("alpha"))]);
        assert_eq!(
            result_of(QuestionAction::Accept, Some(&content)),
            json!({"action": "accept", "content": {"q": "alpha"}})
        );
        assert_eq!(
            result_of(QuestionAction::Decline, None),
            json!({"action": "decline"})
        );
        let refused = AnswerQuestionRequest {
            action: QuestionAction::Cancel,
            content: Some(content),
        };
        assert!(answer_problem(&refused).is_some());
    }
}
