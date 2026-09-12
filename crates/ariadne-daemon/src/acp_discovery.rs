//! Runtime discovery for ACP agents and their session configuration catalogs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::process::Command;
use tokio::sync::RwLock;

use ariadne_api::agents::{AcpAgentDto, AcpAgentStatus, AcpCapabilitiesDto, AcpDegradation};
use ariadne_api::models::{EffortDto, ModelDto};
use ariadne_api::sessions::OutsideSessionDto;
use ariadne_client::endpoint::AcpAgentConfig;
use ariadne_store::Store;

use crate::acp::find_config_option;
use crate::acp_rpc::{Incoming, RpcTransport};

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// The ACP commands Ariadne knows without configuration.
const BUILTINS: [(&str, &[&str]); 3] = [
    ("claude-agent-acp", &["claude-agent-acp"]),
    ("codex-acp", &["codex-acp"]),
    ("opencode-acp", &["opencode", "acp"]),
];

#[derive(Clone)]
pub struct AgentRegistry {
    entries: Arc<Vec<RegistryEntry>>,
    results: Arc<RwLock<Vec<Discovery>>>,
    probe_cwd: Arc<PathBuf>,
    /// Where each agent's catalog is kept across restarts, under its version.
    store: Store,
}

#[derive(Clone)]
struct RegistryEntry {
    id: String,
    command: Vec<String>,
    builtin: bool,
    probe: bool,
    /// Why this entry is refused outright, where it is: a configuration
    /// error, permanent until the config changes. A duplicate id is already
    /// another entry's, and only that one may answer for it. A refused entry
    /// is never probed and never resolved, and its rejection carries this
    /// reason.
    refused: Option<String>,
}

/// Why a configured entry is refused, or `None` for one in good standing:
/// its id is empty or carries the catalog's `:` delimiter, or an earlier
/// entry — a built-in, or one configured before it — already holds it.
fn refusal(id: &str, taken: &std::collections::HashSet<String>) -> Option<String> {
    if id.trim().is_empty() {
        return Some("the id is empty; give the agent a stable id".into());
    }
    if id.contains(':') {
        return Some(format!(
            "the id `{id}` contains `:`, which splits a catalog id from its model; choose another id"
        ));
    }
    taken.contains(id).then(|| {
        format!(
            "the id `{id}` is already another agent's — the first entry keeps it; choose another id"
        )
    })
}

#[derive(Clone)]
struct Discovery {
    agent: AcpAgentDto,
    catalog: Catalog,
    /// The version the agent reported in `initialize`: what its catalog is
    /// kept under. An agent that reports none has its catalog read afresh on
    /// every probe, since nothing tells when it changes.
    version: Option<String>,
}

/// What one `session/new` said about an agent's configuration.
#[derive(Clone, Default, Serialize, Deserialize)]
struct Catalog {
    models: Vec<Choice>,
    thought_level: bool,
    efforts: Vec<Choice>,
    default_effort: Option<String>,
}

/// One agent's catalog as the store keeps it: it stands for the same command
/// at the same version, and for nothing else.
#[derive(Clone)]
struct CachedCatalog {
    command: Vec<String>,
    version: String,
    catalog: Catalog,
}

#[derive(Clone, Serialize, Deserialize)]
struct Choice {
    value: String,
    description: Option<String>,
}

impl AgentRegistry {
    /// The registry of a daemon: probes run in `probe_cwd`, and the catalogs
    /// they read are kept in `store`.
    pub fn new(custom: &[AcpAgentConfig], probe_cwd: PathBuf, store: Store) -> Self {
        Self::build(custom, probe_cwd, store, true)
    }

    /// Build the real registry without launching installed agents from tests.
    #[doc(hidden)]
    pub fn test_registry(custom: &[AcpAgentConfig], probe_cwd: PathBuf, store: Store) -> Self {
        Self::build(custom, probe_cwd, store, false)
    }

    fn build(
        custom: &[AcpAgentConfig],
        probe_cwd: PathBuf,
        store: Store,
        probe_builtins: bool,
    ) -> Self {
        // The first holder of an id keeps it: built-ins first, then the
        // configured entries in configuration order.
        let mut taken: std::collections::HashSet<String> =
            BUILTINS.iter().map(|(id, _)| (*id).to_string()).collect();
        let entries = BUILTINS
            .into_iter()
            .map(|(id, command)| RegistryEntry {
                id: id.into(),
                command: command.iter().map(|part| (*part).to_string()).collect(),
                builtin: true,
                probe: probe_builtins,
                refused: None,
            })
            .chain(custom.iter().map(|agent| {
                let refused = refusal(&agent.id, &taken);
                taken.insert(agent.id.clone());
                RegistryEntry {
                    id: agent.id.clone(),
                    command: agent.command.clone(),
                    builtin: false,
                    probe: refused.is_none(),
                    refused,
                }
            }))
            .collect::<Vec<_>>();
        let results = entries.iter().map(not_probed).collect();
        Self {
            entries: Arc::new(entries),
            results: Arc::new(RwLock::new(results)),
            probe_cwd: Arc::new(probe_cwd),
            store,
        }
    }

    /// Discovery as a daemon start runs it: every agent is asked to
    /// `initialize`, and only one whose version has no kept catalog opens a
    /// session to read one.
    pub async fn discover(&self) -> Vec<AcpAgentDto> {
        self.probe_all(false).await
    }

    /// Discovery on demand: every agent opens a session and its catalog is
    /// read again, for a catalog that moved without a new version — a model
    /// configured, or installed locally.
    pub async fn refresh(&self) -> Vec<AcpAgentDto> {
        self.probe_all(true).await
    }

    /// Probe every command concurrently, replace the results as one snapshot,
    /// and keep every catalog read afresh under its agent's version.
    async fn probe_all(&self, reread: bool) -> Vec<AcpAgentDto> {
        let kept = if reread {
            BTreeMap::new()
        } else {
            self.kept_catalogs().await
        };
        let cwd = self.probe_cwd.clone();
        let probes = self.entries.iter().cloned().map(|entry| {
            let cwd = cwd.clone();
            let cached = kept
                .get(&entry.id)
                .filter(|cached| cached.command == entry.command)
                .cloned();
            async move {
                if let Some(reason) = &entry.refused {
                    return rejected(&entry, reason.clone());
                }
                if !entry.probe {
                    return rejected(&entry, "not probed by the test harness".into());
                }
                probe(entry, &cwd, cached).await
            }
        });
        let discovered = join_all(probes).await;

        for result in &discovered {
            let (AcpAgentStatus::Ready, Some(version)) = (result.agent.status, &result.version)
            else {
                continue;
            };
            let unchanged = kept.get(&result.agent.id).is_some_and(|cached| {
                &cached.version == version && cached.command == result.agent.command
            });
            if !unchanged {
                self.keep_catalog(&result.agent, version, &result.catalog)
                    .await;
            }
        }

        let agents = discovered
            .iter()
            .map(|result| result.agent.clone())
            .collect();
        *self.results.write().await = discovered;
        agents
    }

    /// Every catalog the store keeps, by agent id. One the store cannot give
    /// back is read again, so it costs a session, never the agent.
    async fn kept_catalogs(&self) -> BTreeMap<String, CachedCatalog> {
        let rows = match self.store.list_acp_catalogs().await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(error = %error, "reading the kept ACP catalogs failed");
                return BTreeMap::new();
            }
        };
        rows.into_iter()
            .filter_map(|row| {
                let catalog = serde_json::from_str(&row.catalog).ok()?;
                Some((
                    row.agent_id.clone(),
                    CachedCatalog {
                        command: row.command(),
                        version: row.version,
                        catalog,
                    },
                ))
            })
            .collect()
    }

    async fn keep_catalog(&self, agent: &AcpAgentDto, version: &str, catalog: &Catalog) {
        let kept = match serde_json::to_string(catalog) {
            Ok(json) => {
                self.store
                    .put_acp_catalog(&agent.id, &agent.command, version, &json)
                    .await
            }
            Err(error) => Err(ariadne_store::StoreError::Invalid(error.to_string())),
        };
        if let Err(error) = kept {
            tracing::warn!(agent = %agent.id, error = %error, "keeping an ACP agent's catalog failed");
        }
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

    /// The command one registry agent is spawned with, by its stable id —
    /// what the launcher runs for a session pinned to that agent. `None`
    /// where nothing in the registry carries the id. A refused entry — a
    /// duplicate of an earlier entry's — never answers: the id belongs to
    /// its first holder.
    pub fn command_of(&self, id: &str) -> Option<Vec<String>> {
        self.entries
            .iter()
            .find(|entry| entry.id == id && entry.refused.is_none())
            .map(|entry| entry.command.clone())
    }

    /// The cached capabilities of one agent, by id: what the last probe
    /// measured, which is what a launch decision reads. `None` where the id
    /// is not in the registry.
    pub async fn capabilities_of(&self, id: &str) -> Option<AcpCapabilitiesDto> {
        self.results
            .read()
            .await
            .iter()
            .find(|result| result.agent.id == id)
            .map(|result| result.agent.capabilities.clone())
    }

    /// Convert every accepted discovery choice into the shared model catalog.
    pub async fn models(&self) -> Vec<ModelDto> {
        self.results
            .read()
            .await
            .iter()
            .filter(|result| result.agent.status == AcpAgentStatus::Ready)
            .flat_map(|result| {
                let catalog = &result.catalog;
                catalog.models.iter().map(|model| ModelDto {
                    id: format!("{}:{}", result.agent.id, model.value),
                    agent_id: result.agent.id.clone(),
                    description: model.description.clone(),
                    efforts: catalog
                        .efforts
                        .iter()
                        .map(|effort| EffortDto {
                            id: effort.value.clone(),
                            description: effort.description.clone(),
                            default: catalog.default_effort.as_deref()
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
    match &entry.refused {
        Some(reason) => rejected(entry, reason.clone()),
        None => rejected(entry, "discovery has not run".into()),
    }
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
        catalog: Catalog::default(),
        version: None,
    }
}

/// Probe one agent: `initialize` always, and a `session/new` only where
/// `cached` holds no catalog for the version the agent reports.
async fn probe(entry: RegistryEntry, cwd: &Path, cached: Option<CachedCatalog>) -> Discovery {
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
    // The timeout sits inside the probe so a slow agent is still reported
    // with every capability it showed before it stopped answering.
    let result = tokio::time::timeout(
        PROBE_TIMEOUT,
        probe_protocol(
            &entry,
            &mut rpc,
            &mut incoming,
            cwd,
            cached,
            &mut capabilities,
        ),
    )
    .await
    .unwrap_or_else(|_| Err(anyhow!("discovery timed out")));
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
    cached: Option<CachedCatalog>,
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

    let version = initialized
        .pointer("/agentInfo/version")
        .and_then(Value::as_str)
        .map(str::to_string);
    let catalog = match cached.filter(|cached| version.as_deref() == Some(&cached.version)) {
        // `session/new` answered for this command at this version already.
        Some(cached) => {
            capabilities.session_new = true;
            capabilities.model = true;
            capabilities.thought_level = cached.catalog.thought_level;
            cached.catalog
        }
        None => {
            let closes = capability(&advertised["sessionCapabilities"], "close");
            read_catalog(rpc, incoming, cwd, closes, capabilities).await?
        }
    };

    // No prompt is sent: every ACP v1 agent answers `session/prompt`, and a
    // prompt here would be a real model turn, billed on every probe.
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
        catalog,
        version,
    })
}

/// Open a session to read the agent's catalog off it, then close it again
/// where the agent can (`closes`), so it holds nothing for a session nobody
/// will prompt.
async fn read_catalog(
    rpc: &mut RpcTransport,
    incoming: &mut ProbeIncoming,
    cwd: &Path,
    closes: bool,
    capabilities: &mut AcpCapabilitiesDto,
) -> Result<Catalog> {
    let setup = rpc
        .request(
            "session/new",
            json!({"cwd": cwd.display().to_string(), "mcpServers": []}),
            incoming,
        )
        .await
        .context("session/new failed")?;
    let session_id = setup
        .get("sessionId")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("session/new returned no session id"))?;
    capabilities.session_new = true;
    if closes {
        // Best effort: the catalog is read either way, and the process is
        // gone as soon as the probe ends.
        let _ = rpc
            .request("session/close", json!({"sessionId": session_id}), incoming)
            .await;
    }
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
    Ok(Catalog {
        models,
        thought_level: thought.is_some(),
        efforts: thought.map(choices).unwrap_or_default(),
        default_effort: thought
            .and_then(|option| option.get("currentValue"))
            .and_then(Value::as_str)
            .map(str::to_string),
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
        agent_id: agent.id.clone(),
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
