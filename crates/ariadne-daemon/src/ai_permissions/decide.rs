//! One permission decision made by the local model server (022, Decisions).

use std::sync::LazyLock;
use std::time::Duration;

use serde_json::{Map, Value, json};

use super::AiPermissionsLive;

/// The Hugging Face Hub run `kev.serve --run` loads: the current benchmark
/// winner's. Shared with the install task: the install leaves everything
/// this run needs on disk before the server can start it.
pub(crate) const RUN: &str = "jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101";
/// The `model` label a request carries. Kev accepts any string here and
/// echoes it back; the checkpoint actually served is fixed by `RUN` at
/// launch.
const MODEL: &str = "kev-latest";
const QUESTION: &str = "Does this coding-agent tool call need a person's review?";
const ALLOW_CRITERIA: &str = "git status, diff, log, show, add or commit; cargo, npm, make, tsc, pytest, eslint or prettier builds, tests and lints; ls, find, cat or grep; reading or editing files under the repository path; fetching documentation";
const REVIEW_CRITERIA: &str = "anything not listed as safe";
const INPUT_CUT: usize = 2_000;
static EMPTY_INPUT: LazyLock<Value> = LazyLock::new(|| json!({}));

/// What the model made of one request. `danger` is the probability that the
/// request needs review.
#[derive(Debug, PartialEq)]
pub(crate) enum Decision {
    /// The request runs without asking anyone.
    Allow {
        danger: f64,
        allow_threshold: f64,
        deny_threshold: f64,
    },
    /// The request is rejected without asking anyone.
    Deny {
        danger: f64,
        allow_threshold: f64,
        deny_threshold: f64,
    },
    /// The request needs a person or a learned approval.
    Ask {
        danger: f64,
        allow_threshold: f64,
        deny_threshold: f64,
    },
    /// The model gave no answer: its call `failed`, `timed out`, or came
    /// back `malformed`.
    Unanswered { reason: &'static str },
}

pub(crate) async fn decide(
    live: &AiPermissionsLive,
    tool_call: &Value,
    options: &Value,
    timeout: Duration,
) -> Decision {
    let body = request_body(tool_call, options);
    let request = async {
        let body = serde_json::to_vec(&body)?;
        let response = reqwest::Client::new()
            .post(format!(
                "{}/v1/systemone",
                live.endpoint.trim_end_matches('/')
            ))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await?
            .error_for_status()?;
        let bytes = response.bytes().await?;
        Ok::<Value, anyhow::Error>(serde_json::from_slice(&bytes)?)
    };
    let answer = match tokio::time::timeout(timeout, request).await {
        Ok(Ok(answer)) => answer,
        Ok(Err(error)) => {
            tracing::warn!(error = %error, "AI permission model decision failed");
            return Decision::Unanswered { reason: "failed" };
        }
        Err(error) => {
            tracing::warn!(error = %error, "AI permission model decision timed out");
            return Decision::Unanswered {
                reason: "timed out",
            };
        }
    };
    let probability_needs_review = answer
        .pointer("/answers/decision/noul")
        .and_then(Value::as_f64);
    let Some(probability_needs_review) = probability_needs_review else {
        tracing::warn!("AI permission model decision was malformed");
        return Decision::Unanswered {
            reason: "malformed",
        };
    };
    let danger = probability_needs_review;
    let allow_threshold = live.allow_threshold;
    let deny_threshold = live.deny_threshold;
    if danger <= allow_threshold {
        Decision::Allow {
            danger,
            allow_threshold,
            deny_threshold,
        }
    } else if danger >= deny_threshold {
        Decision::Deny {
            danger,
            allow_threshold,
            deny_threshold,
        }
    } else {
        Decision::Ask {
            danger,
            allow_threshold,
            deny_threshold,
        }
    }
}

fn request_body(tool_call: &Value, options: &Value) -> Value {
    json!({
        "model": MODEL,
        "state": state(tool_call, options),
        "questions": {
            "decision": {
                "type": "noul",
                "instructions": QUESTION,
                "criteria": {
                    "false": ALLOW_CRITERIA,
                    "true": REVIEW_CRITERIA,
                },
            }
        }
    })
}

/// The `json` state Kev renders itself: `tool`, `kind`, `input` and
/// `options` where they are nonempty. Nothing is derived for the model: it
/// judges the call from the call alone.
fn state(tool_call: &Value, options: &Value) -> Value {
    let raw_input = raw_input(tool_call);
    let mut state = Map::new();
    insert_nonempty(&mut state, "tool", tool_call.get("title"));
    insert_nonempty(&mut state, "kind", tool_call.get("kind"));
    let input = compact_json(raw_input, INPUT_CUT);
    if !input.is_empty() {
        state.insert("input".to_string(), Value::String(input));
    }
    let option_names = option_names(options);
    if !option_names.is_empty() {
        state.insert("options".to_string(), Value::String(option_names));
    }
    Value::Object(state)
}

fn insert_nonempty(state: &mut Map<String, Value>, key: &str, value: Option<&Value>) {
    if let Some(Value::String(text)) = value
        && !text.is_empty()
    {
        state.insert(key.to_string(), Value::String(text.clone()));
    }
}

fn option_names(options: &Value) -> String {
    options
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|option| option.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The compact JSON of `value` (`serde_json`'s default separators already
/// match Python's `separators=(",", ":")`), cut at `cut` characters.
fn compact_json(value: &Value, cut: usize) -> String {
    let compact = serde_json::to_string(value).expect("serializing a JSON value cannot fail");
    compact.chars().take(cut).collect()
}

fn raw_input(tool_call: &Value) -> &Value {
    tool_call
        .get("rawInput")
        .filter(|value| !value.is_null())
        .unwrap_or(&EMPTY_INPUT)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// Each line of `fixtures/requests.jsonl` is one tool call and its options, with the
    /// model, state and questions the daemon must send for it: one call per kind of
    /// request, safe and risky alike.
    #[test]
    fn every_fixture_request_builds_its_recorded_model_state_and_questions() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/ai_permissions/fixtures/requests.jsonl");
        let fixture = std::fs::read_to_string(path).unwrap();
        let mut checked = 0;
        for line in fixture.lines().filter(|line| !line.trim().is_empty()) {
            let expected: Value = serde_json::from_str(line).unwrap();
            let id = expected["id"].as_str().unwrap();
            let body = request_body(
                &expected["request"]["toolCall"],
                &expected["request"]["options"],
            );
            assert_eq!(body["model"], expected["model"], "{id}: model");
            assert_eq!(body["questions"], expected["questions"], "{id}: questions");
            assert_eq!(body["state"], expected["state"], "{id}: state");
            checked += 1;
        }
        assert!(checked > 100, "the fixture holds {checked} requests");
    }
}
