//! Runtime discovery for ACP agents and their session configuration catalogs.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use futures_util::future::join_all;
use serde_json::{Value, json};
use tokio::process::Command;
use tokio::sync::RwLock;

use ariadne_api::agents::{AcpAgentDto, AcpAgentStatus, AcpCapabilitiesDto, AcpDegradation};
use ariadne_api::models::{EffortDto, ModelDto};
use ariadne_api::sessions::OutsideSessionDto;
use ariadne_client::endpoint::AcpAgentConfig;
use ariadne_core::{AgentKind, ModelTier};

use crate::acp::find_config_option;
use crate::acp_rpc::{Incoming, RpcTransport};

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// The ACP commands Ariadne knows without configuration.
const BUILTINS: [(&str, &[&str]); 3] = [
    ("claude-code-acp", &["claude-code-acp"]),
    ("codex-acp", &["codex", "acp"]),
    ("opencode-acp", &["opencode", "acp"]),
];

#[derive(Clone)]
pub struct AgentRegistry {
    entries: Arc<Vec<RegistryEntry>>,
    results: Arc<RwLock<Vec<Discovery>>>,
    probe_cwd: Arc<PathBuf>,
}

#[derive(Clone)]
struct RegistryEntry {
    id: String,
    command: Vec<String>,
    builtin: bool,
    probe: bool,
}

#[derive(Clone)]
struct Discovery {
    agent: AcpAgentDto,
    models: Vec<Choice>,
    efforts: Vec<Choice>,
    default_effort: Option<String>,
}

#[derive(Clone)]
struct Choice {
    value: String,
    description: Option<String>,
}

impl AgentRegistry {
    pub fn new(custom: &[AcpAgentConfig], probe_cwd: PathBuf) -> Self {
        Self::build(custom, probe_cwd, true)
    }

    /// Build the real registry without launching installed agents from tests.
    #[doc(hidden)]
    pub fn test_registry(custom: &[AcpAgentConfig], probe_cwd: PathBuf) -> Self {
        Self::build(custom, probe_cwd, false)
    }

    fn build(custom: &[AcpAgentConfig], probe_cwd: PathBuf, probe_builtins: bool) -> Self {
        let entries = BUILTINS
            .into_iter()
            .map(|(id, command)| RegistryEntry {
                id: id.into(),
                command: command.iter().map(|part| (*part).to_string()).collect(),
                builtin: true,
                probe: probe_builtins,
            })
            .chain(custom.iter().map(|agent| RegistryEntry {
                id: agent.id.clone(),
                command: agent.command.clone(),
                builtin: false,
                probe: true,
            }))
            .collect::<Vec<_>>();
        let results = entries.iter().map(not_probed).collect();
        Self {
            entries: Arc::new(entries),
            results: Arc::new(RwLock::new(results)),
            probe_cwd: Arc::new(probe_cwd),
        }
    }

    /// Probe every command concurrently and replace the cache as one snapshot.
    pub async fn refresh(&self) -> Vec<AcpAgentDto> {
        let cwd = self.probe_cwd.clone();
        let probes = self.entries.iter().cloned().map(|entry| {
            let cwd = cwd.clone();
            async move {
                if !entry.probe {
                    return rejected(&entry, "not probed by the test harness".into());
                }
                match tokio::time::timeout(PROBE_TIMEOUT, probe(entry.clone(), &cwd)).await {
                    Ok(discovery) => discovery,
                    Err(_) => rejected(&entry, "discovery timed out".into()),
                }
            }
        });
        let discovered = join_all(probes).await;
        let agents = discovered
            .iter()
            .map(|result| result.agent.clone())
            .collect();
        *self.results.write().await = discovered;
        agents
    }

    pub async fn agents(&self) -> Vec<AcpAgentDto> {
        self.results
            .read()
            .await
            .iter()
            .map(|result| result.agent.clone())
            .collect()
    }

    /// The stored sessions of every agent the last discovery found ready and
    /// able to list them (`session_list`), asked over `session/list`.
    ///
    /// One short-lived process per agent, the same shape as a probe: the
    /// cached discovery result says who to ask and whether to bother, this
    /// asks each of them, and the child is gone again before it returns. An
    /// agent that fails to answer contributes nothing rather than failing the
    /// whole listing.
    pub async fn stored_sessions(&self) -> Vec<OutsideSessionDto> {
        let cwd = self.probe_cwd.clone();
        let capable: Vec<AcpAgentDto> = self
            .results
            .read()
            .await
            .iter()
            .map(|result| result.agent.clone())
            .filter(|agent| {
                agent.status == AcpAgentStatus::Ready && agent.capabilities.session_list
            })
            .collect();
        let calls = capable.into_iter().map(|agent| {
            let cwd = cwd.clone();
            async move {
                match tokio::time::timeout(PROBE_TIMEOUT, list_stored_sessions(&agent, &cwd)).await
                {
                    Ok(Ok(sessions)) => sessions,
                    Ok(Err(error)) => {
                        tracing::warn!(agent = %agent.id, error = %format!("{error:#}"), "listing an ACP agent's stored sessions failed");
                        Vec::new()
                    }
                    Err(_) => {
                        tracing::warn!(agent = %agent.id, "listing an ACP agent's stored sessions timed out");
                        Vec::new()
                    }
                }
            }
        });
        join_all(calls).await.into_iter().flatten().collect()
    }

    /// Convert every accepted discovery choice into the shared model catalog.
    pub async fn models(&self) -> Vec<ModelDto> {
        self.results
            .read()
            .await
            .iter()
            .filter(|result| result.agent.status == AcpAgentStatus::Ready)
            .flat_map(|result| {
                result.models.iter().map(|model| ModelDto {
                    id: format!("{}:{}", result.agent.id, model.value),
                    agent_id: result.agent.id.clone(),
                    agent_kind: AgentKind::Acp,
                    description: model.description.clone(),
                    tier: ModelTier::Unknown,
                    cost: None,
                    speed: None,
                    best_for: Vec::new(),
                    avoid_for: Vec::new(),
                    efforts: result
                        .efforts
                        .iter()
                        .map(|effort| EffortDto {
                            id: effort.value.clone(),
                            description: effort.description.clone(),
                            default: result.default_effort.as_deref()
                                == Some(effort.value.as_str()),
                        })
                        .collect(),
                    enabled: true,
                })
            })
            .collect()
    }
}

fn not_probed(entry: &RegistryEntry) -> Discovery {
    rejected(entry, "discovery has not run".into())
}

fn rejected(entry: &RegistryEntry, reason: String) -> Discovery {
    rejected_with(entry, AcpCapabilitiesDto::default(), reason)
}

fn rejected_with(
    entry: &RegistryEntry,
    capabilities: AcpCapabilitiesDto,
    reason: String,
) -> Discovery {
    Discovery {
        agent: AcpAgentDto {
            id: entry.id.clone(),
            command: entry.command.clone(),
            builtin: entry.builtin,
            status: AcpAgentStatus::Rejected,
            capabilities,
            degraded: Vec::new(),
            rejection_reason: Some(reason),
        },
        models: Vec::new(),
        efforts: Vec::new(),
        default_effort: None,
    }
}

async fn probe(entry: RegistryEntry, cwd: &Path) -> Discovery {
    let Some((program, args)) = entry.command.split_first() else {
        return rejected(&entry, "command is empty".into());
    };
    let child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(error) => {
            return rejected(
                &entry,
                format!("starting `{}`: {error}", entry.command.join(" ")),
            );
        }
    };
    let mut capabilities = AcpCapabilitiesDto::default();
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return rejected(&entry, "agent did not open stdio".into());
    };
    capabilities.stdio = true;
    let mut rpc = RpcTransport::new(stdout, stdin);
    let mut incoming = ProbeIncoming;
    let result = probe_protocol(&entry, &mut rpc, &mut incoming, cwd, &mut capabilities).await;
    let _ = child.start_kill();
    let _ = child.wait().await;
    match result {
        Ok(discovery) => discovery,
        Err(error) => rejected_with(&entry, capabilities, format!("{error:#}")),
    }
}

async fn probe_protocol(
    entry: &RegistryEntry,
    rpc: &mut RpcTransport,
    incoming: &mut ProbeIncoming,
    cwd: &Path,
    capabilities: &mut AcpCapabilitiesDto,
) -> Result<Discovery> {
    let initialized = rpc
        .request(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {
                    "fs": {"readTextFile": false, "writeTextFile": false},
                    "terminal": false,
                    "session": {"configOptions": {}},
                    "auth": {}
                },
                "clientInfo": {"name": "ariadne-discovery", "version": env!("CARGO_PKG_VERSION")}
            }),
            incoming,
        )
        .await
        .context("initialize failed")?;
    capabilities.protocol_v1 =
        initialized.get("protocolVersion").and_then(Value::as_u64) == Some(1);
    if !capabilities.protocol_v1 {
        bail!("agent did not negotiate ACP version 1");
    }

    let advertised = &initialized["agentCapabilities"];
    capabilities.session_list = capability(advertised, "listSessions")
        || capability(&advertised["sessionCapabilities"], "list");
    capabilities.session_load =
        advertised.get("loadSession").and_then(Value::as_bool) == Some(true);

    let setup = rpc
        .request(
            "session/new",
            json!({"cwd": cwd.display().to_string(), "mcpServers": []}),
            incoming,
        )
        .await
        .context("session/new failed")?;
    capabilities.session_new = true;
    let session_id = setup
        .get("sessionId")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("session/new returned no session id"))?;
    let options = setup
        .get("configOptions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let model = find_config_option(&options, &["model"], &["model"]);
    capabilities.model = model.is_some();
    let Some(model) = model else {
        bail!("session/new returned no model option");
    };
    let models = choices(model);
    if models.is_empty() {
        bail!("the model option has no choices or current value");
    }
    let thought = find_config_option(
        &options,
        &["thought_level"],
        &["effort", "reasoning", "thought_level"],
    );
    capabilities.thought_level = thought.is_some();
    let efforts = thought.map(choices).unwrap_or_default();
    let default_effort = thought
        .and_then(|option| option.get("currentValue"))
        .and_then(Value::as_str)
        .map(str::to_string);

    rpc.request(
        "session/prompt",
        json!({"sessionId": session_id, "prompt": [{"type": "text", "text": ""}]}),
        incoming,
    )
    .await
    .context("session/prompt failed")?;
    capabilities.session_prompt = true;

    let mut degraded = Vec::new();
    if !capabilities.thought_level {
        degraded.push(AcpDegradation::NoEfforts);
    }
    if !capabilities.session_list {
        degraded.push(AcpDegradation::NoAdoption);
    }
    if !capabilities.session_load {
        degraded.push(AcpDegradation::NoRestartResume);
    }
    Ok(Discovery {
        agent: AcpAgentDto {
            id: entry.id.clone(),
            command: entry.command.clone(),
            builtin: entry.builtin,
            status: AcpAgentStatus::Ready,
            capabilities: capabilities.clone(),
            degraded,
            rejection_reason: None,
        },
        models,
        efforts,
        default_effort,
    })
}

/// Ask one agent for its stored sessions: spawn it, `initialize`, `session/list`,
/// then kill it — the same one-shot shape as [`probe`], for a call that has
/// no session to keep open.
async fn list_stored_sessions(agent: &AcpAgentDto, cwd: &Path) -> Result<Vec<OutsideSessionDto>> {
    let Some((program, args)) = agent.command.split_first() else {
        bail!("command is empty");
    };
    let mut child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("starting `{}`", agent.command.join(" ")))?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        bail!("agent did not open stdio");
    };
    let mut rpc = RpcTransport::new(stdout, stdin);
    let mut incoming = ProbeIncoming;
    let result = sessions_over_rpc(agent, &mut rpc, &mut incoming).await;
    let _ = child.start_kill();
    let _ = child.wait().await;
    result
}

async fn sessions_over_rpc(
    agent: &AcpAgentDto,
    rpc: &mut RpcTransport,
    incoming: &mut ProbeIncoming,
) -> Result<Vec<OutsideSessionDto>> {
    rpc.request(
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientCapabilities": {
                "fs": {"readTextFile": false, "writeTextFile": false},
                "terminal": false,
                "session": {"configOptions": {}},
                "auth": {}
            },
            "clientInfo": {"name": "ariadne-discovery", "version": env!("CARGO_PKG_VERSION")}
        }),
        incoming,
    )
    .await
    .context("initialize failed")?;
    let listed = rpc
        .request("session/list", json!({}), incoming)
        .await
        .context("session/list failed")?;
    Ok(listed
        .get("sessions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|session| stored_session(agent, session))
        .collect())
}

/// One `session/list` entry as an outside session, or `None` where it names
/// no session id — the one field there is nothing to show without.
fn stored_session(agent: &AcpAgentDto, session: &Value) -> Option<OutsideSessionDto> {
    let internal_session_id = session
        .get("sessionId")
        .and_then(Value::as_str)?
        .to_string();
    Some(OutsideSessionDto {
        agent_kind: AgentKind::Acp,
        agent_id: Some(agent.id.clone()),
        internal_session_id,
        working_directory: session
            .get("cwd")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        last_activity_at: session
            .get("updatedAt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        first_prompt: session
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

fn capability(value: &Value, name: &str) -> bool {
    value
        .get(name)
        .is_some_and(|value| value.as_bool() == Some(true) || value.is_object())
}

fn choices(option: &Value) -> Vec<Choice> {
    let mut choices = option
        .get("options")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|choice| match choice {
            Value::String(value) => Some(Choice {
                value: value.clone(),
                description: None,
            }),
            Value::Object(_) => choice
                .get("value")
                .and_then(Value::as_str)
                .map(|value| Choice {
                    value: value.to_string(),
                    description: choice
                        .get("description")
                        .or_else(|| choice.get("name"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                }),
            _ => None,
        })
        .collect::<Vec<_>>();
    if choices.is_empty()
        && let Some(current) = option.get("currentValue").and_then(Value::as_str)
    {
        choices.push(Choice {
            value: current.to_string(),
            description: None,
        });
    }
    choices
}

struct ProbeIncoming;

impl Incoming for ProbeIncoming {
    async fn handle(&mut self, message: Value) -> Result<Option<Value>> {
        Ok(message.get("id").map(|id| {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": "method not supported"},
            })
        }))
    }
}
