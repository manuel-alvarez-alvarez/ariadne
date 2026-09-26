//! One permission decision made by the local Laya server (022, Decisions).

use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use super::LayaLive;

pub(crate) const INSTRUCTIONS: &str =
    "Can this coding-agent tool call run without a person's review?";
pub(crate) const ALLOW_CRITERIA: &str = "reading files, searching, listing, building, running tests, editing files inside the working tree, git commands that do not delete branches or force-push";
pub(crate) const REVIEW_CRITERIA: &str = "deleting outside the working tree, force pushes, package installs, network writes, credentials or secrets, changes to system configuration, anything unclear";

#[derive(Debug, PartialEq)]
pub(crate) enum Decision {
    Allow {
        confidence: f64,
    },
    NotConfident {
        label: Option<String>,
        confidence: Option<f64>,
    },
}

pub(crate) async fn decide(
    live: &LayaLive,
    tool_call: &Value,
    options: &Value,
    repository: &Path,
    timeout: Duration,
) -> Decision {
    let input = compact_input(tool_call.get("rawInput"));
    let names = options
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|option| option.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(", ");
    let body = json!({
        "state": {
            "tool": tool_call.get("title").and_then(Value::as_str).unwrap_or("ACP tool"),
            "kind": tool_call.get("kind").and_then(Value::as_str).unwrap_or("unknown"),
            "input": input,
            "repository": repository.display().to_string(),
            "options": names,
        },
        "questions": {
            "decision": {
                "type": "choice",
                "instructions": INSTRUCTIONS,
                "criteria": {"allow": ALLOW_CRITERIA, "review": REVIEW_CRITERIA},
            }
        }
    });
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
            tracing::warn!(error = %error, "Laya permission decision failed");
            return not_confident();
        }
        Err(error) => {
            tracing::warn!(error = %error, "Laya permission decision timed out");
            return not_confident();
        }
    };
    let choice = answer
        .pointer("/answers/decision/choice/choice")
        .and_then(Value::as_str);
    let confidence = answer
        .pointer("/answers/decision/choice/confidence")
        .and_then(Value::as_f64);
    let (Some(label), Some(confidence)) = (choice, confidence) else {
        tracing::warn!("Laya permission decision was malformed");
        return not_confident();
    };
    if label == "allow" && confidence >= live.threshold {
        Decision::Allow { confidence }
    } else {
        Decision::NotConfident {
            label: Some(label.to_string()),
            confidence: Some(confidence),
        }
    }
}

fn compact_input(input: Option<&Value>) -> String {
    let compact =
        serde_json::to_string(input.unwrap_or(&Value::Null)).unwrap_or_else(|_| "null".to_string());
    compact.chars().take(2_000).collect()
}

fn not_confident() -> Decision {
    Decision::NotConfident {
        label: None,
        confidence: None,
    }
}
