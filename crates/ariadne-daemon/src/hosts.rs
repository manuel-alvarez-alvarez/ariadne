//! The hosts a client can address (037), and the SSH forward of each
//! `ssh://` host: one `ssh -N -L` child per host, kept up with a capped
//! backoff, and its state on `GET /v1/hosts`. The health of each host is
//! checked again and again, so its status stays true. Each host is also kept
//! in sync with this daemon's catalogs and settings ([`sync`]).
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
use ariadne_client::Client;
use ariadne_client::endpoint::HostConfig;
use ariadne_store::Store;
use rustix::process::{Pid, Signal, kill_process_group, test_kill_process_group};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep, sleep_until, timeout};
use tracing::{info, warn};
use url::Url;

use crate::bus::EventBus;
use crate::config::Config;
use crate::timeouts::Timeouts;

mod setup;
mod sync;

/// The longest wait between two starts of one forward.
const BACKOFF_CAP: Duration = Duration::from_secs(60);

/// A reconnect asked of one host: answered once the host has acted on it.
type Reconnect = oneshot::Sender<()>;

/// Why [`Hosts::reconnect`] refused a name.
#[derive(Debug, PartialEq, Eq)]
pub enum ReconnectError {
    /// `local` is this daemon: there is nothing to reconnect.
    Local,
    /// No configured host has the name.
    Unknown,
}

/// The daemon's configured hosts. Cheap to clone: every clone is the same.
#[derive(Clone)]
pub struct Hosts(Arc<Inner>);

struct Inner {
    cfg: Arc<Config>,
    colors: Mutex<std::collections::HashMap<String, ariadne_api::hosts::HostColor>>,
    setup: Mutex<std::collections::BTreeMap<String, setup::Record>>,
    extra: Mutex<Vec<Hosts>>,
    store: Store,
    mutations: tokio::sync::Mutex<()>,
    events: EventBus,
    timeouts: Timeouts,
    /// The configured hosts, in database insertion order, as `GET /v1/hosts` lists them.
    hosts: Mutex<Vec<HostDto>>,
    /// The endpoint of each configured host while it is `ready`, else
    /// `None`: what its sync waits on.
    ready: Vec<watch::Sender<Option<String>>>,
    /// The reconnects asked of each configured host.
    reconnects: Vec<mpsc::UnboundedSender<Reconnect>>,
    /// The other end of each of `reconnects`, until [`Hosts::start`] gives
    /// it to the task of its host.
    asked: Mutex<Vec<Option<mpsc::UnboundedReceiver<Reconnect>>>>,
    started: AtomicBool,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl Hosts {
    /// The configured hosts, with no forward and no sync started yet.
    pub async fn new(cfg: Arc<Config>, store: Store, events: EventBus, timeouts: Timeouts) -> Self {
        let mut cfg = (*cfg).clone();
        let records = setup::load(&cfg.root);
        let stored = store.list_hosts().await.expect("read stored hosts");
        cfg.hosts = stored
            .iter()
            .filter_map(|row| {
                row.endpoint.as_ref().map(|endpoint| {
                    (
                        row.name.clone(),
                        HostConfig {
                            endpoint: endpoint.clone(),
                            remote: row.remote.clone(),
                        },
                    )
                })
            })
            .collect();
        let root_cfg = Arc::new(cfg.clone());
        let mut root = cfg;
        root.hosts.clear();
        let result = Self::configured(
            Arc::new(root),
            store.clone(),
            events.clone(),
            timeouts,
            records,
        );
        for entry in &root_cfg.hosts {
            let mut child = (*root_cfg).clone();
            child.hosts = vec![entry.clone()];
            result.0.extra.lock().unwrap().push(Self::configured(
                Arc::new(child),
                store.clone(),
                events.clone(),
                timeouts,
                Default::default(),
            ));
        }
        *result.0.colors.lock().unwrap() = stored
            .into_iter()
            .filter_map(|r| {
                r.color.map(|c| {
                    (
                        r.name,
                        serde_json::from_value(serde_json::Value::String(c))
                            .expect("stored host colour"),
                    )
                })
            })
            .collect();
        for child in result.0.extra.lock().unwrap().iter() {
            for host in child.0.hosts.lock().unwrap().iter_mut() {
                host.color = result.0.colors.lock().unwrap().get(&host.name).copied();
            }
        }
        result
    }

    fn configured(
        cfg: Arc<Config>,
        store: Store,
        events: EventBus,
        timeouts: Timeouts,
        records: std::collections::BTreeMap<String, setup::Record>,
    ) -> Self {
        let hosts: Vec<HostDto> = cfg
            .hosts
            .iter()
            .map(|(name, host)| initial(name, host))
            .collect();
        let ready = hosts
            .iter()
            .map(|host| watch::Sender::new(ready_endpoint(host)))
            .collect();
        let (reconnects, asked) = hosts
            .iter()
            .map(|_| {
                let (send, receive) = mpsc::unbounded_channel();
                (send, Some(receive))
            })
            .unzip();
        Self(Arc::new(Inner {
            cfg,
            colors: Mutex::new(Default::default()),
            setup: Mutex::new(records),
            extra: Mutex::new(Vec::new()),
            store,
            mutations: tokio::sync::Mutex::new(()),
            events,
            timeouts,
            hosts: Mutex::new(hosts),
            ready,
            reconnects,
            asked: Mutex::new(asked),
            started: AtomicBool::new(false),
            tasks: Mutex::new(Vec::new()),
        }))
    }

    /// Start and supervise the forward of every `ssh://` host, the health
    /// checks of every configured host, and the sync of every configured
    /// host, until [`Hosts::shutdown`]. A second call does nothing.
    pub fn start(&self) {
        if self.0.started.swap(true, Ordering::SeqCst) {
            return;
        }
        for host in self.0.extra.lock().unwrap().iter() {
            host.start();
        }
        let mut tasks = self.0.tasks.lock().unwrap();
        for index in 0..self.0.cfg.hosts.len() {
            // Subscribed here, not in the task: a change made after `start`
            // returns is on the receiver, however late the task runs.
            let events = self.0.events.subscribe();
            let inner = self.0.clone();
            tasks.push(tokio::spawn(sync::run(inner, index, events)));
        }
        let mut asked = self.0.asked.lock().unwrap();
        for (index, (_, host)) in self.0.cfg.hosts.iter().enumerate() {
            let inner = self.0.clone();
            let asked = asked[index]
                .take()
                .expect("the reconnects of an unstarted host");
            tasks.push(if host.endpoint.starts_with("ssh://") {
                tokio::spawn(async move { inner.supervise(index, asked).await })
            } else {
                tokio::spawn(async move { inner.direct(index, asked).await })
            });
        }
    }

    /// End every forward and every health check: the daemon is stopping.
    /// Each child is killed with the task that holds it.
    pub async fn shutdown(&self) {
        self.stop_own().await;
        let extra = self.0.extra.lock().unwrap().clone();
        for host in extra {
            host.stop_own().await;
        }
    }

    async fn stop_own(&self) {
        let tasks = std::mem::take(&mut *self.0.tasks.lock().unwrap());
        for task in &tasks {
            task.abort();
        }
        for task in tasks {
            let _ = task.await;
        }
    }

    /// `local` first, then the configured hosts in database insertion order.
    pub fn list(&self) -> Vec<HostDto> {
        let local = HostDto {
            color: None,
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
        let extra = self.0.extra.lock().unwrap().clone();
        let colors = self.0.colors.lock().unwrap();
        std::iter::once(local)
            .chain(configured)
            .chain(extra.iter().flat_map(|h| h.0.hosts.lock().unwrap().clone()))
            .map(|mut host| {
                host.color = colors.get(&host.name).copied();
                host
            })
            .collect()
    }

    /// Make host `name` reconnect at once, and answer the host once it has
    /// acted on it. A `down` or `connecting` `ssh` host starts a new
    /// forward with the first wait of its backoff. A `ready` `ssh` host and
    /// a `direct` host take one health check; a failed check makes the host
    /// `down`, and an `ssh` host then starts a new forward.
    pub async fn reconnect(&self, name: &str) -> Result<HostDto, ReconnectError> {
        let extra = self
            .0
            .extra
            .lock()
            .unwrap()
            .iter()
            .find(|h| h.0.cfg.hosts[0].0 == name)
            .cloned();
        if let Some(host) = extra {
            return host.reconnect_own(name).await;
        }
        self.reconnect_own(name).await
    }

    async fn reconnect_own(&self, name: &str) -> Result<HostDto, ReconnectError> {
        if name == "local" {
            return Err(ReconnectError::Local);
        }
        let index = self
            .0
            .cfg
            .hosts
            .iter()
            .position(|(host, _)| host == name)
            .ok_or(ReconnectError::Unknown)?;
        let (done, acted) = oneshot::channel();
        // Before `start` no task reads the request, and after `shutdown`
        // none is left: the host answers as it stands.
        if self.0.started.load(Ordering::SeqCst) && self.0.reconnects[index].send(done).is_ok() {
            let _ = acted.await;
        }
        Ok(self.0.hosts.lock().unwrap()[index].clone())
    }
}

/// A host as it stands before any forward, check or sync runs: nothing is
/// pushed to it yet, and a `direct` host is `connecting` until its first
/// good health answer.
fn initial(name: &str, host: &HostConfig) -> HostDto {
    let sync = HostSyncDto {
        state: SyncState::Pending,
        error: None,
        missing_repositories: Vec::new(),
        skipped_permissions: 0,
    };
    if host.endpoint.starts_with("ssh://") {
        HostDto {
            color: None,
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
            color: None,
            name: name.into(),
            kind: HostKind::Direct,
            endpoint: Some(host.endpoint.clone()),
            status: HostStatus::Connecting,
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

/// A client for the endpoint of a host: an `http://` address, else a
/// socket path.
fn client_for(endpoint: &str) -> Client {
    if endpoint.starts_with("http://") {
        Client::tcp(endpoint)
    } else {
        Client::unix(endpoint)
    }
}

/// One health check of a host: `GET /v1/health` through `client`, bounded
/// by `bound`. Answers why the check failed.
pub(crate) async fn check(client: &Client, bound: Duration) -> Result<(), String> {
    match timeout(bound, client.health()).await {
        Err(_) => Err(format!("no answer in {bound:?}")),
        Ok(Err(error)) => Err(error.to_string()),
        Ok(Ok(health)) if health.status == "ok" => Ok(()),
        Ok(Ok(health)) => Err(format!("health status {:?}", health.status)),
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

/// How one run of a forward ended.
enum Ended {
    /// The forward is down for this reason: the next one waits the backoff.
    Down(String),
    /// A reconnect cut the run: the next one starts at once.
    Again(Reconnect),
}

/// Answer the reconnect in `asked`, if any: the host has acted on it.
fn acted(asked: &mut Option<Reconnect>) {
    if let Some(reply) = asked.take() {
        let _ = reply.send(());
    }
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

    /// Keep the status of `direct` host `index` true: `connecting` until
    /// its first good answer or `host_unanswered`, then `ready` while it
    /// answers and `down` while it does not. A reconnect asked on
    /// `requests` checks it at once, and its status follows the answer.
    async fn direct(&self, index: usize, mut requests: mpsc::UnboundedReceiver<Reconnect>) {
        let (name, host) = &self.cfg.hosts[index];
        let endpoint = &host.endpoint;
        let client = client_for(endpoint);
        let probe = self.timeouts.ssh_probe;
        let mut asked: Option<Reconnect> = None;
        let first = tokio::select! {
            answered = self.answer_within(&client) => answered,
            Some(reply) = requests.recv() => {
                asked = Some(reply);
                check(&client, probe).await
            }
        };
        let mut answered = match first {
            Ok(()) => true,
            Err(why) => {
                self.direct_down(index, name, format!("{endpoint} does not answer: {why}"));
                acted(&mut asked);
                false
            }
        };
        loop {
            if !answered {
                tokio::select! {
                    () = self.until_answered(&client, self.timeouts.host_check) => {}
                    Some(reply) = requests.recv() => {
                        if let Err(why) = check(&client, probe).await {
                            self.direct_down(index, name, format!("{endpoint} does not answer: {why}"));
                            let _ = reply.send(());
                            continue;
                        }
                        asked = Some(reply);
                    }
                }
            }
            info!(host = %name, "host ready");
            self.set(index, HostStatus::Ready, Some(endpoint.clone()), None);
            acted(&mut asked);
            let error = loop {
                tokio::select! {
                    error = self.until_silent(&client, endpoint) => break error,
                    Some(reply) = requests.recv() => match check(&client, probe).await {
                        Ok(()) => {
                            let _ = reply.send(());
                        }
                        Err(why) => {
                            asked = Some(reply);
                            break format!("{endpoint} does not answer: {why}");
                        }
                    },
                }
            };
            self.direct_down(index, name, error);
            acted(&mut asked);
            answered = false;
        }
    }

    /// Make `direct` host `index` `down` with `error`. It keeps its
    /// configured endpoint.
    fn direct_down(&self, index: usize, name: &str, error: String) {
        warn!(host = %name, %error, "host down");
        let endpoint = self.cfg.hosts[index].1.endpoint.clone();
        self.set(index, HostStatus::Down, Some(endpoint), Some(error));
    }

    /// Keep the forward of `ssh` host `index` up. A forward that ends is
    /// started again after the backoff. A reconnect asked on `requests`
    /// cuts the wait or the current try, and puts the backoff back to its
    /// first value.
    async fn supervise(&self, index: usize, mut requests: mpsc::UnboundedReceiver<Reconnect>) {
        let (name, host) = &self.cfg.hosts[index];
        let base = self.timeouts.ssh_retry;
        let mut backoff = base;
        let mut asked: Option<Reconnect> = None;
        loop {
            let ended = self
                .forward(
                    index,
                    &host.endpoint,
                    &host.remote,
                    &mut backoff,
                    base,
                    &mut requests,
                    &mut asked,
                )
                .await;
            let error = match ended {
                Ended::Again(reply) => {
                    asked = Some(reply);
                    backoff = base;
                    continue;
                }
                Ended::Down(error) => error,
            };
            warn!(host = %name, %error, "ssh forward down");
            self.set(index, HostStatus::Down, None, Some(error));
            acted(&mut asked);
            tokio::select! {
                () = sleep(backoff) => backoff = (backoff * 2).min(BACKOFF_CAP),
                Some(reply) = requests.recv() => {
                    asked = Some(reply);
                    backoff = base;
                }
            }
        }
    }

    /// One run of the forward, from its start to its end. A forward that
    /// came up resets `backoff` to `base`. A forward whose far daemon does
    /// not answer in `host_unanswered` is killed. A reconnect asked on
    /// `requests` while it connects kills it. One asked while it is ready
    /// checks the host at once, and a failed check kills it with the host
    /// `down`. The reconnect in `asked` is answered once the host is
    /// `connecting`.
    #[allow(clippy::too_many_arguments)]
    async fn forward(
        &self,
        index: usize,
        endpoint: &str,
        remote: &Option<String>,
        backoff: &mut Duration,
        base: Duration,
        requests: &mut mpsc::UnboundedReceiver<Reconnect>,
        asked: &mut Option<Reconnect>,
    ) -> Ended {
        let port = match free_port().await {
            Ok(port) => port,
            Err(error) => return Ended::Down(format!("cannot pick a loopback port: {error}")),
        };
        let remote = remote
            .clone()
            .unwrap_or_else(|| self.cfg.socket_path.display().to_string());
        let args = match ssh_args(endpoint, &remote, port) {
            Ok(args) => args,
            Err(error) => return Ended::Down(error),
        };
        self.set(index, HostStatus::Connecting, None, None);
        let ssh = self.cfg.ssh_bin.as_deref().unwrap_or("ssh");
        // Close the pipe on exec or process exit, including an idle restart.
        // The watcher ends the forward's process group when that happens.
        const GUARD: &str = r#"exec 3<&0
"$@" </dev/null 3<&- &
forward=$!
(read -r _ <&3; kill -s KILL 0) &
watcher=$!
wait "$forward"
status=$?
kill "$watcher" 2>/dev/null
exit "$status"
"#;
        let mut child = match Command::new("/bin/sh")
            .args(["-c", GUARD, "ariadne-ssh-guard", ssh])
            .args(&args)
            .stdin(Stdio::piped())
            .process_group(0)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(child) => child,
            Err(error) => return Ended::Down(format!("cannot run {ssh}: {error}")),
        };
        let group = Pid::from_raw(child.id().expect("a spawned SSH process").cast_signed())
            .expect("a positive process ID");
        // Child::wait closes its stdin. Keep this end until the forward ends.
        let _lifetime = child.stdin.take();
        acted(asked);
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
        let client = client_for(&local);
        let answered = tokio::select! {
            status = child.wait() => Err(status),
            answered = self.answer_within(&client) => Ok(answered),
            Some(reply) = requests.recv() => {
                self.finish_forward(index, &mut child, group).await;
                return Ended::Again(reply);
            }
        };
        let status = match answered {
            Err(status) => status,
            Ok(Err(why)) => {
                self.finish_forward(index, &mut child, group).await;
                return Ended::Down(format!(
                    "the far daemon does not answer through the forward: {why}"
                ));
            }
            Ok(Ok(())) => {
                *backoff = base;
                info!(endpoint = %local, "ssh forward ready");
                self.set(index, HostStatus::Ready, Some(local.clone()), None);
                loop {
                    tokio::select! {
                        status = child.wait() => break status,
                        error = self.until_silent(&client, &local) => {
                            self.finish_forward(index, &mut child, group).await;
                            return Ended::Down(error);
                        }
                        Some(reply) = requests.recv() => {
                            match check(&client, self.timeouts.ssh_probe).await {
                                Ok(()) => {
                                    let _ = reply.send(());
                                }
                                Err(why) => {
                                    self.finish_forward(index, &mut child, group).await;
                                    let error = format!("{local} does not answer: {why}");
                                    warn!(endpoint = %local, %error, "ssh forward down");
                                    self.set(index, HostStatus::Down, None, Some(error));
                                    return Ended::Again(reply);
                                }
                            }
                        }
                    }
                }
            }
        };
        self.finish_forward(index, &mut child, group).await;
        let last = match stderr {
            Some(task) => timeout(self.timeouts.ssh_probe, task)
                .await
                .ok()
                .and_then(Result::ok)
                .flatten(),
            None => None,
        };
        Ended::Down(last.unwrap_or_else(|| match status {
            Ok(status) => format!("{ssh} exited: {status}"),
            Err(error) => format!("{ssh} could not be waited on: {error}"),
        }))
    }

    /// Keep ownership through cleanup failures. A replacement needs ESRCH,
    /// including reaping by the OS after the guard's own wait completes.
    async fn finish_forward(&self, index: usize, child: &mut Child, group: Pid) {
        let mut attempt = 1u64;
        loop {
            match end_forward(child, group, &self.timeouts).await {
                Ok(()) => return,
                Err(error) => {
                    let error = format!("SSH forward cleanup attempt {attempt}: {error}");
                    warn!(%error, "SSH cleanup will retry");
                    self.set(index, HostStatus::Connecting, None, Some(error));
                }
            }
            attempt = attempt.saturating_add(1);
            sleep(self.timeouts.ssh_forward_poll).await;
        }
    }

    /// Check a `connecting` host every `ssh_probe_interval` until its first
    /// good answer. Answers why it failed once `host_unanswered` ran out
    /// with no good answer.
    async fn answer_within(&self, client: &Client) -> Result<(), String> {
        let deadline = Instant::now() + self.timeouts.host_unanswered;
        let mut why = String::from("no answer yet");
        let checks = async {
            loop {
                match check(client, self.timeouts.ssh_probe).await {
                    Ok(()) => return,
                    Err(error) => why = error,
                }
                sleep(self.timeouts.ssh_probe_interval).await;
            }
        };
        tokio::select! {
            () = checks => Ok(()),
            () = sleep_until(deadline) => Err(why),
        }
    }

    /// Check a host every `interval` until its first good answer.
    async fn until_answered(&self, client: &Client, interval: Duration) {
        while check(client, self.timeouts.ssh_probe).await.is_err() {
            sleep(interval).await;
        }
    }

    /// Check a `ready` host every `host_check` until it has given no good
    /// answer for `host_unanswered`. Answers why it is `down`.
    async fn until_silent(&self, client: &Client, endpoint: &str) -> String {
        let unanswered = self.timeouts.host_unanswered;
        let mut answered = Instant::now();
        let mut why = String::from("no answer");
        loop {
            let next = async {
                sleep(self.timeouts.host_check).await;
                check(client, self.timeouts.ssh_probe).await
            };
            tokio::select! {
                checked = next => match checked {
                    Ok(()) => answered = Instant::now(),
                    Err(error) => why = error,
                },
                () = sleep_until(answered + unanswered) => {
                    return format!("{endpoint} has not answered for {unanswered:?}: {why}");
                }
            }
        }
    }
}

/// End the group before publishing Down or starting its replacement.
/// The lifetime watcher handles exec; normal exits must not depend on it.
async fn end_forward(child: &mut Child, group: Pid, timeouts: &Timeouts) -> Result<(), String> {
    let cleanup = async {
        match kill_process_group(group, Signal::KILL) {
            // macOS reports EPERM for an owned group containing only zombies.
            Ok(()) | Err(rustix::io::Errno::SRCH | rustix::io::Errno::PERM) => (),
            Err(error) => return Err(std::io::Error::from(error)),
        }
        child.wait().await?;
        // Waiting for the guard alone does not reap its SSH grandchild.
        // The group must disappear, including orphaned zombies, before Down.
        loop {
            match test_kill_process_group(group) {
                Err(rustix::io::Errno::SRCH) => return Ok(()),
                Err(rustix::io::Errno::INTR) => continue,
                Ok(()) | Err(rustix::io::Errno::PERM) => sleep(timeouts.ssh_forward_poll).await,
                Err(error) => return Err(std::io::Error::from(error)),
            }
        }
    };
    match timeout(timeouts.ssh_forward_end, cleanup).await {
        Ok(result) => result.map_err(|error| format!("SSH forward cleanup failed: {error}")),
        Err(_) => Err(format!(
            "SSH forward process group {} did not end within {:?}",
            group.as_raw_pid(),
            timeouts.ssh_forward_end
        )),
    }
}

impl Hosts {
    pub(crate) async fn mutation(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.0.mutations.lock().await
    }

    pub(crate) async fn save_connection(
        &self,
        name: &str,
        endpoint: Option<&str>,
        remote: Option<&str>,
        color: Option<ariadne_api::hosts::HostColor>,
    ) -> Result<HostDto, String> {
        let encoded = color.map(|c| {
            serde_json::to_value(c)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        });
        self.0
            .store
            .save_host(name, endpoint, remote, encoded.as_deref())
            .await
            .map_err(|e| e.to_string())?;
        let unchanged = self.connection(name).is_some_and(|c| {
            Some(c.endpoint.as_str()) == endpoint && c.remote.as_deref() == remote
        });
        let replacement = if unchanged { None } else { self.detach(name) };
        let slot = replacement.as_ref().map(|(index, _)| *index);
        if let Some((_, old)) = replacement {
            old.stop_own().await;
        }
        {
            let mut colors = self.0.colors.lock().unwrap();
            colors.remove(name);
            if let Some(color) = color {
                colors.insert(name.into(), color);
            }
        }
        if let Some(endpoint) = endpoint.filter(|_| !unchanged) {
            let mut cfg = (*self.0.cfg).clone();
            cfg.hosts = vec![(
                name.into(),
                HostConfig {
                    endpoint: endpoint.into(),
                    remote: remote.map(str::to_owned),
                },
            )];
            let child = Self::configured(
                Arc::new(cfg),
                self.0.store.clone(),
                self.0.events.clone(),
                self.0.timeouts,
                Default::default(),
            );
            child.0.hosts.lock().unwrap()[0].color = color;
            child.start();
            let mut extra = self.0.extra.lock().unwrap();
            if let Some(index) = slot {
                extra.insert(index, child);
            } else {
                extra.push(child);
            }
        }
        for child in self.0.extra.lock().unwrap().iter() {
            if child.0.cfg.hosts[0].0 == name {
                child.0.hosts.lock().unwrap()[0].color = color;
            }
        }
        let dto = self.list().into_iter().find(|h| h.name == name).unwrap();
        self.0.events.host_updated(dto.clone());
        Ok(dto)
    }

    fn detach(&self, name: &str) -> Option<(usize, Self)> {
        let mut extra = self.0.extra.lock().unwrap();
        let index = extra.iter().position(|h| h.0.cfg.hosts[0].0 == name)?;
        Some((index, extra.remove(index)))
    }

    pub(crate) fn connection(&self, name: &str) -> Option<HostConfig> {
        self.0
            .extra
            .lock()
            .unwrap()
            .iter()
            .find(|h| h.0.cfg.hosts[0].0 == name)
            .map(|h| h.0.cfg.hosts[0].1.clone())
    }

    pub(crate) async fn remove_connection(&self, name: &str) -> Result<(), String> {
        let dto = self
            .list()
            .into_iter()
            .find(|h| h.name == name)
            .ok_or("unknown host")?;
        self.0
            .store
            .delete_host(name)
            .await
            .map_err(|e| e.to_string())?;
        if let Some((_, old)) = self.detach(name) {
            old.stop_own().await;
        }
        self.0.colors.lock().unwrap().remove(name);
        self.0.events.host_deleted(dto);
        Ok(())
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
