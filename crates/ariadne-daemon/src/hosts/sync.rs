//! The push of this daemon's catalogs and settings to each host (037).
//!
//! One task per configured host. When the host is `ready` it pushes every
//! skill, workflow and learned permission, the AI permission settings and the
//! settings of each repository the host has a checkout of. After that it
//! pushes each local change as the bus carries it. Every push goes through
//! the host's own API, so the host checks it as it checks any other write.
//! A failed push is tried again on its own with a capped backoff for as long
//! as the host stays `ready`, and holds back no other push. Nothing is ever read back from the host into this
//! daemon: writes go to the local daemon, and flow one way.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use ariadne_api::hosts::{MissingRepositoryDto, SyncState};
use ariadne_api::permissions::{
    AiPermissionsStatusDto, Flavour, LearnedPermissionDto, LearnedPermissionLevel,
    LearnedPermissionsResponse, RecordLearnedPermissionRequest, UpdateAiPermissionsRequest,
};
use ariadne_api::repositories::{ForgeUpdate, RepositoryDto, UpdateRepositoryRequest};
use ariadne_api::skills::{CreateSkillRequest, SkillDto, SkillFile, UpdateSkillRequest};
use ariadne_api::stream::DomainEvent;
use ariadne_api::workflows::{CreateWorkflowRequest, UpdateWorkflowRequest, WorkflowDto};
use ariadne_client::{Client, ClientError};
use axum::http::{Method, StatusCode};
use serde::de::DeserializeOwned;
use tokio::sync::broadcast::{self, error::RecvError, error::TryRecvError};
use tokio::time::{Instant, sleep_until};
use tracing::warn;

use super::Inner;
use crate::bus::BusEvent;
use crate::http::convert::{learned_permission_dto, repository_dto, skill_dto, workflow_dto};

/// The longest wait between two tries of one push.
const BACKOFF_CAP: Duration = Duration::from_secs(60);

/// Reapply settings and permissions after an explicit repository population.
/// A connection that is not ready gets the normal initial push when it connects.
pub(super) async fn after_registration(hosts: &super::Hosts, name: &str) -> Result<(), String> {
    let owner = hosts
        .0
        .extra
        .lock()
        .unwrap()
        .iter()
        .find(|h| h.0.cfg.hosts.iter().any(|(n, _)| n == name))
        .cloned()
        .unwrap_or_else(|| hosts.clone());
    let Some(index) = owner.0.cfg.hosts.iter().position(|(n, _)| n == name) else {
        return Ok(());
    };
    let Some(endpoint) = owner.0.ready[index].borrow().clone() else {
        return Ok(());
    };
    let mut pusher = Pusher {
        inner: &owner.0,
        index,
        client: super::client_for(&endpoint),
        last_ai: None,
        pairs: None,
        bundles: HashMap::new(),
        warnings: HashMap::new(),
    };
    for push in pusher.everything().await? {
        pusher.push(&push).await?;
    }
    Ok(())
}

/// One thing to push to a host.
#[derive(Clone)]
enum Push {
    /// Everything: the host has just become `ready`, or this task fell
    /// behind the bus and cannot tell what it missed. Run as a push of each
    /// item.
    All,
    Skill(SkillDto),
    SkillDeleted(String),
    Workflow(WorkflowDto),
    WorkflowDeleted(String),
    /// The settings of every repository the host has a checkout of, and the
    /// list of those it lacks. Read afresh when pushed.
    Repositories,
    Learned(LearnedPermissionDto),
    LearnedDeleted(LearnedPermissionDto),
    Ai(AiChoice),
}

/// The item a push is about: a later push of the same item replaces a
/// failed one.
#[derive(PartialEq)]
enum Item<'a> {
    All,
    Skill(&'a str),
    Workflow(&'a str),
    Repositories,
    Learned(&'a str, &'a str, LearnedPermissionLevel, &'a str),
    Ai,
}

impl Push {
    fn is_deletion(&self) -> bool {
        matches!(
            self,
            Self::SkillDeleted(_) | Self::WorkflowDeleted(_) | Self::LearnedDeleted(_)
        )
    }

    fn item(&self) -> Item<'_> {
        match self {
            Push::All => Item::All,
            Push::Skill(skill) => Item::Skill(&skill.name),
            Push::SkillDeleted(name) => Item::Skill(name),
            Push::Workflow(workflow) => Item::Workflow(&workflow.name),
            Push::WorkflowDeleted(name) => Item::Workflow(name),
            Push::Repositories => Item::Repositories,
            Push::Learned(row) | Push::LearnedDeleted(row) => {
                Item::Learned(&row.repository_id, &row.tool_name, row.level, &row.key)
            }
            Push::Ai(_) => Item::Ai,
        }
    }
}

/// What the user chose for the AI permission model. The device is not among
/// them: it is the machine's, and the host picks its own best one.
#[derive(Debug, Clone, PartialEq)]
struct AiChoice {
    enabled: bool,
    /// The pair set by hand; `None` where the flavour's default is in force.
    thresholds: Option<(f64, f64)>,
    flavour: Flavour,
}

impl From<&AiPermissionsStatusDto> for AiChoice {
    fn from(status: &AiPermissionsStatusDto) -> Self {
        Self {
            enabled: status.enabled,
            thresholds: (!status.thresholds_default)
                .then_some((status.allow_threshold, status.deny_threshold)),
            flavour: status.flavour,
        }
    }
}

/// Keep host `index` in sync until the bus closes.
pub(super) async fn run(
    inner: Arc<Inner>,
    index: usize,
    mut events: broadcast::Receiver<BusEvent>,
) {
    let mut ready = inner.ready[index].subscribe();
    let base = inner.timeouts.host_sync_retry;
    let mut pushes = Pushes {
        queue: VecDeque::new(),
        failed: Vec::new(),
        pending_deletions: Vec::new(),
    };
    loop {
        // Until the host is ready, a change waits for the push of
        // everything that its readiness starts.
        let endpoint = loop {
            if let Some(endpoint) = ready.borrow_and_update().clone() {
                break endpoint;
            }
            tokio::select! {
                changed = ready.changed() => if changed.is_err() { return },
                event = events.recv() => match event {
                    Ok(event) => pushes.enqueue(&event.event),
                    Err(RecvError::Lagged(_)) => pushes.enqueue_all(),
                    Err(RecvError::Closed) => return,
                },
            }
        };
        let mut pusher = Pusher {
            inner: &inner,
            index,
            client: super::client_for(&endpoint),
            last_ai: None,
            pairs: None,
            bundles: HashMap::new(),
            warnings: HashMap::new(),
        };
        pushes.failed.clear();
        pushes.enqueue_all();
        let mut scan = tokio::time::interval(inner.timeouts.host_check);
        scan.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut backoff = base;
        // When the failed pushes are next tried; `None` while none failed.
        let mut retry_at: Option<Instant> = None;
        'ready: loop {
            if ready.has_changed().unwrap_or(true) {
                break 'ready;
            }
            loop {
                match events.try_recv() {
                    Ok(event) => pushes.enqueue(&event.event),
                    Err(TryRecvError::Lagged(_)) => pushes.enqueue_all(),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Closed) => return,
                }
            }
            pushes.show(&inner, index, &pusher.warnings);
            if let Some(push) = pushes.queue.pop_front() {
                tokio::select! {
                    biased;
                    _ = ready.changed() => break 'ready,
                    () = pushes.run(&mut pusher, push) => {}
                }
                if pushes.failed.is_empty() {
                    backoff = base;
                    retry_at = None;
                } else if retry_at.is_none() {
                    retry_at = Some(Instant::now() + backoff);
                }
                pushes.show(&inner, index, &pusher.warnings);
                continue;
            }
            let Some(at) = retry_at else {
                tokio::select! {
                    _ = scan.tick() => pusher.scan(&mut pushes).await,
                    changed = ready.changed() => {
                        if changed.is_err() { return }
                        if ready.borrow().as_deref() != Some(endpoint.as_str()) { break 'ready }
                    }
                    event = events.recv() => match event {
                        Ok(event) => pushes.enqueue(&event.event),
                        Err(RecvError::Lagged(_)) => pushes.enqueue_all(),
                        Err(RecvError::Closed) => return,
                    },
                }
                continue;
            };
            tokio::select! {
                _ = scan.tick() => pusher.scan(&mut pushes).await,
                () = sleep_until(at) => {
                    tokio::select! {
                        biased;
                        _ = ready.changed() => break 'ready,
                        () = pushes.retry(&mut pusher) => {}
                    }
                    backoff = (backoff * 2).min(BACKOFF_CAP);
                    retry_at = (!pushes.failed.is_empty()).then(|| Instant::now() + backoff);
                    if pushes.failed.is_empty() {
                        backoff = base;
                    }
                }
                changed = ready.changed() => {
                    if changed.is_err() { return }
                    if ready.borrow().as_deref() != Some(endpoint.as_str()) { break 'ready }
                }
                event = events.recv() => match event {
                    Ok(event) => pushes.enqueue(&event.event),
                    Err(RecvError::Lagged(_)) => pushes.enqueue_all(),
                    Err(RecvError::Closed) => return,
                },
            }
        }
        // The host left `ready`: what is left waits for the next push of
        // everything.
        inner.set_sync(index, |sync| {
            sync.state = SyncState::Pending;
            sync.error = None;
        });
    }
}

/// What is left to push to a `ready` host.
struct Pushes {
    /// The pushes not tried yet, in the order of the changes.
    queue: VecDeque<Push>,
    /// The pushes that failed, each with why, tried again on their own
    /// after the backoff. A failed push holds back no other one.
    failed: Vec<(Push, String)>,
    /// Deletes cannot be reconstructed from the current catalog after reconnect.
    /// Keep them until delivered or superseded, including during a cancelled retry.
    pending_deletions: Vec<Push>,
}

impl Pushes {
    /// Queue the push a local change asks for, if it asks for one. A newer
    /// push of the same item replaces a failed one, here and again when it
    /// runs.
    fn enqueue(&mut self, event: &DomainEvent) {
        let push = match event {
            DomainEvent::SkillCreated(skill) | DomainEvent::SkillUpdated(skill) => {
                Push::Skill(skill.clone())
            }
            DomainEvent::SkillDeleted(deleted) => Push::SkillDeleted(deleted.id.clone()),
            DomainEvent::WorkflowCreated(workflow) | DomainEvent::WorkflowUpdated(workflow) => {
                Push::Workflow(workflow.clone())
            }
            DomainEvent::WorkflowDeleted(deleted) => Push::WorkflowDeleted(deleted.id.clone()),
            DomainEvent::RepositoryCreated(_)
            | DomainEvent::RepositoryUpdated(_)
            | DomainEvent::RepositoryDeleted(_) => Push::Repositories,
            DomainEvent::LearnedPermissionCreated(row)
            | DomainEvent::LearnedPermissionUpdated(row) => Push::Learned(row.clone()),
            DomainEvent::LearnedPermissionDeleted(row) => Push::LearnedDeleted(row.clone()),
            DomainEvent::AiPermissionsUpdated(status) => Push::Ai(AiChoice::from(status)),
            _ => return,
        };
        self.pending_deletions
            .retain(|pending| pending.item() != push.item());
        if push.is_deletion() {
            self.pending_deletions.push(push.clone());
        }
        self.supersede(&push);
        // A push that reads its state afresh covers a later one of its kind.
        let covered = matches!(push, Push::Repositories)
            && self
                .queue
                .iter()
                .any(|queued| matches!(queued, Push::All | Push::Repositories));
        if !covered {
            self.queue.push_back(push);
        }
    }

    /// Push everything again: this task fell behind the bus and cannot tell
    /// what it missed.
    fn enqueue_all(&mut self) {
        self.queue.clear();
        self.queue.push_back(Push::All);
        self.queue.extend(self.pending_deletions.iter().cloned());
    }

    /// Drop the failed push of the item `push` is about.
    fn supersede(&mut self, push: &Push) {
        let item = push.item();
        self.failed.retain(|(failed, _)| failed.item() != item);
    }

    /// Run one push. The push of everything puts a push of each item at the
    /// head of the queue, so one item the host refuses holds back no other.
    async fn run(&mut self, pusher: &mut Pusher<'_>, push: Push) {
        if let Push::All = push {
            match pusher.everything().await {
                Ok(items) => {
                    for item in items.into_iter().rev() {
                        self.supersede(&item);
                        self.queue.push_front(item);
                    }
                }
                Err(error) => self.fail(pusher, push, error),
            }
            return;
        }
        // The item's older failed push is spent: this one carries the newer
        // state, and a retry of the older would write it back over this.
        self.supersede(&push);
        match pusher.push(&push).await {
            Ok(()) if push.is_deletion() => self
                .pending_deletions
                .retain(|pending| pending.item() != push.item()),
            Ok(()) => {}
            Err(error) => self.fail(pusher, push, error),
        }
    }

    fn fail(&mut self, pusher: &Pusher<'_>, push: Push, error: String) {
        warn!(host = %pusher.inner.cfg.hosts[pusher.index].0, %error, "host sync failed");
        self.supersede(&push);
        self.failed.push((push, error));
    }

    /// Try each failed push again, on its own and in the order they failed.
    async fn retry(&mut self, pusher: &mut Pusher<'_>) {
        for (push, _) in std::mem::take(&mut self.failed) {
            self.run(pusher, push).await;
        }
    }

    /// Publish where the sync stands: `failed` while any push has failed,
    /// else `pending` while one waits, else `in_sync`.
    fn show(&self, inner: &Inner, index: usize, warnings: &HashMap<String, Vec<String>>) {
        let (state, error) = match self.failed.last() {
            Some((_, error)) => (SyncState::Failed, Some(error.clone())),
            None if self.queue.is_empty() => {
                let mut messages: Vec<_> = warnings
                    .iter()
                    .flat_map(|(name, rows)| {
                        rows.iter().map(move |row| format!("skill {name}: {row}"))
                    })
                    .collect();
                messages.sort();
                (
                    SyncState::InSync,
                    (!messages.is_empty()).then(|| messages.join("; ")),
                )
            }
            None => (SyncState::Pending, None),
        };
        inner.set_sync(index, |sync| {
            sync.state = state;
            sync.error = error;
        });
    }
}

/// Pushes to one `ready` host.
struct Pusher<'a> {
    inner: &'a Inner,
    index: usize,
    client: Client,
    /// The AI choice the host last took: the status event also moves with
    /// every step of an install, which changes no choice.
    last_ai: Option<AiChoice>,
    /// The local repository ids paired with the host's, as the last push
    /// of the repositories read them.
    pairs: Option<HashMap<String, String>>,
    bundles: HashMap<String, (String, Vec<SkillFile>)>,
    warnings: HashMap<String, Vec<String>>,
}

impl Pusher<'_> {
    async fn scan(&mut self, pushes: &mut Pushes) {
        let _guard = crate::agents::SKILLS_SYNC_LOCK.lock().await;
        let Ok(skills) = self.inner.store.list_skills().await else {
            return;
        };
        for skill in skills {
            // Failed items belong to the retry timer. Scanning must not
            // requeue them or read their supporting files before it expires.
            if pushes
                .failed
                .iter()
                .any(|(failed, _)| failed.item() == Item::Skill(&skill.name))
            {
                continue;
            }
            let skill = skill_dto(skill);
            let current =
                crate::skill_bundle::snapshot(&self.inner.cfg.skills_dir.join(&skill.name));
            let unchanged = current.as_ref().ok().is_some_and(|files| {
                self.bundles.get(&skill.name) == Some(&(skill.document.clone(), files.clone()))
            });
            if !unchanged
                && !pushes
                    .queue
                    .iter()
                    .any(|p| p.item() == Item::Skill(&skill.name))
            {
                pushes.queue.push_back(Push::Skill(skill));
            }
        }
    }

    async fn push(&mut self, push: &Push) -> Result<(), String> {
        match push {
            Push::All => unreachable!("the push of everything runs as one push per item"),
            Push::Skill(skill) => self.skill(skill).await,
            Push::SkillDeleted(name) => {
                self.delete(&format!("/v1/skills/{name}"))
                    .await
                    .map_err(|error| format!("skill {name}: {error}"))?;
                self.bundles.remove(name);
                self.warnings.remove(name);
                Ok(())
            }
            Push::Workflow(workflow) => self.workflow(workflow).await,
            Push::WorkflowDeleted(name) => self
                .delete(&format!("/v1/workflows/{name}"))
                .await
                .map_err(|error| format!("workflow {name}: {error}")),
            Push::Repositories => {
                self.pairs = Some(self.repositories().await?);
                Ok(())
            }
            Push::Learned(row) => {
                let pairs = self.paired().await?;
                self.learned(row, &pairs).await?;
                self.count_skipped(&pairs).await
            }
            Push::LearnedDeleted(row) => {
                let pairs = self.paired().await?;
                self.learned_deleted(row, &pairs).await?;
                self.count_skipped(&pairs).await
            }
            Push::Ai(choice) => self.ai(choice).await,
        }
    }

    /// A push of each item, in the order one item can need another: skills
    /// before the workflows that name them, workflows before the
    /// repositories that default to them, and repositories before their
    /// learned permissions.
    async fn everything(&mut self) -> Result<Vec<Push>, String> {
        let store = &self.inner.store;
        let local = |error: ariadne_store::StoreError| format!("reading the local store: {error}");
        let mut pushes: Vec<Push> = Vec::new();
        for skill in store.list_skills().await.map_err(local)? {
            pushes.push(Push::Skill(skill_dto(skill)));
        }
        for workflow in store.list_workflows().await.map_err(local)? {
            pushes.push(Push::Workflow(workflow_dto(workflow)));
        }
        pushes.push(Push::Repositories);
        for row in store.list_learned_permissions(None).await.map_err(local)? {
            pushes.push(Push::Learned(learned_permission_dto(row)));
        }
        let settings = store.ai_permission_settings().await.map_err(local)?;
        let flavour: Flavour = serde_json::from_value(serde_json::Value::String(settings.flavour))
            .map_err(|error| format!("reading the local AI flavour: {error}"))?;
        self.last_ai = None;
        pushes.push(Push::Ai(AiChoice {
            enabled: settings.enabled,
            thresholds: settings
                .thresholds_hand_set
                .then_some((settings.allow_threshold, settings.deny_threshold)),
            flavour,
        }));
        Ok(pushes)
    }

    /// The pairs of the last push of the repositories, else read afresh.
    async fn paired(&mut self) -> Result<HashMap<String, String>, String> {
        if let Some(pairs) = &self.pairs {
            return Ok(pairs.clone());
        }
        let pairs = self.pairs().await?.0;
        self.pairs = Some(pairs.clone());
        Ok(pairs)
    }

    /// Write the skill's document over the host's, or create it there.
    async fn skill(&mut self, skill: &SkillDto) -> Result<(), String> {
        let current = self
            .inner
            .store
            .get_skill(&skill.name)
            .await
            .map_err(|e| e.to_string())?;
        let skill = &skill_dto(current);
        let files = {
            let _guard = crate::agents::SKILLS_SYNC_LOCK.lock().await;
            crate::skill_bundle::snapshot(&self.inner.cfg.skills_dir.join(&skill.name))
                .map_err(|e| format!("skill {}: {e}", skill.name))?
        };
        let update = UpdateSkillRequest {
            document: Some(skill.document.clone()),
            files: Some(files.clone()),
        };
        let create = CreateSkillRequest {
            name: skill.name.clone(),
            document: skill.document.clone(),
        };
        let path = format!("/v1/skills/{}", skill.name);
        match self.client.put_json::<SkillDto, _>(&path, &update).await {
            Err(error) if not_found(&error) => {
                self.client
                    .post_json::<SkillDto, _>("/v1/skills", &create)
                    .await
                    .map_err(|e| e.to_string())?;
                self.client
                    .put_json::<SkillDto, _>(&path, &update)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            other => {
                other.map_err(|e| format!("skill {}: {e}", skill.name))?;
            }
        }
        let root = self.inner.cfg.skills_dir.join(&skill.name);
        let latest = {
            let _guard = crate::agents::SKILLS_SYNC_LOCK.lock().await;
            crate::skill_bundle::snapshot(&root)?
        };
        let current = self
            .inner
            .store
            .get_skill(&skill.name)
            .await
            .map_err(|e| e.to_string())?;
        if latest != files || current.document_text() != skill.document {
            return Err(format!("skill {} changed during transfer", skill.name));
        }
        self.warnings.insert(
            skill.name.clone(),
            crate::skill_bundle::warnings(&root, &skill.document, &files),
        );
        self.bundles
            .insert(skill.name.clone(), (skill.document.clone(), files));
        Ok(())
    }

    /// Write the workflow's document over the host's, or create it there.
    async fn workflow(&self, workflow: &WorkflowDto) -> Result<(), String> {
        let update = UpdateWorkflowRequest {
            document: Some(workflow.document.clone()),
        };
        let create = CreateWorkflowRequest {
            name: workflow.name.clone(),
            document: workflow.document.clone(),
        };
        self.upsert::<WorkflowDto, _, _>(
            &format!("/v1/workflows/{}", workflow.name),
            &update,
            "/v1/workflows",
            &create,
        )
        .await
        .map_err(|error| format!("workflow {}: {error}", workflow.name))
    }

    /// `PUT path`, and `POST create_path` where the host has no such item.
    async fn upsert<T: DeserializeOwned, U: serde::Serialize, C: serde::Serialize>(
        &self,
        path: &str,
        update: &U,
        create_path: &str,
        create: &C,
    ) -> Result<(), ClientError> {
        match self.client.put_json::<T, _>(path, update).await {
            Err(error) if not_found(&error) => {
                self.client.post_json::<T, _>(create_path, create).await?;
                Ok(())
            }
            other => other.map(drop),
        }
    }

    /// `DELETE path`. An item the host does not have is already gone.
    async fn delete(&self, path: &str) -> Result<(), ClientError> {
        match self
            .client
            .send_no_content(Method::DELETE, path, None::<&()>)
            .await
        {
            Err(error) if not_found(&error) => Ok(()),
            other => other,
        }
    }

    /// The local repositories paired with the host's, and the host's
    /// repositories as read.
    async fn pairs(&self) -> Result<(HashMap<String, String>, Vec<Pair>), String> {
        let local: Vec<RepositoryDto> = self
            .inner
            .store
            .list_repositories()
            .await
            .map_err(|error| format!("reading the local store: {error}"))?
            .into_iter()
            .map(repository_dto)
            .collect();
        let remote: Vec<RepositoryDto> = self
            .client
            .get_json("/v1/repositories")
            .await
            .map_err(|error| format!("listing repositories: {error}"))?;
        let pairs = pair(local, remote);
        let ids = pairs
            .iter()
            .filter_map(|pair| Some((pair.local.id.clone(), pair.remote.as_ref()?.id.clone())))
            .collect();
        Ok((ids, pairs))
    }

    /// Push the settings of each paired repository, and record the local
    /// ones the host has no checkout of. Answers the local id of each
    /// paired repository with the host's id for it.
    async fn repositories(&self) -> Result<HashMap<String, String>, String> {
        let (ids, pairs) = self.pairs().await?;
        let mut missing = Vec::new();
        // A host may refuse one repository's settings: the others still go,
        // and the missing checkouts are still recorded.
        let mut refused = Vec::new();
        for Pair { local, remote } in &pairs {
            let Some(remote) = remote else {
                missing.push(MissingRepositoryDto {
                    id: local.id.clone(),
                    forge: local
                        .forge
                        .as_ref()
                        .map(|forge| format!("{}/{}", forge.owner, forge.name)),
                });
                continue;
            };
            let enabled = local.forge.as_ref().map(|forge| forge.enabled);
            let request = UpdateRepositoryRequest {
                default_workflow: Some(local.default_workflow.clone()),
                path: None,
                // Sent only where it moves: a sent branch is checked against
                // the host's checkout.
                base_branch: (local.base_branch != remote.base_branch)
                    .then(|| local.base_branch.clone()),
                // An empty description clears the host's.
                description: Some(local.description.clone().unwrap_or_default()),
                permission_mode: Some(local.permission_mode),
                // Sent only where it moves: turning the integration on asks
                // the host's forge CLI who is signed in.
                forge: (enabled != remote.forge.as_ref().map(|forge| forge.enabled))
                    .then_some(ForgeUpdate { enabled }),
            };
            if let Err(error) = self
                .client
                .put_json::<RepositoryDto, _>(&format!("/v1/repositories/{}", remote.id), &request)
                .await
            {
                refused.push(format!("repository {}: {error}", local.id));
            }
        }
        self.inner
            .set_sync(self.index, |sync| sync.missing_repositories = missing);
        self.count_skipped(&ids).await?;
        if refused.is_empty() {
            Ok(ids)
        } else {
            Err(refused.join("; "))
        }
    }

    /// Record how many local learned permissions are on a repository the
    /// host has no checkout of. They are not pushed, and leave the state as
    /// it is.
    async fn count_skipped(&self, pairs: &HashMap<String, String>) -> Result<(), String> {
        let rows = self
            .inner
            .store
            .list_learned_permissions(None)
            .await
            .map_err(|error| format!("reading the local store: {error}"))?;
        let skipped = rows
            .iter()
            .filter(|row| !pairs.contains_key(&row.repository_id))
            .count();
        let skipped = u32::try_from(skipped).unwrap_or(u32::MAX);
        self.inner
            .set_sync(self.index, |sync| sync.skipped_permissions = skipped);
        Ok(())
    }

    /// Record the row on the host's checkout of its repository. A row whose
    /// repository the host has no checkout of is skipped, and counted.
    async fn learned(
        &self,
        row: &LearnedPermissionDto,
        pairs: &HashMap<String, String>,
    ) -> Result<(), String> {
        let Some(repository_id) = pairs.get(&row.repository_id) else {
            return Ok(());
        };
        let request = RecordLearnedPermissionRequest {
            repository_id: repository_id.clone(),
            tool_name: row.tool_name.clone(),
            key: row.key.clone(),
            level: row.level,
            family: row.family.clone(),
            risk_tags: row.risk_tags.clone(),
            scope: row.scope,
            tool_call: row.tool_call.clone(),
            options: row.options.clone(),
            selected_option: row.selected_option.clone(),
            target: row.target,
            output: row.output.clone(),
        };
        self.client
            .post_json::<LearnedPermissionDto, _>("/v1/permissions/learned", &request)
            .await
            .map(drop)
            .map_err(|error| format!("learned permission {}: {error}", row.id))
    }

    /// Delete the host's row with the same repository, tool name, level and
    /// key.
    async fn learned_deleted(
        &self,
        row: &LearnedPermissionDto,
        pairs: &HashMap<String, String>,
    ) -> Result<(), String> {
        let Some(repository_id) = pairs.get(&row.repository_id) else {
            return Ok(());
        };
        let failed = |error: ClientError| format!("learned permission {}: {error}", row.id);
        let rows: LearnedPermissionsResponse = self
            .client
            .get_json(&format!(
                "/v1/permissions/learned?repository={repository_id}"
            ))
            .await
            .map_err(failed)?;
        let Some(remote) = rows.items.iter().find(|remote| {
            remote.tool_name == row.tool_name && remote.level == row.level && remote.key == row.key
        }) else {
            return Ok(());
        };
        self.delete(&format!("/v1/permissions/learned/{}", remote.id))
            .await
            .map_err(failed)
    }

    async fn ai(&mut self, choice: &AiChoice) -> Result<(), String> {
        if self.last_ai.as_ref() == Some(choice) {
            return Ok(());
        }
        let request = UpdateAiPermissionsRequest {
            enabled: Some(choice.enabled),
            allow_threshold: choice.thresholds.map(|(allow, _)| allow),
            deny_threshold: choice.thresholds.map(|(_, deny)| deny),
            default_thresholds: choice.thresholds.is_none(),
            flavour: Some(choice.flavour),
            device: None,
        };
        self.client
            .put_json::<AiPermissionsStatusDto, _>("/v1/permissions/ai", &request)
            .await
            .map_err(|error| format!("AI permission settings: {error}"))?;
        self.last_ai = Some(choice.clone());
        Ok(())
    }
}

fn not_found(error: &ClientError) -> bool {
    matches!(error, ClientError::Api { status, .. } if *status == StatusCode::NOT_FOUND)
}

/// A local repository and the host's checkout of the same forge repository.
#[derive(Debug)]
pub(super) struct Pair {
    pub(super) local: RepositoryDto,
    pub(super) remote: Option<RepositoryDto>,
}

/// Pair each local repository with a host repository on the same forge
/// repository: one on the same base branch first, else the first one left.
/// Each host repository pairs once. A repository with no forge pairs with
/// none.
pub(super) fn pair(local: Vec<RepositoryDto>, mut remote: Vec<RepositoryDto>) -> Vec<Pair> {
    let forge = |repository: &RepositoryDto| {
        repository.forge.as_ref().map(|forge| {
            (
                forge.kind,
                forge.host.clone(),
                forge.owner.clone(),
                forge.name.clone(),
            )
        })
    };
    // Same base branch first, so a looser match cannot take a closer one's
    // checkout.
    let mut matched: Vec<Option<usize>> = vec![None; local.len()];
    for exact in [true, false] {
        for (at, repository) in local.iter().enumerate() {
            if matched[at].is_some() || forge(repository).is_none() {
                continue;
            }
            let taken = |candidate: &usize| matched.contains(&Some(*candidate));
            let found = (0..remote.len()).find(|candidate| {
                !taken(candidate)
                    && forge(&remote[*candidate]) == forge(repository)
                    && (!exact || remote[*candidate].base_branch == repository.base_branch)
            });
            matched[at] = found;
        }
    }
    let mut taken: Vec<Option<RepositoryDto>> = remote.drain(..).map(Some).collect();
    local
        .into_iter()
        .zip(matched)
        .map(|(local, at)| Pair {
            local,
            remote: at.and_then(|at| taken[at].take()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariadne_api::repositories::{ForgeDto, WebhookDto};
    use ariadne_core::{ForgeKind, PermissionMode};

    fn repository(id: &str, branch: &str, forge: Option<(&str, &str)>) -> RepositoryDto {
        RepositoryDto {
            default_workflow: "develop-review-merge".into(),
            id: id.into(),
            path: format!("/src/{id}"),
            base_branch: branch.into(),
            description: None,
            permission_mode: PermissionMode::Auto,
            forge: forge.map(|(owner, name)| ForgeDto {
                webhook: WebhookDto {
                    state: "none".into(),
                    url: None,
                    error: None,
                    last_delivery_at: None,
                    fetch_error: None,
                },
                kind: ForgeKind::Github,
                host: "github.com".into(),
                owner: owner.into(),
                name: name.into(),
                remote: "origin".into(),
                enabled: false,
                login: None,
            }),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn paired(pairs: &[Pair]) -> Vec<(&str, Option<&str>)> {
        pairs
            .iter()
            .map(|pair| {
                (
                    pair.local.id.as_str(),
                    pair.remote.as_ref().map(|remote| remote.id.as_str()),
                )
            })
            .collect()
    }

    #[test]
    fn a_repository_pairs_by_forge_and_prefers_its_own_base_branch() {
        let local = vec![
            repository("main", "main", Some(("acme", "widgets"))),
            repository("next", "next", Some(("acme", "widgets"))),
            repository("other", "main", Some(("acme", "gadgets"))),
            repository("plain", "main", None),
        ];
        let remote = vec![
            repository("far-next", "next", Some(("acme", "widgets"))),
            repository("far-main", "main", Some(("acme", "widgets"))),
            repository("far-plain", "main", None),
        ];
        assert_eq!(
            paired(&pair(local, remote)),
            [
                ("main", Some("far-main")),
                ("next", Some("far-next")),
                ("other", None),
                ("plain", None),
            ]
        );
    }

    #[test]
    fn a_forge_repository_on_another_branch_pairs_once() {
        let local = vec![
            repository("main", "main", Some(("acme", "widgets"))),
            repository("next", "next", Some(("acme", "widgets"))),
        ];
        let remote = vec![repository("far", "trunk", Some(("acme", "widgets")))];
        assert_eq!(
            paired(&pair(local, remote)),
            [("main", Some("far")), ("next", None)]
        );
    }
}
