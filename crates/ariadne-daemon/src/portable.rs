//! Portable configuration, live policy reads, and maintenance admission (037).
use std::collections::{BTreeMap, HashSet};
use std::sync::{Mutex, RwLock};

use anyhow::{Context, Result, bail};
use ariadne_api::hosts::{ConfigSyncDto, PortableAgentDto, PortableConfigDto, SyncState};
use ariadne_core::{SessionStatus, TaskStatus};
use ariadne_store::{AgentSession, SessionFilter, Store, TaskFilter};
use tokio::sync::RwLock as AsyncRwLock;

use crate::config::Config;

const FILE: &str = "portable-config.json";
type Reload = Box<dyn Fn(&str) -> Result<()> + Send + Sync>;

#[derive(Default)]
pub struct PortableRuntime {
    live: RwLock<Option<PortableConfigDto>>,
    maintenance: Mutex<Option<Drain>>,
    error: Mutex<Option<String>>,
    external: Mutex<bool>,
    reload: Mutex<Option<Reload>>,
    /// Admission and maintenance snapshots use the same lock.
    pub(crate) admission: AsyncRwLock<()>,
}

impl std::fmt::Debug for PortableRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PortableRuntime").finish_non_exhaustive()
    }
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Drain {
    tasks: HashSet<String>,
    goals: HashSet<String>,
    pulls: HashSet<String>,
    sessions: HashSet<String>,
}

impl Config {
    /// Read current policy, including changes accepted since startup.
    pub fn live_settings(&self) -> PortableConfigDto {
        self.portable
            .live
            .read()
            .unwrap()
            .clone()
            .unwrap_or_else(|| self.startup_settings())
    }

    fn startup_settings(&self) -> PortableConfigDto {
        PortableConfigDto {
            delete_merged_branches: self.delete_merged_branches,
            delete_merged_worktrees: self.delete_merged_worktrees,
            auto_switch: self.auto_switch,
            exhausted_patterns: self.exhausted_patterns.clone(),
            log_filter: self.log_filter.clone(),
            telemetry_samples_days: self.telemetry_samples_days,
            telemetry_logs_days: self.telemetry_logs_days,
            telemetry_rollups_days: self.telemetry_rollups_days,
            acp_registry_url: self.acp_registry_url.clone(),
            acp_agents: self
                .acp_agents
                .iter()
                .map(|agent| PortableAgentDto {
                    id: agent.id.clone(),
                    command: agent.command.clone(),
                })
                .collect(),
            agent_flags: BTreeMap::new(),
        }
    }
}

impl PortableRuntime {
    /// Install the process's tracing reload handle.
    pub fn set_log_reload(&self, reload: impl Fn(&str) -> Result<()> + Send + Sync + 'static) {
        *self.reload.lock().unwrap() = Some(Box::new(reload));
    }

    pub(crate) fn allows_task(&self, id: &str) -> bool {
        self.maintenance
            .lock()
            .unwrap()
            .as_ref()
            .is_none_or(|drain| drain.tasks.contains(id))
    }

    pub(crate) fn allows_goal(&self, id: &str) -> bool {
        self.maintenance
            .lock()
            .unwrap()
            .as_ref()
            .is_none_or(|drain| drain.goals.contains(id))
    }

    pub(crate) fn allows_pull(&self, id: &str, task: Option<&str>) -> bool {
        self.maintenance
            .lock()
            .unwrap()
            .as_ref()
            .is_none_or(|drain| {
                drain.pulls.contains(id) || task.is_some_and(|id| drain.tasks.contains(id))
            })
    }

    pub(crate) fn allows_session(&self, session: &AgentSession) -> bool {
        self.maintenance
            .lock()
            .unwrap()
            .as_ref()
            .is_none_or(|drain| {
                if let Some(id) = &session.task_id {
                    drain.tasks.contains(id)
                } else {
                    drain.sessions.contains(&session.id)
                        || session
                            .goal_id
                            .as_ref()
                            .is_some_and(|id| drain.goals.contains(id))
                        || session
                            .pull_request_id
                            .as_ref()
                            .is_some_and(|id| drain.pulls.contains(id))
                }
            })
    }

    /// Report the current worker configuration and maintenance result.
    pub async fn status(&self, cfg: &Config, store: &Store) -> Result<ConfigSyncDto> {
        let mut configuration = cfg.live_settings();
        for row in store.list_agent_configs().await? {
            configuration
                .agent_flags
                .insert(row.agent_id.clone(), row.extra_flags());
        }
        let error = self.error.lock().unwrap().clone();
        let state = if error.is_some() {
            SyncState::Failed
        } else if self.maintenance.lock().unwrap().is_some() {
            SyncState::PendingRestart
        } else {
            SyncState::InSync
        };
        Ok(ConfigSyncDto {
            state,
            error,
            configuration,
        })
    }

    /// Persist accepted settings before publishing them to live consumers.
    pub async fn apply(
        &self,
        cfg: &Config,
        store: &Store,
        mut desired: PortableConfigDto,
    ) -> Result<ConfigSyncDto> {
        let _admission = self.admission.write().await;
        validate(&desired)?;
        let current = cfg.live_settings();
        for agent in &mut desired.acp_agents {
            if let Some(worker) = current
                .acp_agents
                .iter()
                .find(|worker| worker.id == agent.id)
            {
                agent.command = worker.command.clone();
            } else if agent.command.iter().any(|arg| machine_path(arg)) {
                bail!(
                    "agent {} needs a worker command mapping for its source path",
                    agent.id
                );
            } else if ariadne_core::probe::which(
                &std::env::var_os("PATH").unwrap_or_default(),
                &agent.command[0],
            )
            .is_none()
            {
                bail!(
                    "agent {} command {} is unavailable on the worker PATH",
                    agent.id,
                    agent.command[0]
                );
            }
        }
        for (id, flags) in &desired.agent_flags {
            if flags.iter().any(|arg| machine_path(arg)) {
                bail!("agent {id} flags need a worker path mapping");
            }
        }
        for worker in &current.acp_agents {
            if !desired.acp_agents.iter().any(|agent| agent.id == worker.id) {
                desired.acp_agents.push(worker.clone());
            }
        }
        let startup = cfg.startup_settings();
        let restart = desired.acp_agents != startup.acp_agents
            || desired.acp_registry_url != startup.acp_registry_url;
        let mut drain = Drain::default();
        if restart && self.maintenance.lock().unwrap().is_none() {
            drain = Self::snapshot(store).await?;
        }
        // Validate all input before changing either persistent or live state.
        let bytes = serde_json::to_vec_pretty(&desired)?;
        let mut staged = tempfile::NamedTempFile::new_in(&cfg.root)?;
        std::io::Write::write_all(&mut staged, &bytes)?;
        staged.as_file().sync_all()?;
        if let Some(reload) = self.reload.lock().unwrap().as_ref() {
            reload(&desired.log_filter)?;
        }
        staged
            .persist(cfg.root.join(FILE))
            .context("saving portable configuration")?;
        for (id, flags) in &desired.agent_flags {
            store.update_agent_config(id, flags.clone()).await?;
        }
        *self.live.write().unwrap() = Some(desired);
        *self.error.lock().unwrap() = None;
        {
            let mut maintenance = self.maintenance.lock().unwrap();
            if restart {
                if maintenance.is_none() {
                    *maintenance = Some(drain);
                }
            } else if !*self.external.lock().unwrap() {
                *maintenance = None;
            }
        }
        self.status(cfg, store).await
    }

    async fn snapshot(store: &Store) -> Result<Drain> {
        let mut drain = Drain::default();
        for task in store.list_tasks(TaskFilter::default()).await? {
            if task.status() == TaskStatus::InProgress {
                drain.goals.insert(task.goal_id.clone());
                let goal = store.get_goal(&task.goal_id).await?;
                if let (Some(repository), Some(number)) = (goal.pr_repository_id, goal.pr_number)
                    && let Some(pull) = store.pull_request_by_number(&repository, number).await?
                {
                    drain.pulls.insert(pull.id);
                }
                drain.tasks.insert(task.id);
            }
        }
        for session in store
            .list_sessions(SessionFilter {
                live_only: true,
                ..Default::default()
            })
            .await?
        {
            if let Some(id) = session.goal_id {
                drain.goals.insert(id);
            }
            if let Some(id) = session.pull_request_id {
                drain.pulls.insert(id);
            }
            drain.sessions.insert(session.id);
        }
        Ok(drain)
    }

    pub(crate) async fn hold(&self, cfg: &Config, store: &Store, active: bool) -> Result<bool> {
        let _admission = self.admission.write().await;
        let path = cfg.root.join("worker-maintenance.json");
        if active {
            if !*self.external.lock().unwrap() {
                let drain = Self::snapshot(store).await?;
                let mut file = tempfile::NamedTempFile::new_in(&cfg.root)?;
                std::io::Write::write_all(&mut file, &serde_json::to_vec(&drain)?)?;
                file.as_file().sync_all()?;
                file.persist(&path)?;
                std::fs::File::open(&cfg.root)?.sync_all()?;
                *self.error.lock().unwrap() = None;
                *self.maintenance.lock().unwrap() = Some(drain);
                *self.external.lock().unwrap() = true;
            }
            self.restart_ready(store).await
        } else {
            if path.exists() {
                std::fs::remove_file(path)?;
                std::fs::File::open(&cfg.root)?.sync_all()?;
            }
            *self.external.lock().unwrap() = false;
            let live = cfg.live_settings();
            let startup = cfg.startup_settings();
            *self.maintenance.lock().unwrap() = (live.acp_agents != startup.acp_agents
                || live.acp_registry_url != startup.acp_registry_url)
                .then(Drain::default);
            Ok(true)
        }
    }

    /// True only after admitted tasks and their active turns have ended.
    pub async fn restart_ready(&self, store: &Store) -> Result<bool> {
        if self.maintenance.lock().unwrap().is_none() || self.error.lock().unwrap().is_some() {
            return Ok(false);
        }
        let tasks = store.list_tasks(TaskFilter::default()).await?;
        let sessions = store
            .list_sessions(SessionFilter {
                live_only: true,
                ..Default::default()
            })
            .await?;
        let pulls = store.list_pull_requests(Default::default()).await?;
        let guard = self.maintenance.lock().unwrap();
        let Some(drain) = guard.as_ref() else {
            return Ok(false);
        };
        Ok(!tasks
            .iter()
            .any(|task| drain.tasks.contains(&task.id) && task.status() == TaskStatus::InProgress)
            && !sessions
                .iter()
                .any(|session| self.session_blocks(drain, session))
            && !pulls.iter().any(|pull| {
                drain.pulls.contains(&pull.id) && (pull.review_asked || pull.babysit_asked)
            }))
    }

    fn session_blocks(&self, drain: &Drain, session: &AgentSession) -> bool {
        (drain.sessions.contains(&session.id)
            || session
                .task_id
                .as_ref()
                .is_some_and(|id| drain.tasks.contains(id))
            || session
                .goal_id
                .as_ref()
                .is_some_and(|id| drain.goals.contains(id)))
            && matches!(
                session.status(),
                SessionStatus::Running | SessionStatus::Starting
            )
    }

    /// Restore admission after a failed restart and retain the failure for clients.
    pub fn restart_failed(&self, error: String) {
        *self.error.lock().unwrap() = Some(error);
        *self.maintenance.lock().unwrap() = None;
    }
}

fn machine_path(arg: &str) -> bool {
    let value = arg.split_once('=').map_or(arg, |(_, value)| value);
    value.starts_with('/')
        || value.starts_with("~/")
        || value.starts_with("./")
        || value.starts_with("../")
        || value.contains('\\')
        || (value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
            && value.as_bytes().get(1) == Some(&b':'))
}

fn validate(config: &PortableConfigDto) -> Result<()> {
    for (key, value) in [
        ("telemetry_samples_days", config.telemetry_samples_days),
        ("telemetry_logs_days", config.telemetry_logs_days),
        ("telemetry_rollups_days", config.telemetry_rollups_days),
    ] {
        if value <= 0
            || chrono::Duration::try_days(value)
                .and_then(|duration| chrono::Utc::now().checked_sub_signed(duration))
                .is_none()
        {
            bail!("{key} must be a positive, representable number of days");
        }
    }
    tracing_subscriber::EnvFilter::try_new(&config.log_filter).context("invalid log_filter")?;
    let url = url::Url::parse(&config.acp_registry_url).context("invalid acp_registry_url")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        bail!("acp_registry_url must use HTTP or HTTPS");
    }
    let mut ids = HashSet::new();
    for agent in &config.acp_agents {
        if agent.id.trim().is_empty()
            || agent.id.contains(':')
            || !ids.insert(&agent.id)
            || agent.command.first().is_none_or(|arg| arg.is_empty())
        {
            bail!("invalid or duplicate agent {}", agent.id);
        }
    }
    Ok(())
}

pub(crate) fn saved(root: &std::path::Path) -> Result<Option<PortableConfigDto>> {
    let path = root.join(FILE);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    let saved: PortableConfigDto =
        serde_json::from_slice(&bytes).context("reading portable configuration")?;
    validate(&saved)?;
    Ok(Some(saved))
}

pub(crate) fn restore(cfg: &mut Config) -> Result<()> {
    let hold = cfg.root.join("worker-maintenance.json");
    if hold.exists() {
        *cfg.portable.maintenance.lock().unwrap() =
            Some(serde_json::from_slice(&std::fs::read(hold)?)?);
        *cfg.portable.external.lock().unwrap() = true;
    }
    let Some(saved) = saved(&cfg.root)? else {
        return Ok(());
    };
    cfg.delete_merged_branches = saved.delete_merged_branches;
    cfg.delete_merged_worktrees = saved.delete_merged_worktrees;
    cfg.auto_switch = saved.auto_switch;
    cfg.exhausted_patterns = saved.exhausted_patterns;
    cfg.log_filter = saved.log_filter;
    cfg.telemetry_samples_days = saved.telemetry_samples_days;
    cfg.telemetry_logs_days = saved.telemetry_logs_days;
    cfg.telemetry_rollups_days = saved.telemetry_rollups_days;
    cfg.acp_registry_url = saved.acp_registry_url;
    let worker_agents = std::mem::take(&mut cfg.acp_agents);
    cfg.acp_agents = saved
        .acp_agents
        .into_iter()
        .map(|agent| ariadne_client::endpoint::AcpAgentConfig {
            id: agent.id,
            command: agent.command,
        })
        .collect();
    for worker in worker_agents {
        if let Some(agent) = cfg
            .acp_agents
            .iter_mut()
            .find(|agent| agent.id == worker.id)
        {
            agent.command = worker.command;
        } else {
            cfg.acp_agents.push(worker);
        }
    }
    Ok(())
}

/// Replace the process after maintenance drains. Keep the PID and service environment.
pub fn start_restart(
    launcher: std::sync::Arc<crate::launcher::Launcher>,
    telemetry: crate::telemetry::Telemetry,
    timeouts: crate::timeouts::Timeouts,
) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(timeouts.worker_maintenance);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let cfg = &launcher.cfg;
            let _admission = cfg.portable.admission.write().await;
            if *cfg.portable.external.lock().unwrap() {
                continue;
            }
            match cfg.portable.restart_ready(&launcher.store).await {
                Ok(false) => continue,
                Err(error) => {
                    cfg.portable
                        .restart_failed(format!("checking maintenance drain: {error}"));
                    continue;
                }
                Ok(true) => {}
            }
            let result: Result<()> = async {
                let executable =
                    std::env::current_exe().context("finding the daemon executable")?;
                std::fs::metadata(&executable).context("opening the restart executable")?;
                let sessions = launcher
                    .store
                    .list_sessions(SessionFilter {
                        live_only: true,
                        ..Default::default()
                    })
                    .await?;
                // Retire idle runtimes before replacement, including their child processes.
                for session in sessions {
                    launcher.acp.kill_and_wait(&session.id).await;
                }
                let flush = telemetry.clone();
                tokio::task::spawn_blocking(move || flush.flush()).await?;
                use std::os::unix::process::CommandExt;
                let error = std::process::Command::new(executable)
                    .arg("--home")
                    .arg(cfg.root.canonicalize()?)
                    .current_dir(&cfg.root)
                    .exec();
                Err(error).context("replacing the daemon process")
            }
            .await;
            if let Err(error) = result {
                tracing::warn!(%error, "worker maintenance failed");
                cfg.portable.restart_failed(format!("{error:#}"));
            }
        }
    });
}
