//! An optional, local, advisory diagnosis of a failed ACP session (024).
//!
//! `acp.rs` already decides whether a failure is exhausted, from the
//! protocol's own fields and the configured text patterns
//! (`acp.rs::exhausted_reason`) — that decision, and the switch it may
//! start, is never touched here. What this adds is a second, finer opinion,
//! read from the AI permission model's own local Kev service once the
//! session's error is already recorded and published, run as its own
//! bounded task so that nothing waiting on the ACP driver or the scheduler
//! ever waits on it.
//!
//! It never installs a model, never waits for one to load, and never
//! queues more than the one request already in flight: a diagnosis that
//! cannot run now — the model is off, busy, or silent — simply does not
//! run, and the original error stands on its own, exactly as it does today.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Map, Value, json};
use tokio::task::JoinHandle;

use ariadne_api::events::IngestEventRequest;
use ariadne_store::Store;

use crate::ai_permissions::AiPermissions;

/// The label Kev accepts and echoes, the same one a permission decision
/// uses (022, Decisions).
const MODEL: &str = "kev-latest";

const INSTRUCTIONS: &str = "Classify this coding agent error from its explicit evidence. Do not treat a quoted example, a negated claim, or token accounting as resource exhaustion.";

/// The five diagnoses a failed session can be classified as, each with the
/// criterion Kev is asked to tell it apart by. This is the stable category
/// vocabulary the event payload's `category` field is drawn from.
const CATEGORIES: [(&str, &str); 5] = [
    (
        "exhausted",
        "The model or account cannot continue because its usage, credit, quota, or rate allocation is exhausted.",
    ),
    (
        "temporary",
        "The request can reasonably be retried because the service has a temporary fault, overload, timeout, or transient limit.",
    ),
    (
        "auth_config",
        "Credentials, permissions, account setup, model setup, or configuration must be corrected.",
    ),
    (
        "task_error",
        "The task, request, tool input, repository state, or command failed and needs task-specific correction.",
    ),
    (
        "insufficient",
        "The payload has no reliable evidence for one diagnosis.",
    ),
];

/// How much of the error's message and data a classification request
/// carries — a bounded snapshot of the one failure, never the session's
/// files or its transcript.
const SNAPSHOT_CUT: usize = 2_000;

/// The daemon's optional failure classifier: the model's existing live
/// handle, and admission for at most one request in flight.
#[derive(Clone)]
pub struct FailureDiagnosis {
    inner: Arc<Inner>,
}

struct Inner {
    enabled: bool,
    ai_permissions: AiPermissions,
    store: Store,
    timeout: Duration,
    /// Claimed with a compare-and-swap, so at most one diagnosis request
    /// runs at a time: a request that finds one already running is skipped
    /// rather than queued.
    busy: AtomicBool,
    /// The one diagnosis task in flight, if any — aborted at daemon
    /// shutdown so none outlives it.
    current: Mutex<Option<JoinHandle<()>>>,
}

/// Resets `busy` and forgets the registered abort handle when the task that
/// holds it ends, whichever way: ran to its own conclusion, or was dropped
/// mid-flight by [`AiPermissions::preempt_diagnosis`]. Either way the next
/// failure is free to be considered, and no stale handle is ever aborted by
/// a later, unrelated request.
struct BusyGuard(Arc<Inner>);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
        self.0.ai_permissions.clear_diagnosis_request();
    }
}

impl FailureDiagnosis {
    pub fn new(
        enabled: bool,
        ai_permissions: AiPermissions,
        store: Store,
        timeout: Duration,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                enabled,
                ai_permissions,
                store,
                timeout,
                busy: AtomicBool::new(false),
                current: Mutex::new(None),
            }),
        }
    }

    /// Consider a failed session for an advisory diagnosis: a no-op unless
    /// the feature is enabled, the model is already live — this never
    /// starts or waits for one — and no other diagnosis is running. Returns
    /// at once either way. The classification itself, where it runs at all,
    /// is its own task: it never holds up the caller, which is expected to
    /// have already recorded and published the session's own error.
    pub(crate) async fn consider(
        &self,
        session_id: String,
        launch_id: String,
        error_event_id: Option<String>,
        message: String,
        data: Option<Value>,
    ) {
        if !self.inner.enabled {
            return;
        }
        let Some(error_event_id) = error_event_id else {
            return;
        };
        // Reads whether a server is already up; never waits on one loading
        // because an error just happened.
        let Some(live) = self.inner.ai_permissions.live().await else {
            return;
        };
        if self.inner.busy.swap(true, Ordering::AcqRel) {
            return;
        }
        let inner = self.inner.clone();
        let handle = tokio::spawn(async move {
            let _busy = BusyGuard(inner.clone());
            if let Some((category, probabilities, model)) =
                classify(&live.endpoint, &message, data.as_ref(), inner.timeout).await
            {
                let mut payload = Map::new();
                payload.insert("error_event_id".into(), json!(error_event_id));
                payload.insert("launch_id".into(), json!(launch_id.clone()));
                payload.insert("category".into(), json!(category));
                payload.insert("model".into(), json!(model));
                if let Some(probabilities) = probabilities {
                    payload.insert("probabilities".into(), probabilities);
                }
                let request = IngestEventRequest {
                    session_id,
                    launch: Some(launch_id),
                    kind: "session.diagnosis".into(),
                    payload: Value::Object(payload),
                };
                if let Err(error) = crate::http::ingest_event(&inner.store, &request).await {
                    tracing::warn!(error = %error, "recording a failure diagnosis failed");
                }
            }
        });
        // A permission decision that needs the same model aborts this
        // handle before it asks the model itself (024); registered right
        // after the spawn, so there is no window in which this request
        // cannot be preempted.
        self.inner
            .ai_permissions
            .register_diagnosis_request(handle.abort_handle());
        *self.inner.current.lock().expect("failure diagnosis lock") = Some(handle);
    }

    /// Cancel the one diagnosis task in flight, if any, so it does not
    /// outlive the daemon.
    pub async fn shutdown(&self) {
        let handle = self
            .inner
            .current
            .lock()
            .expect("failure diagnosis lock")
            .take();
        if let Some(handle) = handle {
            handle.abort();
            let _ = handle.await;
        }
    }
}

/// Ask Kev once, bounded by `timeout`, and validate its answer. `None` for
/// every outcome that is not a usable diagnosis: unavailable, failed, timed
/// out, or malformed — the same "unanswered" shape a permission decision
/// fails open with (`ai_permissions::decide::Decision::Unanswered`).
async fn classify(
    endpoint: &str,
    message: &str,
    data: Option<&Value>,
    timeout: Duration,
) -> Option<(String, Option<Value>, String)> {
    let body = request_body(message, data);
    let request = async {
        let bytes = serde_json::to_vec(&body)?;
        let response = reqwest::Client::new()
            .post(format!("{}/v1/systemone", endpoint.trim_end_matches('/')))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(bytes)
            .send()
            .await?
            .error_for_status()?;
        let bytes = response.bytes().await?;
        Ok::<Value, anyhow::Error>(serde_json::from_slice(&bytes)?)
    };
    let answer = match tokio::time::timeout(timeout, request).await {
        Ok(Ok(answer)) => answer,
        Ok(Err(error)) => {
            tracing::warn!(error = %error, "failure diagnosis request failed");
            return None;
        }
        Err(_) => {
            tracing::warn!("failure diagnosis request timed out");
            return None;
        }
    };
    validate(&answer)
}

/// The request body Kev answers: a bounded snapshot of the error under a
/// `diagnosis` choice question, one criterion per category.
fn request_body(message: &str, data: Option<&Value>) -> Value {
    let mut criteria = Map::new();
    for (label, description) in CATEGORIES {
        criteria.insert(label.into(), json!(description));
    }
    json!({
        "model": MODEL,
        "state": {
            "error_message": cut(message, SNAPSHOT_CUT),
            "error_data": cut_json(data, SNAPSHOT_CUT),
        },
        "questions": {
            "diagnosis": {
                "type": "choice",
                "instructions": INSTRUCTIONS,
                "criteria": Value::Object(criteria),
            }
        }
    })
}

/// Validate Kev's answer: the model identity it served from, a category
/// this module knows, and, only where every one of them validates too, the
/// probability of each category as a finite value between 0 and 1. A
/// missing or empty model identity, or an unknown category, is not a
/// diagnosis at all; invalid probabilities beside a known category still
/// keep the category, without them.
fn validate(answer: &Value) -> Option<(String, Option<Value>, String)> {
    let model = answer.get("model")?.as_str()?.trim();
    if model.is_empty() {
        return None;
    }
    let diagnosis = answer.pointer("/answers/diagnosis")?.as_object()?;
    let choice = diagnosis.get("choice")?.as_str()?;
    if !CATEGORIES.iter().any(|(label, _)| *label == choice) {
        return None;
    }
    let probabilities = diagnosis
        .get("probabilities")
        .and_then(Value::as_object)
        .filter(|map| {
            CATEGORIES.iter().all(|(label, _)| {
                map.get(*label)
                    .and_then(Value::as_f64)
                    .is_some_and(|probability| {
                        probability.is_finite() && (0.0..=1.0).contains(&probability)
                    })
            })
        })
        .cloned()
        .map(Value::Object);
    Some((choice.to_string(), probabilities, model.to_string()))
}

/// `text`'s first `limit` characters, marked where it was cut — counted in
/// characters, like every other cut in Ariadne, so an accent is never split
/// in half.
fn cut(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    text.chars()
        .take(limit.saturating_sub(1))
        .chain(['…'])
        .collect()
}

/// `data` whole where its compact JSON fits `limit` characters, else that
/// JSON cut to it: the model always reads a bounded snapshot, never the
/// whole of a large structured error.
fn cut_json(data: Option<&Value>, limit: usize) -> Value {
    let Some(data) = data else {
        return Value::Null;
    };
    let text = serde_json::to_string(data).unwrap_or_default();
    if text.chars().count() <= limit {
        return data.clone();
    }
    json!(cut(&text, limit))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(choice: &str, probabilities: Value) -> Value {
        answer_from("kev-latest", choice, probabilities)
    }

    fn answer_from(model: &str, choice: &str, probabilities: Value) -> Value {
        json!({"model": model, "answers": {"diagnosis": {"type": "choice", "choice": choice, "confidence": 0.8, "probabilities": probabilities}}})
    }

    /// A well-formed answer keeps its category, the model it was served
    /// from, and its validated probabilities, keyed exactly as the
    /// request's criteria were.
    #[test]
    fn a_well_formed_answer_keeps_its_category_model_and_probabilities() {
        let probabilities = json!({
            "exhausted": 0.7, "temporary": 0.1, "auth_config": 0.1,
            "task_error": 0.05, "insufficient": 0.05,
        });
        assert_eq!(
            validate(&answer("exhausted", probabilities.clone())),
            Some((
                "exhausted".to_string(),
                Some(probabilities),
                "kev-latest".to_string()
            ))
        );
    }

    /// The model identity stored is the one the response actually names,
    /// not the one the request asked for: a service free to answer as
    /// another checkpoint is taken at its word.
    #[test]
    fn the_stored_model_identity_is_the_responses_own() {
        let probabilities = json!({
            "exhausted": 0.1, "temporary": 0.1, "auth_config": 0.1,
            "task_error": 0.6, "insufficient": 0.1,
        });
        let (_, _, model) =
            validate(&answer_from("kev-8b@deadbeef", "task_error", probabilities)).unwrap();
        assert_eq!(model, "kev-8b@deadbeef");
    }

    /// A category outside the five this module knows is not a diagnosis:
    /// the answer is dropped whole, probabilities included.
    #[test]
    fn an_unknown_category_is_dropped_whole() {
        assert_eq!(validate(&answer("made_up", json!({"made_up": 1.0}))), None);
    }

    /// A missing or empty model identity is not a diagnosis either, even
    /// with an otherwise well-formed category: a diagnosis this module
    /// cannot attribute to a model is not one it stores.
    #[test]
    fn a_missing_or_empty_model_identity_is_not_a_diagnosis() {
        for answer in [
            json!({"answers": {"diagnosis": {"choice": "exhausted", "probabilities": {}}}}),
            json!({"model": "", "answers": {"diagnosis": {"choice": "exhausted", "probabilities": {}}}}),
            json!({"model": "  ", "answers": {"diagnosis": {"choice": "exhausted", "probabilities": {}}}}),
        ] {
            assert_eq!(validate(&answer), None, "{answer}");
        }
    }

    /// Probabilities that are missing a category, out of range, or not
    /// finite are dropped — but the category they sat beside still stands,
    /// since it validated on its own.
    #[test]
    fn a_malformed_probability_keeps_the_category_without_it() {
        for probabilities in [
            json!({"exhausted": 0.7}),
            json!({
                "exhausted": 1.7, "temporary": 0.1, "auth_config": 0.1,
                "task_error": 0.05, "insufficient": 0.05,
            }),
            json!("not an object"),
        ] {
            assert_eq!(
                validate(&answer("exhausted", probabilities.clone())),
                Some(("exhausted".to_string(), None, "kev-latest".to_string())),
                "{probabilities}"
            );
        }
    }

    /// An answer with no usable `choice` at all is not a diagnosis.
    #[test]
    fn an_answer_with_no_readable_choice_is_not_a_diagnosis() {
        assert_eq!(validate(&json!({})), None);
        assert_eq!(
            validate(&json!({"model": "kev-latest", "answers": {}})),
            None
        );
        assert_eq!(
            validate(&json!({
                "model": "kev-latest",
                "answers": {"diagnosis": {"probabilities": {}}},
            })),
            None
        );
    }

    /// The request carries the bounded snapshot and the five-category
    /// question, never a repository file or a whole transcript.
    #[test]
    fn the_request_carries_a_bounded_snapshot_and_the_five_categories() {
        let body = request_body("rate limited", Some(&json!({"code": -32000})));
        assert_eq!(body["model"], json!(MODEL));
        assert_eq!(body["state"]["error_message"], json!("rate limited"));
        assert_eq!(body["state"]["error_data"], json!({"code": -32000}));
        let criteria = body["questions"]["diagnosis"]["criteria"]
            .as_object()
            .unwrap();
        assert_eq!(criteria.len(), 5);
        for (label, _) in CATEGORIES {
            assert!(criteria.contains_key(label), "{label}");
        }
    }

    /// A message or a structured error over the bound is cut, and marked.
    #[test]
    fn an_oversized_message_and_data_are_cut_to_the_bound() {
        let long_message = "x".repeat(SNAPSHOT_CUT + 50);
        let cut_message = cut(&long_message, SNAPSHOT_CUT);
        assert_eq!(cut_message.chars().count(), SNAPSHOT_CUT);
        assert!(cut_message.ends_with('…'));

        let long_data = json!({"detail": "y".repeat(SNAPSHOT_CUT + 50)});
        let Value::String(cut_data) = cut_json(Some(&long_data), SNAPSHOT_CUT) else {
            panic!("an oversized data value cuts to a string");
        };
        assert_eq!(cut_data.chars().count(), SNAPSHOT_CUT);
        assert!(cut_data.ends_with('…'));

        assert_eq!(cut_json(None, SNAPSHOT_CUT), Value::Null);
    }
}
