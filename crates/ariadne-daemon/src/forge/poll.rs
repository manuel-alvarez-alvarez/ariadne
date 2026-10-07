//! Timer fallback and explicit repository wakes. Each repository has one worker.
use super::{ForgeClient, pulls};
use crate::{bus::EventBus, config::Config};
use ariadne_api::stream::DomainEvent;
use ariadne_store::{ForgeIntegration, PullRequest, PullRequestFilter, Store};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tracing::warn;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Timer,
    WakeOnly,
}

enum Command {
    Wake(String),
    Mode(String, Mode),
    Changed(String, oneshot::Sender<()>),
}
#[derive(Clone)]
pub struct ForgePoll {
    commands: mpsc::UnboundedSender<Command>,
}
struct Worker {
    wake: Arc<Notify>,
    mode: watch::Sender<Mode>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl ForgePoll {
    /// Finish an integration change before its HTTP response returns.
    pub(crate) async fn changed(&self, repository_id: &str) {
        let (done, wait) = oneshot::channel();
        if self
            .commands
            .send(Command::Changed(repository_id.into(), done))
            .is_ok()
        {
            let _ = wait.await;
        }
    }

    pub fn wake(&self, repository_id: &str) {
        let _ = self.commands.send(Command::Wake(repository_id.into()));
    }
    pub fn set_mode(&self, repository_id: &str, mode: Mode) {
        let _ = self
            .commands
            .send(Command::Mode(repository_id.into(), mode));
    }
}

pub fn start(store: Store, cfg: Arc<Config>, events: &EventBus, every: Duration) -> ForgePoll {
    let mut events = events.subscribe();
    let (commands, mut requests) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut workers = HashMap::new();
        reconcile(&store, &cfg, every, &mut workers).await;
        loop {
            tokio::select! {
                command = requests.recv() => match command {
                    None => break,
                    Some(Command::Changed(id, done)) => {
                        sync(&store, &cfg, every, &mut workers, &id).await;
                        let _ = done.send(());
                    }
                    Some(Command::Wake(id)) => {
                        let existed = workers.contains_key(&id);
                        sync(&store, &cfg, every, &mut workers, &id).await;
                        if existed && let Some(worker) = workers.get(&id) { worker.wake.notify_one(); }
                    }
                    Some(Command::Mode(id, mode)) => {
                        sync(&store, &cfg, every, &mut workers, &id).await;
                        if let Some(worker) = workers.get(&id) { worker.mode.send_replace(mode); }
                    }
                },
                event = events.recv() => match event {
                    Ok(event) => match event.event {
                        DomainEvent::RepositoryCreated(repo) | DomainEvent::RepositoryUpdated(repo) => {
                            sync(&store, &cfg, every, &mut workers, &repo.id).await;
                        }
                        DomainEvent::RepositoryDeleted(repo) => { workers.remove(&repo.id); }
                        _ => {}
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => reconcile(&store, &cfg, every, &mut workers).await,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    });
    ForgePoll { commands }
}

async fn reconcile(
    store: &Store,
    cfg: &Arc<Config>,
    every: Duration,
    workers: &mut HashMap<String, Worker>,
) {
    match store.list_repositories().await {
        Ok(repositories) => {
            let ids: HashSet<_> = repositories.iter().map(|r| r.id.clone()).collect();
            workers.retain(|id, _| ids.contains(id));
            for repo in repositories {
                sync(store, cfg, every, workers, &repo.id).await;
            }
        }
        Err(error) => warn!(%error, "cannot reconcile forge fetches"),
    }
}

async fn sync(
    store: &Store,
    cfg: &Arc<Config>,
    every: Duration,
    workers: &mut HashMap<String, Worker>,
    id: &str,
) {
    let enabled = match store.forge_integration(id).await {
        Ok(row) => row.is_some_and(|r| r.enabled),
        Err(error) => {
            warn!(%error, repository = id, "cannot read forge integration");
            return;
        }
    };
    if !enabled {
        if let Some(mut worker) = workers.remove(id) {
            worker.task.abort();
            let _ = (&mut worker.task).await;
        }
        if let Err(error) = store.close_disabled_pull_requests(id).await {
            warn!(%error, repository = id, "cannot close disabled pull requests");
        }
    } else if !workers.contains_key(id) {
        let wake = Arc::new(Notify::new());
        let (mode, changes) = watch::channel(Mode::Timer);
        let task = tokio::spawn(worker(
            store.clone(),
            cfg.clone(),
            id.into(),
            every,
            wake.clone(),
            changes,
        ));
        workers.insert(id.into(), Worker { wake, mode, task });
    }
}

async fn worker(
    store: Store,
    cfg: Arc<Config>,
    id: String,
    every: Duration,
    wake: Arc<Notify>,
    mut mode: watch::Receiver<Mode>,
) {
    loop {
        if let Err(error) = fetch(&store, &cfg, &id).await {
            warn!(%error, repository = id, "cannot fetch pull requests");
        }
        loop {
            let timer = *mode.borrow_and_update() == Mode::Timer;
            tokio::select! {
                _ = wake.notified() => break,
                _ = tokio::time::sleep(every), if timer => break,
                changed = mode.changed() => if changed.is_err() { return; },
            }
        }
    }
}

async fn fetch(store: &Store, cfg: &Config, id: &str) -> Result<(), String> {
    let Some(integration) = store
        .forge_integration(id)
        .await
        .map_err(|e| e.to_string())?
        .filter(|f| f.enabled)
    else {
        return Ok(());
    };
    let client = ForgeClient::for_repository(cfg, &integration);
    let slug = format!(
        "{}/{}/{}",
        integration.host, integration.owner, integration.name
    );
    let listed = client
        .list_open_pull_requests(&slug, integration.login.as_deref().unwrap_or_default())
        .await?;
    let numbers: HashSet<_> = listed.iter().map(|p| p.number).collect();
    let previous = store
        .list_pull_requests(PullRequestFilter {
            repository_id: Some(id.into()),
            ..Default::default()
        })
        .await
        .map_err(|e| e.to_string())?;
    let reads: HashSet<_> = previous
        .iter()
        .filter(|p| p.tracked_by == "user" || (p.state == "open" && !numbers.contains(&p.number)))
        .map(|p| p.number)
        .collect();
    let mut changed = Vec::new();
    for pull in listed.into_iter().filter(|p| !reads.contains(&p.number)) {
        if !still_enabled(store, &integration).await? {
            return Ok(());
        }
        changed.push(
            pulls::record(store, &integration, pull, "forge", None, None)
                .await?
                .0,
        );
    }
    for number in reads {
        if !still_enabled(store, &integration).await? {
            return Ok(());
        }
        let pull = client.pull_request(&slug, number).await?;
        if pull.number != number {
            return Err("the forge returned another request number".into());
        }
        let existing_id = previous
            .iter()
            .find(|p| p.number == number)
            .map(|p| p.id.clone());
        if !still_enabled(store, &integration).await? {
            return Ok(());
        }
        changed.push(
            pulls::record(store, &integration, pull, "forge", None, existing_id)
                .await?
                .0,
        );
    }
    after_fetch(id, &changed).await;
    Ok(())
}

async fn still_enabled(store: &Store, expected: &ForgeIntegration) -> Result<bool, String> {
    Ok(store
        .forge_integration(&expected.repository_id)
        .await
        .map_err(|e| e.to_string())?
        .is_some_and(|f| {
            f.enabled
                && f.host == expected.host
                && f.owner == expected.owner
                && f.name == expected.name
                && f.login == expected.login
        }))
}

/// The next task fills this with per-request details and news routing.
async fn after_fetch(_repository_id: &str, _changed_rows: &[PullRequest]) {}
