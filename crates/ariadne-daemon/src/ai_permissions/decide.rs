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

/// What the model made of one request. `confidence` is the allow score,
/// `1 - noul`, and `threshold` the one it was held to.
///
/// `pub` only for `examples/ai_permission_eval.rs`, which links this crate as
/// a library; nothing else outside the crate should read it.
#[doc(hidden)]
#[derive(Debug, PartialEq)]
pub enum Decision {
    /// The request runs without asking anyone.
    Allow { confidence: f64, threshold: f64 },
    /// The model answered, but not an allow that clears the threshold:
    /// `label` is `allow` or `escalate`.
    NotConfident {
        label: &'static str,
        confidence: f64,
        threshold: f64,
    },
    /// The model gave no answer: its call `failed`, `timed out`, or came
    /// back `malformed`.
    Unanswered { reason: &'static str },
}

/// `pub` only for `examples/ai_permission_eval.rs`; see [`Decision`].
#[doc(hidden)]
pub async fn decide(
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
    let confidence = 1.0 - probability_needs_review;
    let threshold = live.threshold;
    if probability_needs_review < 0.5 && confidence >= threshold {
        Decision::Allow {
            confidence,
            threshold,
        }
    } else {
        Decision::NotConfident {
            label: if probability_needs_review < 0.5 {
                "allow"
            } else {
                "escalate"
            },
            confidence,
            threshold,
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
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    use super::*;

    #[test]
    fn every_benchmark_case_builds_the_winning_request() {
        let bench = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/ai-permissions");
        let mut case_paths = fs::read_dir(bench.join("cases"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "jsonl")
            })
            .collect::<Vec<_>>();
        case_paths.sort();
        let cases = case_paths
            .iter()
            .flat_map(|path| lines(path))
            .map(|line| serde_json::from_str::<Value>(&line).unwrap())
            .map(|case| (case["id"].as_str().unwrap().to_string(), case))
            .collect::<BTreeMap<_, _>>();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ai_permissions/fixtures");
        let fixture = lines(&fixtures.join("winner-states.jsonl"))
            .map(|line| serde_json::from_str::<Value>(&line).unwrap())
            .map(|expected| (expected["id"].as_str().unwrap().to_string(), expected))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(cases.len(), fixture.len());

        for (id, case) in &cases {
            let expected = &fixture[id];
            let body = request_body(&case["request"]["toolCall"], &case["request"]["options"]);
            assert_eq!(body["model"], expected["model"], "{id}: model");
            assert_eq!(body["questions"], expected["questions"], "{id}: questions");
            assert_eq!(body["state"], expected["state"], "{id}: state");
        }
    }

    fn lines(path: &Path) -> impl Iterator<Item = String> {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>()
            .into_iter()
    }
}
