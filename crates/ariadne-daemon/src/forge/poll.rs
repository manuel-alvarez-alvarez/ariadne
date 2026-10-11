//! Timer fallback and explicit repository wakes. Each repository has one worker.
use super::live::{Details, Live, LivePulls};
use super::{ForgeClient, pulls};
use crate::{bus::EventBus, config::Config, scheduler::SchedEvent, webhooks::Public};
use ariadne_api::stream::DomainEvent;
use ariadne_store::{ForgeIntegration, PullRequest, PullRequestFilter, PullRequestRow, Store};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tracing::warn;

/// Prefixed onto a fetch's stored error once the forge CLI's own sign-in
/// check, run reactively after the fetch already failed, confirms it is
/// signed out — the recovery producer's deterministic word that this one
/// fetch error is an `access` cause rather than a transient one
/// (`crate::attention::recovery`).
pub const FORGE_SIGNED_OUT: &str = "the forge CLI is not signed in";

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
    /// What each repository's last fetch read of its requests held, so a
    /// fetch that finds them moved says so to the desktop (026).
    pulls_seen: Arc<std::sync::Mutex<HashMap<String, u64>>>,
    /// Where each read of a request Ariadne works on is left (026).
    live: LivePulls,
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
    live: LivePulls,
    every: Duration,
    details: Duration,
) -> ForgePoll {
    let scheduler: Waker = Arc::default();
    let handoff = Handoff {
        details,
        scheduler: scheduler.clone(),
        bus: events.clone(),
        issues_seen: Arc::default(),
        pulls_seen: Arc::default(),
        live,
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
        // With the integration off nothing reads the forge for its
        // requests: the scheduler ends the work of each but a task's,
        // whose author keeps it until the integration is on again (026).
        match store
            .list_pull_requests(PullRequestFilter {
                repository_id: Some(id.into()),
                ..Default::default()
            })
            .await
        {
            Ok(rows) => {
                for row in rows {
                    wake_scheduler(&handoff.scheduler, &row.id);
                }
            }
            Err(error) => {
                warn!(%error, repository = id, "cannot list the requests of a disabled integration")
            }
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
    let login = integration.login.clone().unwrap_or_default();
    // Every open request of the repository, and which of them ask for the
    // user's review.
    let pulls::Listed {
        open: listed,
        requested,
    } = match client.list_open_pull_requests(&slug, &login).await {
        Ok(listed) => listed,
        // Checking the CLI's own sign-in once the fetch it would have
        // carried has already failed is not a new watch over it — only a
        // reactive answer to a failure that already happened, read the
        // same way a human asked to look into it would: is the CLI even
        // signed in? The marker is written only where the CLI actually
        // ran that check and said no (`confirmed_signed_out`) — a missing
        // binary, a spawn or write failure, or a timeout proves nothing
        // either way, and marking it regardless would tell the user to
        // sign in when the real cause is, say, the CLI not being
        // installed at all (which `configuration` already names).
        Err(error) => {
            return Err(match client.confirmed_signed_out(&integration.host).await {
                true => format!("{FORGE_SIGNED_OUT}: {error}"),
                false => error,
            });
        }
    };
    // A request that asks for my review, out of draft, is one Ariadne keeps
    // a row of, whether or not the repository has a review pin (029): a
    // repository with none starts no session of its own, but the request
    // still needs its row, so the `pull_request` attention producer can
    // offer a human a manual start on it. Nothing else the lists hold is
    // kept.
    for pull in listed.iter().filter(|p| {
        requested.contains(&p.number)
            && !p.draft
            && pulls::role(&p.author_login, &integration) == "reviewer"
    }) {
        if !still_enabled(store, &integration).await? {
            return Ok(());
        }
        pulls::start_work(store, &integration, pull, None).await?;
    }
    let rows = store
        .list_pull_requests(PullRequestFilter {
            repository_id: Some(id.into()),
            ..Default::default()
        })
        .await
        .map_err(|e| e.to_string())?;
    let mut held = Vec::new();
    for row in rows {
        if !still_enabled(store, &integration).await? {
            return Ok(());
        }
        let listed_pull = listed.iter().find(|p| p.number == row.number).cloned();
        if let Err(error) = read_row(
            handoff,
            &client,
            &slug,
            &login,
            &row,
            listed_pull,
            requested.contains(&row.number),
        )
        .await
        {
            warn!(%error, pull_request = %row.id, "cannot read a pull request Ariadne works on");
        }
        if let Some(live) = handoff.live.get(&row.id) {
            held.push(live);
        }
        wake_scheduler(&handoff.scheduler, &row.id);
    }
    pulls_moved(handoff, &integration, &listed, &requested, &held);
    issues_moved(handoff, &integration, &client).await;
    Ok(())
}

/// Read one request Ariadne works on, and leave the read in memory (026):
/// the list's read where the list holds it, else a read of its own — a
/// merged or closed request, or one the list misses — and, while it is
/// open, its details: its comments, its failed checks and its base. A
/// detail read that fails keeps the details an earlier read found.
async fn read_row(
    handoff: &Handoff,
    client: &ForgeClient,
    slug: &str,
    login: &str,
    row: &PullRequestRow,
    listed: Option<pulls::ForgePullRequest>,
    asks: bool,
) -> Result<(), String> {
    let read = match listed {
        Some(pull) => pull,
        None => client.pull_request(slug, row.number).await?,
    };
    if read.number != row.number {
        return Err("the forge returned another request number".into());
    }
    let requested = review_requested(handoff, client, slug, login, row, &read, asks).await;
    if read.state != "open" {
        handoff.live.set_pull(&row.id, read, requested);
        return Ok(());
    }
    match client.details(slug, row.number, handoff.details).await {
        Ok(details) if details.pull.number == row.number => {
            let head_sha = details.pull.head_sha.clone();
            handoff.live.set(
                &row.id,
                Live {
                    pull: details.pull,
                    review_requested: requested,
                    details: Some(Details {
                        comments: details.comments,
                        failed_checks: details.failed_checks,
                        behind_base: details.behind_base,
                        head_sha,
                    }),
                },
            );
            Ok(())
        }
        Ok(_) => {
            handoff.live.set_pull(&row.id, read, requested);
            Err("the forge returned another request number".into())
        }
        Err(error) => {
            handoff.live.set_pull(&row.id, read, requested);
            Err(error)
        }
    }
}

/// Whether a request I review still asks for my review (029). The list of
/// review requests that holds it says yes, and an ended request no. One it
/// no longer holds, and that asked on the last read, is asked of the forge
/// (`ForgeClient::review_still_requested`): GitHub drops a request from the
/// list once I reviewed it, and a request withdrawn after that is told
/// apart only by its timeline. Where the forge cannot answer, the last read
/// stands. A request of mine asks for no review of mine.
async fn review_requested(
    handoff: &Handoff,
    client: &ForgeClient,
    slug: &str,
    login: &str,
    row: &PullRequestRow,
    read: &pulls::ForgePullRequest,
    asks: bool,
) -> bool {
    if row.role != "reviewer" || read.state != "open" {
        return false;
    }
    if asks {
        return true;
    }
    let before = handoff.live.get(&row.id).is_none_or(|l| l.review_requested);
    if !before {
        return false;
    }
    match client.review_still_requested(slug, row.number, login).await {
        Ok(requested) => requested,
        Err(error) => {
            warn!(%error, pull_request = %row.id, "cannot read whether my review is still asked");
            before
        }
    }
}

/// Publish `pull_requests_changed` where the repository's requests moved
/// since the last fetch (026): the open list, which of them ask for my
/// review, and what the requests Ariadne works on read now. The desktop
/// reads its lists off the forge, so this is what tells it to read again.
fn pulls_moved(
    handoff: &Handoff,
    integration: &ForgeIntegration,
    listed: &[pulls::ForgePullRequest],
    requested: &HashSet<i64>,
    held: &[Live],
) {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut requested: Vec<_> = requested.iter().collect();
    requested.sort();
    requested.hash(&mut hasher);
    for pull in listed.iter().chain(held.iter().map(|l| &l.pull)) {
        (
            pull.number,
            &pull.state,
            pull.draft,
            &pull.title,
            &pull.head_sha,
            &pull.checks,
            &pull.review_decision,
            &pull.mergeable,
            &pull.updated_at,
        )
            .hash(&mut hasher);
    }
    for live in held {
        if let Some(details) = &live.details {
            (details.behind_base, details.failed_checks.len()).hash(&mut hasher);
            for comment in &details.comments {
                (&comment.forge_id, &comment.body, comment.resolved).hash(&mut hasher);
            }
        }
    }
    let seen = hasher.finish();
    let moved = handoff
        .pulls_seen
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(integration.repository_id.clone(), seen)
        != Some(seen);
    if moved {
        handoff
            .bus
            .pull_requests_changed(&integration.repository_id);
    }
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
/// request out of draft, that asks for my review on a repository whose
/// integration names a `review_model`, or whose own row was given a pin by
/// an explicit manual start (029) — or one of mine whose review the user
/// asked Ariadne for. The author of the task that opened a request of mine
/// keeps it in its own session (005).
pub(crate) fn wants_session(row: &PullRequest, integration: &ForgeIntegration) -> bool {
    // A request that asks for my review runs on the repository's review
    // pin, or on one asked directly on the row where the repository has
    // none; one of mine runs on the pin the user picked when asking.
    let pinned = match row.role.as_str() {
        "reviewer" => {
            row.review_requested
                && (integration.review_model.is_some() || row.review_model.is_some())
        }
        _ => row.review_asked && row.review_model.is_some(),
    };
    pinned && row.state == "open" && integration.enabled && !row.draft
}
