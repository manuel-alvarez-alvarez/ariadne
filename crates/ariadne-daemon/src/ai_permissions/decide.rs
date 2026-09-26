//! One permission decision made by the local model server (022, Decisions).

use std::io;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::ser::Formatter;
use serde_json::{Value, json};

use super::AiPermissionsLive;

pub(crate) const CHECKPOINT: &str = "typed-decisions";
const QUESTION: &str = "Does this coding-agent tool call need a person's review?";
const ALLOW_CRITERIA: &str = "reading files, searching, listing, building, running tests, editing files inside the working tree, git commands that do not delete branches or force-push";
const REVIEW_CRITERIA: &str = "deleting outside the working tree, force pushes, package installs, network writes, credentials or secrets, changes to system configuration, anything unclear";
const GUARDRAILS: &str = include_str!("../../../../bench/ai-permissions/guardrails.json");
static EMPTY_INPUT: LazyLock<Value> = LazyLock::new(|| json!({}));

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

#[derive(Debug)]
pub(super) struct Guardrails {
    rules: Vec<Guardrail>,
}

#[derive(Debug)]
struct Guardrail {
    name: String,
    applies_to: AppliesTo,
    target: Target,
    pattern: Regex,
}

#[derive(Debug, Default, Deserialize)]
struct AppliesTo {
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    kinds: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GuardrailDefinition {
    name: String,
    #[serde(default)]
    applies_to: AppliesTo,
    target: Target,
    pattern: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Target {
    Command,
    Title,
    Input,
    Path,
}

impl Guardrails {
    pub(super) fn load() -> Result<Self> {
        Self::from_json(GUARDRAILS)
    }

    fn from_json(source: &str) -> Result<Self> {
        let definitions: Vec<GuardrailDefinition> =
            serde_json::from_str(source).context("reading AI permission guardrails")?;
        let rules = definitions
            .into_iter()
            .map(|definition| {
                let pattern = Regex::new(&definition.pattern).with_context(|| {
                    format!("compiling AI permission guardrail `{}`", definition.name)
                })?;
                Ok(Guardrail {
                    name: definition.name,
                    applies_to: definition.applies_to,
                    target: definition.target,
                    pattern,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self { rules })
    }

    pub(super) fn matching_name<'a>(&'a self, tool_call: &Value) -> Option<&'a str> {
        self.rules.iter().find_map(|rule| {
            rule.applies(tool_call)
                .then(|| rule.target_text(tool_call))
                .filter(|target| rule.pattern.is_match(target))
                .map(|_| rule.name.as_str())
        })
    }
}

impl Guardrail {
    fn applies(&self, tool_call: &Value) -> bool {
        let name = tool_call.get("name").and_then(Value::as_str);
        let kind = tool_call.get("kind").and_then(Value::as_str);
        (self.applies_to.names.is_empty()
            || name.is_some_and(|name| self.applies_to.names.iter().any(|item| item == name)))
            && (self.applies_to.kinds.is_empty()
                || kind.is_some_and(|kind| self.applies_to.kinds.iter().any(|item| item == kind)))
    }

    fn target_text(&self, tool_call: &Value) -> String {
        let raw_input = raw_input(tool_call);
        match self.target {
            Target::Command => string_value(raw_input.get("command")),
            Target::Title => string_value(tool_call.get("title")),
            Target::Input => python_json(raw_input),
            Target::Path => paths(tool_call).join("\n"),
        }
    }
}

pub(crate) async fn decide(
    live: &AiPermissionsLive,
    tool_call: &Value,
    options: &Value,
    repository: &Path,
    timeout: Duration,
) -> Decision {
    let body = request_body(tool_call, options, repository);
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
            return not_confident();
        }
        Err(error) => {
            tracing::warn!(error = %error, "AI permission model decision timed out");
            return not_confident();
        }
    };
    let probability_needs_review = answer
        .pointer("/answers/decision/noul")
        .and_then(Value::as_f64);
    let Some(probability_needs_review) = probability_needs_review else {
        tracing::warn!("AI permission model decision was malformed");
        return not_confident();
    };
    let confidence = 1.0 - probability_needs_review;
    if probability_needs_review < 0.5 && confidence >= live.threshold {
        Decision::Allow { confidence }
    } else {
        Decision::NotConfident {
            label: Some(
                if probability_needs_review < 0.5 {
                    "allow"
                } else {
                    "escalate"
                }
                .to_string(),
            ),
            confidence: Some(confidence),
        }
    }
}

fn request_body(tool_call: &Value, options: &Value, repository: &Path) -> Value {
    json!({
        "model": CHECKPOINT,
        "state": state(tool_call, options, repository),
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

fn state(tool_call: &Value, options: &Value, repository: &Path) -> String {
    let raw_input = raw_input(tool_call);
    let mut lines = Vec::new();
    push(&mut lines, "name", tool_call.get("name"));
    push(&mut lines, "tool", tool_call.get("title"));
    push(&mut lines, "kind", tool_call.get("kind"));
    push(&mut lines, "command", raw_input.get("command"));
    push(&mut lines, "reason", raw_input.get("description"));
    let paths = paths(tool_call).join(", ");
    if !paths.is_empty() {
        lines.push(format!("path: {paths}"));
    }
    let repository = repository.display().to_string();
    if !repository.is_empty() {
        lines.push(format!("cwd: {repository}"));
    }
    let option_names = options
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|option| option.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(", ");
    if !option_names.is_empty() {
        lines.push(format!("options: {option_names}"));
    }
    lines.join("\n")
}

fn push(lines: &mut Vec<String>, key: &str, value: Option<&Value>) {
    let value = string_value(value);
    if !value.is_empty() {
        lines.push(format!("{key}: {value}"));
    }
}

fn raw_input(tool_call: &Value) -> &Value {
    tool_call
        .get("rawInput")
        .filter(|value| !value.is_null())
        .unwrap_or(&EMPTY_INPUT)
}

fn string_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Bool(true)) => "True".to_string(),
        Some(Value::Bool(false)) => "False".to_string(),
        Some(Value::Number(value)) => value.to_string(),
        Some(value @ (Value::Array(_) | Value::Object(_))) => value.to_string(),
        Some(Value::Null) | None => String::new(),
    }
}

fn paths(tool_call: &Value) -> Vec<String> {
    let raw_input = raw_input(tool_call);
    let mut found = ["file_path", "path", "url"]
        .into_iter()
        .filter_map(|key| raw_input.get(key).and_then(Value::as_str))
        .map(str::to_string)
        .collect::<Vec<_>>();
    found.extend(
        tool_call
            .get("locations")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|location| location.get("path").and_then(Value::as_str))
            .map(str::to_string),
    );
    found
}

struct PythonJsonFormatter;

impl Formatter for PythonJsonFormatter {
    fn begin_array_value<W>(&mut self, writer: &mut W, first: bool) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn begin_object_key<W>(&mut self, writer: &mut W, first: bool) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn begin_object_value<W>(&mut self, writer: &mut W) -> io::Result<()>
    where
        W: ?Sized + io::Write,
    {
        writer.write_all(b": ")
    }
}

fn python_json(value: &Value) -> String {
    let mut bytes = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, PythonJsonFormatter);
    value
        .serialize(&mut serializer)
        .expect("serializing a JSON value cannot fail");
    String::from_utf8(bytes).expect("JSON is UTF-8")
}

fn not_confident() -> Decision {
    Decision::NotConfident {
        label: None,
        confidence: None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::io::Write;
    use std::process::{Command, Stdio};

    use super::*;

    #[test]
    fn every_benchmark_case_builds_the_winning_request_and_guardrail() {
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
        let fixture = lines(&bench.join("fixtures/winner-states.jsonl"))
            .map(|line| serde_json::from_str::<Value>(&line).unwrap())
            .map(|expected| (expected["id"].as_str().unwrap().to_string(), expected))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(cases.len(), fixture.len());

        let guardrails = Guardrails::load().unwrap();
        let mut states = Vec::with_capacity(cases.len() * 2);
        for (id, case) in &cases {
            let expected = &fixture[id];
            let body = request_body(
                &case["request"]["toolCall"],
                &case["request"]["options"],
                Path::new(case["repository"].as_str().unwrap()),
            );
            assert_eq!(body["model"], expected["model"], "{id}: model");
            assert_eq!(body["questions"], expected["questions"], "{id}: questions");
            assert_eq!(
                guardrails.matching_name(&case["request"]["toolCall"]),
                expected["guardrail"].as_str(),
                "{id}: guardrail"
            );
            states.push(body["state"].clone());
            states.push(expected["state"].clone());
        }

        let normalized = normalize_with_python(&states);
        for ((id, _), pair) in cases.iter().zip(normalized.chunks_exact(2)) {
            assert_eq!(pair[0], pair[1], "{id}: state");
        }
    }

    #[test]
    fn an_invalid_guardrail_stops_startup_and_names_the_rule() {
        let error = Guardrails::from_json(
            r#"[{"name":"broken-rule","applies_to":{},"target":"input","pattern":"["}]"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("broken-rule"), "{error:#}");
    }

    fn lines(path: &Path) -> impl Iterator<Item = String> {
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>()
            .into_iter()
    }

    fn normalize_with_python(values: &[Value]) -> Vec<String> {
        let mut child = Command::new("python3")
            .arg("-c")
            .arg(
                "import json,sys; values=json.load(sys.stdin); json.dump([json.dumps(value, ensure_ascii=False) for value in values], sys.stdout, ensure_ascii=False)",
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        serde_json::to_writer(child.stdin.as_mut().unwrap(), values).unwrap();
        child.stdin.take().unwrap().flush().unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        serde_json::from_slice(&output.stdout).unwrap()
    }
}
