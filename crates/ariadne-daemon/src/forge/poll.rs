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
    webhook_url: crate::webhooks::WebhookUrl,
}
struct Worker {
    identity: (String, String, String, String),
    public_url: Option<String>,
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
    pub fn webhook_url(&self) -> crate::webhooks::WebhookUrl {
        self.webhook_url.clone()
    }

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
    let webhook_url = crate::webhooks::WebhookUrl::new(cfg.webhook_public_url.clone());
    let mut urls = webhook_url.subscribe();
    let mut events = events.subscribe();
    let (commands, mut requests) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut workers = HashMap::new();
        let mut public_url = urls.borrow_and_update().clone();
        reconcile(&store, &cfg, every, &mut workers, &public_url).await;
        loop {
            tokio::select! {
                changed = urls.changed() => {
                    if changed.is_err() { break; }
                    public_url = urls.borrow_and_update().clone();
                    reconcile(&store, &cfg, every, &mut workers, &public_url).await;
                },
                command = requests.recv() => match command {
                    None => break,
                    Some(Command::Changed(id, done)) => {
                        sync(&store, &cfg, every, &mut workers, &id, &public_url).await;
                        let _ = done.send(());
                    }
                    Some(Command::Wake(id)) => {
                        let existed = workers.contains_key(&id);
                        sync(&store, &cfg, every, &mut workers, &id, &public_url).await;
                        if existed && let Some(worker) = workers.get(&id) { worker.wake.notify_one(); }
                    }
                    Some(Command::Mode(id, mode)) => {
                        sync(&store, &cfg, every, &mut workers, &id, &public_url).await;
                        if let Some(worker) = workers.get(&id) { worker.mode.send_replace(mode); }
                    }
                },
                event = events.recv() => match event {
                    Ok(event) => match event.event {
                        DomainEvent::RepositoryCreated(repo) | DomainEvent::RepositoryUpdated(repo) => {
                            sync(&store, &cfg, every, &mut workers, &repo.id, &public_url).await;
                        }
                        DomainEvent::RepositoryDeleted(repo) => { workers.remove(&repo.id); }
                        _ => {}
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => reconcile(&store, &cfg, every, &mut workers, &public_url).await,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    });
    ForgePoll {
        commands,
        webhook_url,
    }
}

async fn reconcile(
    store: &Store,
    cfg: &Arc<Config>,
    every: Duration,
    workers: &mut HashMap<String, Worker>,
    public_url: &Option<String>,
) {
    match store.list_repositories().await {
        Ok(repositories) => {
            let ids: HashSet<_> = repositories.iter().map(|r| r.id.clone()).collect();
            workers.retain(|id, _| ids.contains(id));
            for repo in repositories {
                sync(store, cfg, every, workers, &repo.id, public_url).await;
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
    public_url: &Option<String>,
) {
    let row = match store.forge_integration(id).await {
        Ok(row) => row,
        Err(error) => {
            warn!(%error, repository = id, "cannot read forge integration");
            return;
        }
    };
    let enabled = row.as_ref().is_some_and(|r| r.enabled);
    if !enabled {
        if let Some(mut worker) = workers.remove(id) {
            worker.task.abort();
            let _ = (&mut worker.task).await;
        }
        if let Some(row) = row
            && row.webhook_id.is_some()
            && let Err(error) = super::hooks::reconcile(store, cfg, row, None).await
        {
            warn!(%error, repository = id, "cannot remove forge hook");
        }
        if let Err(error) = store.close_disabled_pull_requests(id).await {
            warn!(%error, repository = id, "cannot close disabled pull requests");
        }
        return;
    }
    let mut row = row.unwrap();
    let identity = (
        row.kind.clone(),
        row.host.clone(),
        row.owner.clone(),
        row.name.clone(),
    );
    if workers
        .get(id)
        .is_some_and(|worker| worker.identity != identity)
        && let Some(mut worker) = workers.remove(id)
    {
        worker.task.abort();
        let _ = (&mut worker.task).await;
    }
    let before = (
        row.webhook_state.clone(),
        row.webhook_url.clone(),
        row.webhook_id,
    );
    let reconcile_hook = workers.get(id).is_none_or(|w| &w.public_url != public_url);
    if reconcile_hook {
        match super::hooks::reconcile(store, cfg, row, public_url.as_deref()).await {
            Ok(current) => row = current,
            Err(error) => {
                warn!(%error, repository = id, "cannot reconcile forge hook");
                return;
            }
        }
    }
    let initial_mode = if row.webhook_state == "live" {
        Mode::WakeOnly
    } else {
        Mode::Timer
    };
    if let Some(worker) = workers.get_mut(id) {
        if reconcile_hook {
            worker.public_url = public_url.clone();
            worker.mode.send_replace(initial_mode);
            if before != (row.webhook_state, row.webhook_url, row.webhook_id) {
                worker.wake.notify_one();
            }
        }
    } else {
        let wake = Arc::new(Notify::new());
        let (mode, changes) = watch::channel(initial_mode);
        let task = tokio::spawn(worker(
            store.clone(),
            cfg.clone(),
            id.into(),
            every,
            wake.clone(),
            changes,
        ));
        workers.insert(
            id.into(),
            Worker {
                identity,
                wake,
                mode,
                task,
                public_url: public_url.clone(),
            },
        );
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
