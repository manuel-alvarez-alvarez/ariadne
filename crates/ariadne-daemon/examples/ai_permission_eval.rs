//! Runs the daemon's real AI permission decision path — guardrails, then the
//! request to Kev, then the threshold — against case JSON Lines from stdin
//! (022, rule 37). `bench/ai-permissions`'s `ariadne` evaluator is the only
//! caller; nothing here is part of the daemon's API.
#![doc(hidden)]

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde::Serialize;
use serde_json::Value;

use ariadne_daemon::ai_permissions::decide::{self, Decision, Guardrails};
use ariadne_daemon::ai_permissions::{AiPermissionsLive, DEFAULT_THRESHOLD, server};
use ariadne_daemon::timeouts::Timeouts;

#[derive(Parser)]
struct Args {
    /// A running `kev.serve`'s base URL. Without it, this starts the model
    /// the daemon installed, on an ephemeral loopback port, and stops it on
    /// exit.
    #[arg(long)]
    endpoint: Option<String>,
    /// The allow score the model's answer must clear (default: the daemon's).
    #[arg(long)]
    threshold: Option<f64>,
}

#[derive(Serialize)]
struct OutputLine {
    id: String,
    allow_score: Option<f64>,
    label: &'static str,
    guardrail: Option<String>,
    latency_ms: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let threshold = args.threshold.unwrap_or(DEFAULT_THRESHOLD);
    let timeouts = Timeouts::default();

    let mut server = match &args.endpoint {
        Some(endpoint) => Server::Remote(endpoint.clone()),
        None => Server::spawn(&timeouts).await?,
    };
    let live = AiPermissionsLive {
        endpoint: server.endpoint().to_string(),
        threshold,
    };
    let guardrails = Guardrails::load().context("loading the AI permission guardrails")?;

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.context("reading a case line")?;
        if line.trim().is_empty() {
            continue;
        }
        let output = evaluate(&guardrails, &live, timeouts.ai_permissions_decision, &line).await?;
        writeln!(stdout, "{}", serde_json::to_string(&output)?)?;
        stdout.flush()?;
    }

    server.stop().await;
    Ok(())
}

/// One case line to one output line: the guardrail first, then the model,
/// exactly as `decide::decide` decides it — except a `label` of `allow` here
/// means only [`Decision::Allow`], since every other outcome is what the
/// daemon would hand to a person.
async fn evaluate(
    guardrails: &Guardrails,
    live: &AiPermissionsLive,
    timeout: Duration,
    case_line: &str,
) -> Result<OutputLine> {
    let case: Value = serde_json::from_str(case_line).context("parsing a case line")?;
    let id = case["id"].as_str().unwrap_or_default().to_string();
    let repository = PathBuf::from(case["repository"].as_str().unwrap_or_default());
    let tool_call = &case["request"]["toolCall"];
    let options = &case["request"]["options"];

    let start = Instant::now();
    let (allow_score, label, guardrail) = match guardrails.matching_name(tool_call) {
        Some(name) => (None, "escalate", Some(name.to_string())),
        None => match decide::decide(live, tool_call, options, &repository, timeout).await {
            Decision::Allow { confidence, .. } => (Some(confidence), "allow", None),
            Decision::NotConfident { confidence, .. } => (Some(confidence), "escalate", None),
            Decision::Unanswered { .. } => (None, "escalate", None),
        },
    };
    let latency_ms = start.elapsed().as_secs_f64() * 1000.0;

    Ok(OutputLine {
        id,
        allow_score,
        label,
        guardrail,
        latency_ms,
    })
}

/// The server this run started, if any, so it can be stopped on exit. A
/// `--endpoint` run started none and stops nothing.
enum Server {
    Remote(String),
    Started {
        child: tokio::process::Child,
        endpoint: String,
    },
}

impl Server {
    /// Start the model the daemon installed, on an ephemeral loopback port,
    /// with the daemon's own server-start code (`server::spawn`).
    async fn spawn(timeouts: &Timeouts) -> Result<Self> {
        let home = ariadne_client::endpoint::home(None)
            .context("cannot determine the Ariadne home directory")?
            .join("ai-permissions");
        let python = home.join("venv").join("bin").join("python");
        if !python.is_file() {
            bail!(
                "the AI permission model is not installed at {}; turn it on first",
                home.display()
            );
        }
        let serve_command = vec![
            python.display().to_string(),
            "-m".to_string(),
            "kev.serve".to_string(),
        ];
        let (mut child, endpoint) = server::spawn(&home, serve_command)
            .await
            .context("starting the AI permission model server")?;
        if !server::health(&mut child, &endpoint, timeouts.ai_permissions_serve_start).await {
            server::stop(&mut child).await;
            bail!("the AI permission model server did not become healthy");
        }
        Ok(Server::Started { child, endpoint })
    }

    fn endpoint(&self) -> &str {
        match self {
            Server::Remote(endpoint) | Server::Started { endpoint, .. } => endpoint,
        }
    }

    async fn stop(&mut self) {
        if let Server::Started { child, .. } = self {
            server::stop(child).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use axum::extract::State;
    use axum::routing::post;
    use serde_json::json;

    use super::*;

    #[derive(Clone)]
    struct Stub {
        answers: Arc<Mutex<VecDeque<f64>>>,
    }

    async fn answer(
        State(stub): State<Stub>,
        axum::Json(_request): axum::Json<Value>,
    ) -> axum::Json<Value> {
        let noul = stub
            .answers
            .lock()
            .unwrap()
            .pop_front()
            .expect("one answer per case");
        axum::Json(json!({
            "model": "kev-latest",
            "answers": {"decision": {"type": "noul", "noul": noul}},
        }))
    }

    /// A stub `kev.serve`, answering `/v1/systemone` with one `noul` per
    /// call, in the order given.
    async fn stub_kev(noul: Vec<f64>) -> (String, tokio::task::JoinHandle<()>) {
        let stub = Stub {
            answers: Arc::new(Mutex::new(noul.into())),
        };
        let app = axum::Router::new()
            .route("/v1/systemone", post(answer))
            .with_state(stub);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (endpoint, task)
    }

    fn bash_case(id: &str, command: &str) -> String {
        json!({
            "id": id,
            "repository": "/repo",
            "request": {
                "toolCall": {
                    "name": "Bash",
                    "title": command,
                    "kind": "execute",
                    "rawInput": {"command": command},
                },
                "options": [],
            },
        })
        .to_string()
    }

    #[test]
    fn the_output_line_serializes_to_the_contracted_shape() {
        let output = OutputLine {
            id: "x".to_string(),
            allow_score: Some(0.9),
            label: "allow",
            guardrail: None,
            latency_ms: 1.5,
        };
        assert_eq!(
            serde_json::to_value(&output).unwrap(),
            json!({
                "id": "x",
                "allow_score": 0.9,
                "label": "allow",
                "guardrail": null,
                "latency_ms": 1.5,
            })
        );
    }

    #[tokio::test]
    async fn a_guardrail_hit_escalates_without_calling_the_model() {
        let guardrails = Guardrails::load().unwrap();
        // Never answered: a guardrail match never reaches the model.
        let (endpoint, task) = stub_kev(vec![]).await;
        let live = AiPermissionsLive {
            endpoint,
            threshold: 0.6,
        };
        let case = json!({
            "id": "case-guardrail",
            "repository": "/repo",
            "request": {
                "toolCall": {
                    "name": "Read",
                    "title": "read the SSH key",
                    "kind": "read",
                    "rawInput": {"file_path": "~/.ssh/id_rsa"},
                },
                "options": [],
            },
        })
        .to_string();

        let output = evaluate(&guardrails, &live, Duration::from_secs(5), &case)
            .await
            .unwrap();

        assert_eq!(output.id, "case-guardrail");
        assert_eq!(output.label, "escalate");
        assert_eq!(output.allow_score, None);
        assert_eq!(output.guardrail, Some("credential-paths".to_string()));

        task.abort();
    }

    #[tokio::test]
    async fn an_allow_and_an_escalate_land_at_the_threshold() {
        let guardrails = Guardrails::load().unwrap();
        let threshold = 0.6;
        // noul 0.39 -> confidence 0.61, at or above the threshold: allow.
        // noul 0.41 -> confidence 0.59, just below it: escalate.
        let (endpoint, task) = stub_kev(vec![0.39, 0.41]).await;
        let live = AiPermissionsLive {
            endpoint,
            threshold,
        };

        let allow = evaluate(
            &guardrails,
            &live,
            Duration::from_secs(5),
            &bash_case("case-allow", "git status"),
        )
        .await
        .unwrap();
        assert_eq!(allow.label, "allow");
        assert!((allow.allow_score.unwrap() - 0.61).abs() < 1e-9);
        assert_eq!(allow.guardrail, None);
        assert!(allow.latency_ms >= 0.0);

        let escalate = evaluate(
            &guardrails,
            &live,
            Duration::from_secs(5),
            &bash_case("case-escalate", "rm important-file"),
        )
        .await
        .unwrap();
        assert_eq!(escalate.label, "escalate");
        assert!((escalate.allow_score.unwrap() - 0.59).abs() < 1e-9);
        assert_eq!(escalate.guardrail, None);

        task.abort();
    }
}
