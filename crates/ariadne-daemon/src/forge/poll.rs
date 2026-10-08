//! Timer fallback and explicit repository wakes. Each repository has one worker.
use super::{ForgeClient, pulls};
use crate::{bus::EventBus, config::Config, scheduler::SchedEvent, webhooks::Public};
use ariadne_api::stream::DomainEvent;
use ariadne_store::{ForgeIntegration, PullRequest, PullRequestFilter, SessionFilter, Store};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, OnceLock},
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
    /// Where a changed request is reported, once the scheduler is up: it
    /// is what starts, tells and ends the request's session (026).
    scheduler: Waker,
}

/// The scheduler's event sender, shared by the handle and every worker.
type Waker = Arc<OnceLock<mpsc::UnboundedSender<SchedEvent>>>;

/// Tell the scheduler that `pull_request_id` changed.
fn wake_scheduler(scheduler: &Waker, pull_request_id: &str) {
    if let Some(tx) = scheduler.get() {
        let _ = tx.send(SchedEvent::PullRequestChanged(pull_request_id.into()));
    }
}

/// What a fetch hands its changed rows on to (026): how long a detail fetch
/// may take, and the scheduler to report to; and where a move of the open
/// issues is published (028), with what each repository's last read of them
/// held.
#[derive(Clone)]
struct Handoff {
    details: Duration,
    scheduler: Waker,
    bus: EventBus,
    issues_seen: Arc<std::sync::Mutex<HashMap<String, u64>>>,
}
struct Worker {
    identity: (String, String, String, String),
    public_url: Public,
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

    /// Report every changed request to `tx` from now on.
    pub fn connect_scheduler(&self, tx: mpsc::UnboundedSender<SchedEvent>) {
        let _ = self.scheduler.set(tx);
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

/// Start the fetches: `every` is the fallback timer, and `details` bounds one
/// detail fetch of a request (`Timeouts::forge_details`).
pub fn start(
    store: Store,
    cfg: Arc<Config>,
    events: &EventBus,
    every: Duration,
    details: Duration,
) -> ForgePoll {
    let scheduler: Waker = Arc::default();
    let handoff = Handoff {
        details,
        scheduler: scheduler.clone(),
        bus: events.clone(),
        issues_seen: Arc::default(),
    };
    let webhook_url = crate::webhooks::WebhookUrl::new(cfg.webhook_public_url.clone());
    let mut urls = webhook_url.subscribe();
    let mut events = events.subscribe();
    let (commands, mut requests) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut workers = HashMap::new();
        let mut public_url = urls.borrow_and_update().clone();
        reconcile(&store, &cfg, every, &mut workers, &public_url, &handoff).await;
        loop {
            tokio::select! {
                changed = urls.changed() => {
                    if changed.is_err() { break; }
                    public_url = urls.borrow_and_update().clone();
                    reconcile(&store, &cfg, every, &mut workers, &public_url, &handoff).await;
                },
                command = requests.recv() => match command {
                    None => break,
                    Some(Command::Changed(id, done)) => {
                        sync(&store, &cfg, every, &mut workers, &id, &public_url, &handoff).await;
                        let _ = done.send(());
                    }
                    Some(Command::Wake(id)) => {
                        let existed = workers.contains_key(&id);
                        sync(&store, &cfg, every, &mut workers, &id, &public_url, &handoff).await;
                        if existed && let Some(worker) = workers.get(&id) { worker.wake.notify_one(); }
                    }
                    Some(Command::Mode(id, mode)) => {
                        sync(&store, &cfg, every, &mut workers, &id, &public_url, &handoff).await;
                        if let Some(worker) = workers.get(&id) { worker.mode.send_replace(mode); }
                    }
                },
                event = events.recv() => match event {
                    Ok(event) => match event.event {
                        DomainEvent::RepositoryCreated(repo) | DomainEvent::RepositoryUpdated(repo) => {
                            sync(&store, &cfg, every, &mut workers, &repo.id, &public_url, &handoff).await;
                        }
                        DomainEvent::RepositoryDeleted(repo) => { workers.remove(&repo.id); }
                        _ => {}
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => reconcile(&store, &cfg, every, &mut workers, &public_url, &handoff).await,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    });
    ForgePoll {
        commands,
        webhook_url,
        scheduler,
    }
}

async fn reconcile(
    store: &Store,
    cfg: &Arc<Config>,
    every: Duration,
    workers: &mut HashMap<String, Worker>,
    public_url: &Public,
    handoff: &Handoff,
) {
    match store.list_repositories().await {
        Ok(repositories) => {
            let ids: HashSet<_> = repositories.iter().map(|r| r.id.clone()).collect();
            workers.retain(|id, _| ids.contains(id));
            for repo in repositories {
                sync(store, cfg, every, workers, &repo.id, public_url, handoff).await;
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
    public_url: &Public,
    handoff: &Handoff,
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
            && let Err(error) = super::hooks::reconcile(store, cfg, row, &Public::default()).await
        {
            warn!(%error, repository = id, "cannot remove forge hook");
        }
        match store.close_disabled_pull_requests(id).await {
            // A closed request's session ends with it (026).
            Ok(closed) => {
                for row in closed {
                    wake_scheduler(&handoff.scheduler, &row.id);
                }
            }
            Err(error) => warn!(%error, repository = id, "cannot close disabled pull requests"),
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
        match super::hooks::reconcile(store, cfg, row, public_url).await {
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
            handoff.clone(),
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
    handoff: Handoff,
    id: String,
    every: Duration,
    wake: Arc<Notify>,
    mut mode: watch::Receiver<Mode>,
) {
    loop {
        let fetched = fetch(&store, &cfg, &handoff, &id).await;
        if let Err(error) = &fetched {
            warn!(%error, repository = id, "cannot fetch pull requests");
        }
        // What says polling works, where no hook is live (026).
        if let Err(error) = store
            .set_forge_fetch_error(&id, fetched.err().as_deref())
            .await
        {
            warn!(%error, repository = id, "cannot record how the fetch went");
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

async fn fetch(store: &Store, cfg: &Config, handoff: &Handoff, id: &str) -> Result<(), String> {
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
    // Every open request of the repository, and which of them ask for the
    // user's review. A request of the user's a task opened is that task's
    // author's to keep (005); any other is listed, and nobody keeps it.
    let pulls::Listed {
        open: listed,
        requested,
    } = client
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
        let asks = requested.contains(&pull.number);
        let (row, created) = pulls::record(store, &integration, pull, "forge", None, None).await?;
        let row = match (asks, created) {
            (true, _) => note_review_request(store, &client, &integration, row, true).await?,
            // Born of the open list alone: it never asked for my review.
            (false, true) => mark_not_requested(store, row).await?,
            (false, false) => note_review_request(store, &client, &integration, row, false).await?,
        };
        changed.push(row);
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
        let row = pulls::record(store, &integration, pull, "forge", None, existing_id)
            .await?
            .0;
        changed.push(note_review_request(store, &client, &integration, row, false).await?);
    }
    after_fetch(store, handoff, &integration, &client, &slug, &changed).await;
    issues_moved(handoff, &integration, &client).await;
    Ok(())
}

/// Read the repository's open issues, and publish `issues_changed` where
/// they moved since the last read (028): the desktop's issues come from the
/// forge on every read, so this is what tells it to read them again. A read
/// that fails tells nothing, and the next fetch reads them again.
async fn issues_moved(handoff: &Handoff, integration: &ForgeIntegration, client: &ForgeClient) {
    use std::hash::{Hash, Hasher};
    let repository = format!(
        "{}/{}/{}",
        integration.host, integration.owner, integration.name
    );
    let issues = match client.list_open_issues(&repository, None).await {
        Ok(issues) => issues,
        Err(error) => {
            warn!(%error, repository = %integration.repository_id, "cannot read the open issues");
            return;
        }
    };
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(&issues)
        .unwrap_or_default()
        .hash(&mut hasher);
    let seen = hasher.finish();
    let moved = handoff
        .issues_seen
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(integration.repository_id.clone(), seen)
        != Some(seen);
    if moved {
        handoff.bus.issues_changed(&integration.repository_id);
    }
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

/// Whether the request `row` wants a review session of its own: an open
/// request out of draft, in a repository whose integration names a
/// `review_model`, that asks for my review — or one of mine whose review the
/// user asked Ariadne for (029). The author of the task that opened a
/// request of mine keeps it in its own session (005).
pub(crate) fn wants_session(row: &PullRequest, integration: &ForgeIntegration) -> bool {
    // A request that asks for my review runs on the repository's review
    // pin; one of mine runs on the pin the user picked when asking.
    let pinned = match row.role.as_str() {
        "reviewer" => row.review_requested && integration.review_model.is_some(),
        _ => row.review_asked && row.review_model.is_some(),
    };
    pinned && row.state == "open" && integration.enabled && !row.draft
}

/// A request the open list holds and nothing ever asked my review on: no
/// timeline is read for it, since it never asked.
async fn mark_not_requested(store: &Store, row: PullRequest) -> Result<PullRequest, String> {
    if row.role != "reviewer" || row.tracked_by != "forge" || !row.review_requested {
        return Ok(row);
    }
    store
        .set_pull_request_review_requested(&row.id, false)
        .await
        .map_err(|e| e.to_string())
}

/// Record whether a request I review asks for my review (029). The list of
/// review requests that holds it says yes. One it no longer holds and that
/// asked before is asked of the forge (`ForgeClient::review_still_requested`):
/// GitHub drops a request from the list once I reviewed it, and a request
/// withdrawn after that is told apart only by its timeline. One that never
/// asked is not asked about. A request tracked by hand is left as it is,
/// since no list ever holds it. Where the forge cannot answer, the flag
/// stays as it was, and the next fetch asks again.
async fn note_review_request(
    store: &Store,
    client: &ForgeClient,
    integration: &ForgeIntegration,
    row: PullRequest,
    listed: bool,
) -> Result<PullRequest, String> {
    if row.role != "reviewer" || row.tracked_by != "forge" {
        return Ok(row);
    }
    if !listed && row.state == "open" && !row.review_requested {
        return Ok(row);
    }
    let requested = match listed || row.state != "open" {
        true => listed,
        false => {
            let slug = format!(
                "{}/{}/{}",
                integration.host, integration.owner, integration.name
            );
            let login = integration.login.as_deref().unwrap_or_default();
            match client
                .review_still_requested(&slug, row.number, login)
                .await
            {
                Ok(requested) => requested,
                Err(error) => {
                    warn!(%error, pull_request = %row.id, "cannot read whether my review is still asked");
                    return Ok(row);
                }
            }
        }
    };
    if row.review_requested == requested {
        return Ok(row);
    }
    store
        .set_pull_request_review_requested(&row.id, requested)
        .await
        .map_err(|e| e.to_string())
}

/// The hand-off after a repository fetch (026): read the details of every
/// open row that has a session or wants one, and of every open row a task
/// opened, whose author keeps it (005); store its comments and checks, and
/// tell the scheduler about every row the fetch changed.
///
/// A detail fetch that fails leaves the row as the list fetch wrote it:
/// the next fetch reads it again.
async fn after_fetch(
    store: &Store,
    handoff: &Handoff,
    integration: &ForgeIntegration,
    client: &ForgeClient,
    slug: &str,
    changed_rows: &[PullRequest],
) {
    let login = integration.login.as_deref().unwrap_or_default();
    for row in changed_rows.iter().filter(|row| row.state == "open") {
        let has_session = store
            .list_sessions(SessionFilter {
                pull_request_id: Some(row.id.clone()),
                live_only: true,
                ..Default::default()
            })
            .await
            .is_ok_and(|live| !live.is_empty());
        let kept_by_task = row.role == "author" && row.origin_task_id.is_some();
        if !has_session && !kept_by_task && !wants_session(row, integration) {
            continue;
        }
        if let Err(error) = details(store, handoff, integration, client, slug, row, login).await {
            warn!(%error, pull_request = %row.id, "cannot read the details of a pull request");
        }
    }
    for row in changed_rows {
        wake_scheduler(&handoff.scheduler, &row.id);
    }
}

/// One request's details, read and stored.
async fn details(
    store: &Store,
    handoff: &Handoff,
    integration: &ForgeIntegration,
    client: &ForgeClient,
    slug: &str,
    row: &PullRequest,
    login: &str,
) -> Result<(), String> {
    let details = client.details(slug, row.number, handoff.details).await?;
    if details.pull.number != row.number {
        return Err("the forge returned another request number".into());
    }
    if !still_enabled(store, integration).await? {
        return Ok(());
    }
    pulls::record(
        store,
        integration,
        details.pull,
        "forge",
        None,
        Some(row.id.clone()),
    )
    .await?;
    store
        .upsert_pull_request_comments(&row.id, &details.comments, login)
        .await
        .map_err(|e| e.to_string())?;
    let failed = serde_json::to_string(&details.failed_checks).map_err(|e| e.to_string())?;
    store
        .set_pull_request_details(&row.id, &failed, details.behind_base)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
