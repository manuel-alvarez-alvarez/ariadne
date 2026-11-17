//! The hosts a client can address (037), and the SSH forward of each
//! `ssh://` host: one `ssh -N -L` child per host, kept up with a capped
//! backoff, and its state on `GET /v1/hosts`. Each host is also kept in
//! sync with this daemon's catalogs and settings ([`sync`]).
//!
//! The forward is the user's own `ssh`, so their keys, agent and
//! `~/.ssh/config` apply. It carries requests, event streams and answers both
//! ways: a client on this machine talks to the far daemon through the local
//! port as if it were here.

use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ariadne_api::hosts::{HostDto, HostKind, HostStatus, HostSyncDto, SyncState};
use ariadne_client::endpoint::HostConfig;
use ariadne_store::Store;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};
use tracing::{info, warn};
use url::Url;

use crate::bus::EventBus;
use crate::config::Config;
use crate::timeouts::Timeouts;

mod sync;

/// The longest wait between two starts of one forward.
const BACKOFF_CAP: Duration = Duration::from_secs(60);

/// The daemon's configured hosts. Cheap to clone: every clone is the same.
#[derive(Clone)]
pub struct Hosts(Arc<Inner>);

struct Inner {
    cfg: Arc<Config>,
    store: Store,
    events: EventBus,
    timeouts: Timeouts,
    /// The configured hosts, in file order, as `GET /v1/hosts` lists them.
    hosts: Mutex<Vec<HostDto>>,
    /// The endpoint of each configured host while it is `ready`, else
    /// `None`: what its sync waits on.
    ready: Vec<watch::Sender<Option<String>>>,
    started: AtomicBool,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl Hosts {
    /// The configured hosts, with no forward and no sync started yet.
    pub fn new(cfg: Arc<Config>, store: Store, events: EventBus, timeouts: Timeouts) -> Self {
        let hosts: Vec<HostDto> = cfg
            .hosts
            .iter()
            .map(|(name, host)| initial(name, host))
            .collect();
        let ready = hosts
            .iter()
            .map(|host| watch::Sender::new(ready_endpoint(host)))
            .collect();
        Self(Arc::new(Inner {
            cfg,
            store,
            events,
            timeouts,
            hosts: Mutex::new(hosts),
            ready,
            started: AtomicBool::new(false),
            tasks: Mutex::new(Vec::new()),
        }))
    }

    /// Start and supervise the forward of every `ssh://` host, and the
    /// sync of every configured host, until [`Hosts::shutdown`]. A second
    /// call does nothing.
    pub fn start(&self) {
        if self.0.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let mut tasks = self.0.tasks.lock().unwrap();
        for index in 0..self.0.cfg.hosts.len() {
            // Subscribed here, not in the task: a change made after `start`
            // returns is on the receiver, however late the task runs.
            let events = self.0.events.subscribe();
            let inner = self.0.clone();
            tasks.push(tokio::spawn(sync::run(inner, index, events)));
        }
        for (index, (_, host)) in self.0.cfg.hosts.iter().enumerate() {
            if !host.endpoint.starts_with("ssh://") {
                continue;
            }
            let inner = self.0.clone();
            tasks.push(tokio::spawn(async move { inner.supervise(index).await }));
        }
    }

    /// End every forward: the daemon is stopping. Each child is killed with
    /// the task that holds it.
    pub async fn shutdown(&self) {
        let tasks = std::mem::take(&mut *self.0.tasks.lock().unwrap());
        for task in tasks {
            task.abort();
            let _ = task.await;
        }
    }

    /// `local` first, then the configured hosts in file order.
    pub fn list(&self) -> Vec<HostDto> {
        let local = HostDto {
            name: "local".into(),
            kind: HostKind::Local,
            endpoint: Some(match self.0.cfg.tcp_listen {
                Some(address) => format!("http://{address}"),
                None => self.0.cfg.socket_path.display().to_string(),
            }),
            status: HostStatus::Ready,
            error: None,
            remote: None,
            sync: HostSyncDto::in_sync(),
        };
        let configured = self.0.hosts.lock().unwrap().clone();
        std::iter::once(local).chain(configured).collect()
    }
}

/// A host as it stands before any forward or sync runs: nothing is pushed
/// to it yet.
fn initial(name: &str, host: &HostConfig) -> HostDto {
    let sync = HostSyncDto {
        state: SyncState::Pending,
        error: None,
        missing_repositories: Vec::new(),
        skipped_permissions: 0,
    };
    if host.endpoint.starts_with("ssh://") {
        HostDto {
            name: name.into(),
            kind: HostKind::Ssh,
            endpoint: None,
            status: HostStatus::Down,
            error: Some("ssh forward not started".into()),
            remote: host.remote.clone(),
            sync,
        }
    } else {
        HostDto {
            name: name.into(),
            kind: HostKind::Direct,
            endpoint: Some(host.endpoint.clone()),
            status: HostStatus::Ready,
            error: None,
            remote: None,
            sync,
        }
    }
}

/// The endpoint a sync pushes to, where the host is `ready`.
fn ready_endpoint(host: &HostDto) -> Option<String> {
    match host.status {
        HostStatus::Ready => host.endpoint.clone(),
        HostStatus::Connecting | HostStatus::Down => None,
    }
}

/// The arguments of one forward from `127.0.0.1:port` to `remote` on the
/// host of the `ssh://` `endpoint`.
fn ssh_args(endpoint: &str, remote: &str, port: u16) -> Result<Vec<String>, String> {
    let url = Url::parse(endpoint).map_err(|error| format!("{endpoint}: {error}"))?;
    let host = url
        .host_str()
        .ok_or_else(|| format!("{endpoint}: no host"))?
        .trim_start_matches('[')
        .trim_end_matches(']');
    let mut args: Vec<String> = [
        "-N",
        "-L",
        &format!("127.0.0.1:{port}:{remote}"),
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=3",
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "BatchMode=yes",
    ]
    .map(String::from)
    .into();
    if let Some(port) = url.port() {
        args.extend(["-p".into(), port.to_string()]);
    }
    args.push(match url.username() {
        "" => host.to_string(),
        user => format!("{user}@{host}"),
    });
    Ok(args)
}

/// A loopback port nothing listens on now.
async fn free_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    Ok(listener.local_addr()?.port())
}

impl Inner {
    /// Move host `index`, and publish it where its status, endpoint or error
    /// changed.
    fn set(
        &self,
        index: usize,
        status: HostStatus,
        endpoint: Option<String>,
        error: Option<String>,
    ) {
        let changed = {
            let mut hosts = self.hosts.lock().unwrap();
            let host = &mut hosts[index];
            let changed = host.status != status || host.endpoint != endpoint || host.error != error;
            host.status = status;
            host.endpoint = endpoint;
            host.error = error;
            self.ready[index].send_if_modified(|ready| {
                let now = ready_endpoint(host);
                let moved = *ready != now;
                *ready = now;
                moved
            });
            changed.then(|| host.clone())
        };
        if let Some(host) = changed {
            self.events.host_updated(host);
        }
    }

    /// Move the sync of host `index`, and publish the host where it changed.
    fn set_sync(&self, index: usize, change: impl FnOnce(&mut HostSyncDto)) {
        let changed = {
            let mut hosts = self.hosts.lock().unwrap();
            let host = &mut hosts[index];
            let before = host.sync.clone();
            change(&mut host.sync);
            (host.sync != before).then(|| host.clone())
        };
        if let Some(host) = changed {
            self.events.host_updated(host);
        }
    }

    /// The sync state of host `index`.
    fn sync_state(&self, index: usize) -> SyncState {
        self.hosts.lock().unwrap()[index].sync.state
    }

    async fn supervise(&self, index: usize) {
        let (name, host) = &self.cfg.hosts[index];
        let base = self.timeouts.ssh_retry;
        let mut backoff = base;
        loop {
            let error = self
                .forward(index, &host.endpoint, &host.remote, &mut backoff, base)
                .await;
            warn!(host = %name, %error, "ssh forward down");
            self.set(index, HostStatus::Down, None, Some(error));
            sleep(backoff).await;
            backoff = (backoff * 2).min(BACKOFF_CAP);
        }
    }

    /// One run of the forward, from its start to its exit. Answers why it
    /// ended. A forward that came up resets `backoff` to `base`.
    async fn forward(
        &self,
        index: usize,
        endpoint: &str,
        remote: &Option<String>,
        backoff: &mut Duration,
        base: Duration,
    ) -> String {
        let port = match free_port().await {
            Ok(port) => port,
            Err(error) => return format!("cannot pick a loopback port: {error}"),
        };
        let remote = remote
            .clone()
            .unwrap_or_else(|| self.cfg.socket_path.display().to_string());
        let args = match ssh_args(endpoint, &remote, port) {
            Ok(args) => args,
            Err(error) => return error,
        };
        self.set(index, HostStatus::Connecting, None, None);
        let ssh = self.cfg.ssh_bin.as_deref().unwrap_or("ssh");
        let mut child = match Command::new(ssh)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(child) => child,
            Err(error) => return format!("cannot run {ssh}: {error}"),
        };
        // The last line the process wrote is why it ended.
        let stderr = child.stderr.take().map(|stderr| {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                let mut last = None;
                while let Ok(Some(line)) = lines.next_line().await {
                    if !line.trim().is_empty() {
                        last = Some(line.trim().to_string());
                    }
                }
                last
            })
        });
        let local = format!("http://127.0.0.1:{port}");
        let exited = tokio::select! {
            status = child.wait() => Some(status),
            () = self.until_answered(&local) => None,
        };
        let status = match exited {
            Some(status) => status,
            None => {
                *backoff = base;
                info!(endpoint = %local, "ssh forward ready");
                self.set(index, HostStatus::Ready, Some(local), None);
                child.wait().await
            }
        };
        let last = match stderr {
            Some(task) => timeout(self.timeouts.ssh_probe, task)
                .await
                .ok()
                .and_then(Result::ok)
                .flatten(),
            None => None,
        };
        last.unwrap_or_else(|| match status {
            Ok(status) => format!("{ssh} exited: {status}"),
            Err(error) => format!("{ssh} could not be waited on: {error}"),
        })
    }

    /// Probe `local` until the far daemon answers its health check.
    async fn until_answered(&self, local: &str) {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(self.timeouts.ssh_probe)
            .build()
            .expect("an HTTP client with no proxy");
        let health = format!("{local}/v1/health");
        loop {
            if let Ok(response) = client.get(&health).send().await
                && response.status().is_success()
                && let Ok(body) = response.bytes().await
                && serde_json::from_slice::<serde_json::Value>(&body)
                    .is_ok_and(|health| health["status"] == "ok")
            {
                return;
            }
            sleep(self.timeouts.ssh_probe_interval).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_forward_names_the_user_the_port_and_the_remote() {
        let args = ssh_args("ssh://me@box:2222", "/run/a.sock", 4100).unwrap();
        assert_eq!(
            args,
            [
                "-N",
                "-L",
                "127.0.0.1:4100:/run/a.sock",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                "-o",
                "ExitOnForwardFailure=yes",
                "-o",
                "BatchMode=yes",
                "-p",
                "2222",
                "me@box",
            ]
        );
    }

    #[test]
    fn a_forward_without_user_or_port_names_the_host_alone() {
        let args = ssh_args("ssh://box", "127.0.0.1:7676", 4100).unwrap();
        assert_eq!(args[2], "127.0.0.1:4100:127.0.0.1:7676");
        assert_eq!(args.last().map(String::as_str), Some("box"));
        assert!(!args.iter().any(|arg| arg == "-p"));
    }
}
