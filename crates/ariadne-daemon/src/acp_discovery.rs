//! Runtime discovery for ACP agents and their session configuration catalogs.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use agent_client_protocol::schema::{ProtocolVersion, v1};
use agent_client_protocol::{Agent, Client, ConnectionTo};
use anyhow::{Context, Result, anyhow, bail};
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::sync::RwLock;

use ariadne_api::agents::{
    AcpAgentDto, AcpAgentSource, AcpAgentStatus, AcpCapabilitiesDto, AcpDegradation,
};
use ariadne_api::models::{EffortDto, ModelDto};
use ariadne_api::sessions::OutsideSessionDto;
use ariadne_client::endpoint::AcpAgentConfig;
use ariadne_store::Store;

use crate::acp::{apply_agent_launch_environment, find_config_option};
use crate::acp_calls::call;
use crate::acp_index;
use crate::acp_transport::pipes;
use crate::timeouts::Timeouts;

/// The snapshot of the published ACP registry index Ariadne ships, vendored
/// under `acp-registry/` on the date its README records. It says what every
/// ACP agent is run by; the daemon's `PATH` says which of them is here.
pub const SHIPPED_INDEX: &str = include_str!("../acp-registry/registry.json");

/// Update this date with the vendored snapshot and its README.
const SHIPPED_INDEX_DATE: &str = "2026-09-23T00:00:00Z";

#[derive(Clone)]
pub struct AgentRegistry {
    entries: Arc<std::sync::RwLock<Vec<RegistryEntry>>>,
    custom: Arc<Vec<AcpAgentConfig>>,
    path: Arc<OsString>,
    index: Arc<str>,
    discovery: Arc<tokio::sync::Mutex<()>>,
    results: Arc<RwLock<Vec<Discovery>>>,
    probe_cwd: Arc<PathBuf>,
    /// Where each agent's catalog is kept across restarts, under its version.
    store: Store,
    timeouts: Timeouts,
}

#[derive(Clone)]
struct RegistryEntry {
    id: String,
    command: Vec<String>,
    /// The file a probe starts: the one discovery found on the `PATH` it
    /// searched, or the program of a configured command.
    program: PathBuf,
    source: AcpAgentSource,
    /// Whether the `PATH` answered for this entry under the name of its
    /// package rather than a name the agent is known by. A package name is
    /// nobody's in particular, so a probe that shows the program is not an
    /// ACP agent takes the entry off the registry rather than listing it.
    named_by_package: bool,
    /// Why this entry is refused outright, where it is: a configuration
    /// error, permanent until the config changes. A duplicate id is already
    /// another entry's, and only that one may answer for it. A refused entry
    /// is never probed and never resolved, and its rejection carries this
    /// reason.
    refused: Option<String>,
}

/// Why an entry is refused, or `None` for one in good standing: its id is
/// empty or carries the catalog's `:` delimiter, or an entry before it
/// already holds it. The rule is the id's, not the config's, so an index that
/// names an agent Ariadne could never pin is refused the same way.
///
/// The ids an index entry takes are counted apart from the configured ones:
/// a configured entry replaces the agent of a discovered id rather than
/// duplicating it.
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
    /// Whether the probe showed the program is no ACP agent at all: it never
    /// negotiated the protocol, and it did not merely run out of time. A
    /// probe that got that far answered for the agent, whatever it failed at
    /// afterwards.
    refuted: bool,
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
    /// The registry of a daemon: every agent of the shipped index that `path`
    /// holds, and the configured agents over them. Probes run in `probe_cwd`,
    /// and the catalogs they read are kept in `store`.
    pub fn new(custom: &[AcpAgentConfig], probe_cwd: PathBuf, store: Store, path: &OsStr) -> Self {
        Self::build(custom, probe_cwd, store, path, SHIPPED_INDEX)
    }

    /// The same registry over an index the caller writes, so a test says
    /// which agents there are to find rather than depending on what the
    /// shipped snapshot names today.
    #[doc(hidden)]
    pub fn test_registry(
        custom: &[AcpAgentConfig],
        probe_cwd: PathBuf,
        store: Store,
        path: &OsStr,
        index: &str,
    ) -> Self {
        Self::build(custom, probe_cwd, store, path, index)
    }

    fn build(
        custom: &[AcpAgentConfig],
        probe_cwd: PathBuf,
        store: Store,
        path: &OsStr,
        index: &str,
    ) -> Self {
        let entries = Self::entries(custom, path, index);
        let results = entries.iter().map(not_probed).collect();
        Self {
            entries: Arc::new(std::sync::RwLock::new(entries)),
            custom: Arc::new(custom.to_vec()),
            path: Arc::new(path.to_os_string()),
            index: Arc::from(index),
            discovery: Arc::default(),
            results: Arc::new(RwLock::new(results)),
            probe_cwd: Arc::new(probe_cwd),
            store,
            timeouts: Timeouts::default(),
        }
    }

    fn entries(custom: &[AcpAgentConfig], path: &OsStr, index: &str) -> Vec<RegistryEntry> {
        let mut found_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut entries: Vec<RegistryEntry> = acp_index::installed(index, path)
            .into_iter()
            .map(|found| {
                let refused = refusal(&found.id, &found_ids);
                found_ids.insert(found.id.clone());
                RegistryEntry {
                    id: found.id,
                    command: found.command,
                    program: found.program,
                    source: AcpAgentSource::Registry,
                    named_by_package: found.named_by_package,
                    refused,
                }
            })
            .collect();
        // A configured entry with a discovered id replaces it, wherever the
        // discovered agent stands: the user's command is the one to run.
        // Every other id is kept by the first entry that holds it, in
        // configuration order.
        let mut taken: std::collections::HashSet<String> = std::collections::HashSet::new();
        for agent in custom {
            let refused = refusal(&agent.id, &taken);
            taken.insert(agent.id.clone());
            let replaces = refused.is_none();
            let entry = RegistryEntry {
                id: agent.id.clone(),
                command: agent.command.clone(),
                program: agent.command.first().cloned().unwrap_or_default().into(),
                source: AcpAgentSource::Config,
                named_by_package: false,
                refused,
            };
            let discovered = entries
                .iter_mut()
                .find(|discovered| discovered.id == agent.id)
                .filter(|_| replaces);
            match discovered {
                Some(discovered) => *discovered = entry,
                None => entries.push(entry),
            }
        }
        entries
    }

    /// The same registry, probing and listing agents as long as `timeouts`
    /// says.
    pub fn with_timeouts(mut self, timeouts: Timeouts) -> Self {
        self.timeouts = timeouts;
        self
    }

    /// Discovery as a daemon start runs it: every agent is asked to
    /// `initialize`, and only one whose version has no kept catalog opens a
    /// session to read one.
    pub async fn discover(&self) -> Vec<AcpAgentDto> {
        let _guard = self.discovery.lock().await;
        self.probe_all(false, None).await
    }

    /// Discovery on demand: every agent opens a session and its catalog is
    /// read again, for a catalog that moved without a new version — a model
    /// configured, or installed locally.
    pub async fn refresh(&self) -> Vec<AcpAgentDto> {
        let _guard = self.discovery.lock().await;
        self.probe_all(true, None).await
    }

    /// Download and keep an accepted index before searching PATH and probing.
    pub(crate) async fn download_and_refresh(&self, url: &str) -> Vec<AcpAgentDto> {
        let _guard = self.discovery.lock().await;
        let index = match self.download_index(url).await {
            Ok(index) => Some(index),
            Err(error) => {
                tracing::warn!(error = %error, "downloading the ACP registry index failed");
                None
            }
        };
        self.probe_all(true, index.as_deref()).await
    }

    async fn download_index(&self, url: &str) -> Result<String> {
        let bytes = reqwest::Client::builder()
            .timeout(self.timeouts.registry_download)
            .build()?
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        let document = String::from_utf8(bytes.to_vec())?;
        acp_index::validate(&document)?;
        self.store.put_acp_registry_index(url, &document).await?;
        Ok(document)
    }

    /// A start reads the kept index without contacting its source.
    async fn selected_index(&self) -> String {
        let kept = match self.store.acp_registry_index().await {
            Ok(kept) => kept,
            Err(error) => {
                tracing::warn!(error = %error, "reading the kept ACP registry index failed");
                None
            }
        };
        if let Some(kept) = kept {
            let fetched = chrono::DateTime::parse_from_rfc3339(&kept.fetched_at);
            let snapshot = chrono::DateTime::parse_from_rfc3339(SHIPPED_INDEX_DATE)
                .expect("the shipped index date is valid");
            if fetched.is_ok_and(|fetched| fetched > snapshot) {
                match acp_index::validate(&kept.document) {
                    Ok(()) => return kept.document,
                    Err(error) => {
                        tracing::warn!(error = %error, "reading the kept ACP registry index failed");
                    }
                }
            }
        }
        self.index.to_string()
    }

    /// Probe every command concurrently, replace the results as one snapshot,
    /// and keep every catalog read afresh under its agent's version.
    async fn probe_all(&self, reread: bool, downloaded: Option<&str>) -> Vec<AcpAgentDto> {
        let index = match downloaded {
            Some(index) => index.to_string(),
            None => self.selected_index().await,
        };
        let entries = Self::entries(&self.custom, &self.path, &index);
        // Every kept catalog is on hand, a refresh included: a refresh reads
        // the catalogs again rather than trusting these, but a probe that
        // runs out of time still falls back on the one it did not replace.
        let kept = self.kept_catalogs().await;
        let cwd = self.probe_cwd.clone();
        let probes = entries.iter().cloned().map(|entry| {
            let cwd = cwd.clone();
            let cached = kept
                .get(&entry.id)
                .filter(|cached| cached.command == entry.command)
                .cloned();
            async move {
                if let Some(reason) = &entry.refused {
                    return rejected(&entry, reason.clone());
                }
                probe(entry, &cwd, cached, reread, self.timeouts.probe).await
            }
        });
        let discovered = join_all(probes).await;
        // A name only a package gives is another program's as easily as the
        // agent's. One that answered for nothing is that other program, and
        // the entry stands for nothing here: the registry holds it no more
        // than it holds an agent the `PATH` never had.
        let (entries, discovered): (Vec<RegistryEntry>, Vec<Discovery>) = entries
            .into_iter()
            .zip(discovered)
            .filter(|(entry, result)| {
                let mistaken = entry.named_by_package && result.refuted;
                if mistaken {
                    tracing::info!(
                        agent = entry.id,
                        command = entry.command.join(" "),
                        program = %entry.program.display(),
                        reason = result.agent.rejection_reason.as_deref().unwrap_or_default(),
                        "a program under the package name of an agent is not that agent; it is registered as nothing"
                    );
                }
                !mistaken
            })
            .unzip();

        for result in &discovered {
            let (AcpAgentStatus::Ready, Some(version)) = (result.agent.status, &result.version)
            else {
                continue;
            };
            let unchanged = !reread
                && kept.get(&result.agent.id).is_some_and(|cached| {
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
        let mut results = self.results.write().await;
        *self.entries.write().unwrap() = entries;
        *results = discovered;
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
    pub(crate) async fn stored_sessions(&self) -> Vec<OutsideSessionDto> {
        let cwd = self.probe_cwd.clone();
        let capable: Vec<(AcpAgentDto, PathBuf)> = self
            .results
            .read()
            .await
            .iter()
            .map(|result| result.agent.clone())
            .filter(|agent| {
                agent.status == AcpAgentStatus::Ready && agent.capabilities.session_list
            })
            .filter_map(|agent| {
                let program = self.program_of(&agent.id)?;
                Some((agent, program))
            })
            .collect();
        let calls = capable.into_iter().map(|(agent, program)| {
            let cwd = cwd.clone();
            async move {
                // One budget over every page of the agent, and the pages that
                // arrived inside it are kept whatever ended the listing.
                let mut sessions = Vec::new();
                match tokio::time::timeout(
                    self.timeouts.probe,
                    list_stored_sessions(&agent, &program, &cwd, &mut sessions),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        tracing::warn!(agent = %agent.id, error = %format!("{error:#}"), "listing an ACP agent's stored sessions failed");
                    }
                    Err(_) => {
                        tracing::warn!(agent = %agent.id, "listing an ACP agent's stored sessions timed out");
                    }
                }
                sessions
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
        self.entry_of(id).map(|entry| entry.command.clone())
    }

    /// The file a probe of that agent starts, by its stable id.
    fn program_of(&self, id: &str) -> Option<PathBuf> {
        self.entry_of(id).map(|entry| entry.program.clone())
    }

    fn entry_of(&self, id: &str) -> Option<RegistryEntry> {
        self.entries
            .read()
            .unwrap()
            .iter()
            .find(|entry| entry.id == id && entry.refused.is_none())
            .cloned()
    }

    /// The cached capabilities of one agent, by id: what the last probe
    /// measured, which is what a launch decision reads. `None` where the id
    /// is not in the registry.
    pub(crate) async fn capabilities_of(&self, id: &str) -> Option<AcpCapabilitiesDto> {
        self.results
            .read()
            .await
            .iter()
            .find(|result| result.agent.id == id)
            .map(|result| result.agent.capabilities.clone())
    }

    /// Convert every accepted discovery choice into the shared model catalog.
    /// Every model the registry knows of: an agent's own, as its last
    /// answer gave them. A ready agent has just said what it offers, and one
    /// whose probe ran out of time carries the catalog the store kept for
    /// its command — the models it offered when it last answered, which
    /// nothing has taken back. An agent that answered and was rejected has
    /// no catalog, and offers nothing.
    pub(crate) async fn models(&self) -> Vec<ModelDto> {
        self.results
            .read()
            .await
            .iter()
            .filter(|result| !result.catalog.models.is_empty())
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
                    rank: None,
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
    rejected_with(entry, AcpCapabilitiesDto::default(), reason, false)
}

fn rejected_with(
    entry: &RegistryEntry,
    capabilities: AcpCapabilitiesDto,
    reason: String,
    refuted: bool,
) -> Discovery {
    Discovery {
        agent: AcpAgentDto {
            id: entry.id.clone(),
            command: entry.command.clone(),
            source: entry.source,
            status: AcpAgentStatus::Rejected,
            capabilities,
            degraded: Vec::new(),
            rejection_reason: Some(reason),
        },
        catalog: Catalog::default(),
        version: None,
        refuted,
    }
}

/// Probe one agent: `initialize` always, and a `session/new` only where
/// `cached` holds no catalog for the version the agent reports — or where
/// `reread` asks for the catalog to be read again whatever is kept.
///
/// A kept catalog is the agent's last word on its models, so a probe that
/// runs out of time hands it back rather than nothing: what the agent
/// offers is replaced by an answer, never by silence.
async fn probe(
    entry: RegistryEntry,
    cwd: &Path,
    cached: Option<CachedCatalog>,
    reread: bool,
    timeout: Duration,
) -> Discovery {
    let Some((_, args)) = entry.command.split_first() else {
        return rejected(&entry, "command is empty".into());
    };
    let mut command = Command::new(&entry.program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    apply_agent_launch_environment(&mut command, &entry.id);
    let child = command.spawn();
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
    // A probe answers nothing: the SDK rejects a request it has no handler
    // for and ignores a notification, which is what `ProbeIncoming` did.
    //
    // The timeout sits inside the probe so a slow agent is still reported
    // with every capability it showed before it stopped answering.
    let mut discovered = None;
    let result = tokio::time::timeout(
        timeout,
        Client
            .builder()
            .name("ariadne-discovery")
            .connect_with(pipes(stdin, stdout), async |cx| {
                let shortcut = cached.clone().filter(|_| !reread);
                discovered =
                    Some(probe_protocol(&entry, &cx, cwd, shortcut, &mut capabilities).await);
                Ok(())
            }),
    )
    .await;
    let (result, timed_out) = match (result, discovered) {
        (_, Some(discovery)) => (discovery, false),
        (Ok(Err(error)), None) => (Err(anyhow!("ACP connection failed: {error}")), false),
        (Ok(Ok(())), None) => (
            Err(anyhow!("the agent closed its stdio during discovery")),
            false,
        ),
        (Err(_), None) => (Err(anyhow!("discovery timed out")), true),
    };
    let _ = child.start_kill();
    let _ = child.wait().await;
    match result {
        Ok(discovery) => discovery,
        Err(error) => {
            // Nothing spoke ACP here, and time is not what it ran out of:
            // whatever this program is, it is not the agent.
            let refuted = !capabilities.protocol_v1 && !timed_out;
            let mut rejection = rejected_with(&entry, capabilities, format!("{error:#}"), refuted);
            // A probe out of time said nothing about the models. The catalog
            // the store kept for this very command is still the agent's last
            // word, and it stays on offer until an answer replaces it: a busy
            // machine keeps the models its goals are staffed from.
            if timed_out && let Some(kept) = cached {
                rejection.catalog = kept.catalog;
                rejection.version = Some(kept.version);
            }
            rejection
        }
    }
}

async fn probe_protocol(
    entry: &RegistryEntry,
    cx: &ConnectionTo<Agent>,
    cwd: &Path,
    cached: Option<CachedCatalog>,
    capabilities: &mut AcpCapabilitiesDto,
) -> Result<Discovery> {
    let initialized = call(cx, "initialize", initialize())
        .await
        .context("initialize failed")?;
    capabilities.protocol_v1 = initialized.protocol_version == ProtocolVersion::V1;
    if !capabilities.protocol_v1 {
        bail!("agent did not negotiate ACP version 1");
    }

    let advertised = &initialized.agent_capabilities;
    capabilities.session_list = advertised.session_capabilities.list.is_some();
    capabilities.session_load = advertised.load_session;

    let version = initialized
        .agent_info
        .as_ref()
        .map(|info| info.version.clone());
    let catalog = match cached.filter(|cached| version.as_deref() == Some(&cached.version)) {
        // `session/new` answered for this command at this version already.
        Some(cached) => {
            capabilities.session_new = true;
            capabilities.model = true;
            capabilities.thought_level = cached.catalog.thought_level;
            cached.catalog
        }
        None => {
            let closes = advertised.session_capabilities.close.is_some();
            read_catalog(cx, cwd, closes, capabilities).await?
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
            source: entry.source,
            status: AcpAgentStatus::Ready,
            capabilities: capabilities.clone(),
            degraded,
            rejection_reason: None,
        },
        catalog,
        version,
        refuted: false,
    })
}

/// Open a session to read the agent's catalog off it, then close it again
/// where the agent can (`closes`), so it holds nothing for a session nobody
/// will prompt.
async fn read_catalog(
    cx: &ConnectionTo<Agent>,
    cwd: &Path,
    closes: bool,
    capabilities: &mut AcpCapabilitiesDto,
) -> Result<Catalog> {
    let setup = call(cx, "session/new", v1::NewSessionRequest::new(cwd))
        .await
        .context("session/new failed")?;
    let session_id = setup.session_id.clone();
    capabilities.session_new = true;
    if closes {
        // Best effort: the catalog is read either way, and the process is
        // gone as soon as the probe ends.
        let _ = call(
            cx,
            "session/close",
            v1::CloseSessionRequest::new(session_id.to_string()),
        )
        .await;
    }
    let options = setup.config_options.unwrap_or_default();
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
            .and_then(crate::acp::current_value)
            .map(str::to_string),
    })
}

/// What a probe tells an agent it is, on `initialize`.
///
/// A probe reads the catalog and the stored sessions and prompts nothing, so
/// it advertises less than a live session does: the config options it reads
/// the model and effort from, and no compaction, which only a running turn
/// reports.
fn initialize() -> v1::InitializeRequest {
    let capabilities = v1::ClientCapabilities::new().terminal(false).session(
        v1::ClientSessionCapabilities::new()
            .config_options(v1::SessionConfigOptionsCapabilities::new()),
    );
    v1::InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(capabilities)
        .client_info(v1::Implementation::new(
            "ariadne-discovery",
            env!("CARGO_PKG_VERSION"),
        ))
}

/// Ask one agent for its stored sessions: spawn it, `initialize`, `session/list`
/// page after page, then kill it — the same one-shot shape as [`probe`], for a
/// call that has no session to keep open. Each page lands in `sessions` as it
/// arrives, so a caller that gives up on the rest still holds what came.
async fn list_stored_sessions(
    agent: &AcpAgentDto,
    program: &Path,
    cwd: &Path,
    sessions: &mut Vec<OutsideSessionDto>,
) -> Result<()> {
    let Some((_, args)) = agent.command.split_first() else {
        bail!("command is empty");
    };
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    apply_agent_launch_environment(&mut command, &agent.id);
    let mut child = command
        .spawn()
        .with_context(|| format!("starting `{}`", agent.command.join(" ")))?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        bail!("agent did not open stdio");
    };
    let mut listed = None;
    let connected = Client
        .builder()
        .name("ariadne-discovery")
        .connect_with(pipes(stdin, stdout), async |cx| {
            listed = Some(sessions_over_rpc(agent, &cx, sessions).await);
            Ok(())
        })
        .await;
    let _ = child.start_kill();
    let _ = child.wait().await;
    match (connected, listed) {
        (_, Some(result)) => result,
        (Err(error), None) => Err(anyhow!("ACP connection failed: {error}")),
        (Ok(()), None) => Err(anyhow!("the agent closed its stdio while listing sessions")),
    }
}

/// `initialize`, then `session/list` with each `nextCursor` the agent answers
/// with, until a reply carries none.
async fn sessions_over_rpc(
    agent: &AcpAgentDto,
    cx: &ConnectionTo<Agent>,
    sessions: &mut Vec<OutsideSessionDto>,
) -> Result<()> {
    call(cx, "initialize", initialize())
        .await
        .context("initialize failed")?;
    let mut cursor: Option<String> = None;
    loop {
        let request = v1::ListSessionsRequest::new().cursor(cursor.clone());
        let listed = call(cx, "session/list", request)
            .await
            .context("session/list failed")?;
        sessions.extend(
            listed
                .sessions
                .iter()
                .map(|session| stored_session(agent, session)),
        );
        cursor = listed.next_cursor;
        if cursor.is_none() {
            return Ok(());
        }
    }
}

/// One `session/list` entry as an outside session, or `None` where it names
/// no session id — the one field there is nothing to show without.
fn stored_session(agent: &AcpAgentDto, session: &v1::SessionInfo) -> OutsideSessionDto {
    OutsideSessionDto {
        agent_id: agent.id.clone(),
        internal_session_id: session.session_id.0.to_string(),
        working_directory: session.cwd.display().to_string(),
        last_activity_at: session.updated_at.clone().unwrap_or_default(),
        first_prompt: session.title.clone().unwrap_or_default(),
    }
}

/// Every value a select option offers, grouped or not, and its current value
/// where it offers no list at all.
fn choices(option: &v1::SessionConfigOption) -> Vec<Choice> {
    let v1::SessionConfigKind::Select(select) = &option.kind else {
        return Vec::new();
    };
    let listed: Vec<&v1::SessionConfigSelectOption> = match &select.options {
        v1::SessionConfigSelectOptions::Ungrouped(options) => options.iter().collect(),
        // A grouped list is the same values under headings the agent draws
        // with; Ariadne pins by value, so the headings are dropped.
        v1::SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .collect(),
        _ => Vec::new(),
    };
    let mut choices: Vec<Choice> = listed
        .into_iter()
        .map(|choice| Choice {
            value: choice.value.0.to_string(),
            description: choice
                .description
                .clone()
                .or_else(|| Some(choice.name.clone())),
        })
        .collect();
    if choices.is_empty() {
        choices.push(Choice {
            value: select.current_value.0.to_string(),
            description: None,
        });
    }
    choices
}
