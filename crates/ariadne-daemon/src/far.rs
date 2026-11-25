//! Far tasks (037): tasks that the orchestrator creates on another host.
//!
//! The goal's daemon keeps each far task in its own task list. A far task
//! stays `pending` here until every task it depends on has merged, on any
//! host. Then the daemon sends it to its host: the linked goal of the goal on
//! that host, and the task in it. The host answers both requests again with
//! what it made, so a send whose answer was lost is safe to repeat. From then
//! on the daemon mirrors the far task's status, step, branch, request URL and
//! merge commit from the host's event stream, until its state is final here.
//!
//! One worker per host does all of this. It follows the host's status from
//! the `host_updated` events of this daemon. While the host is not `ready`,
//! each far task on it keeps its last known state and is `host_unreachable`.
//! On the next `ready`, the worker opens the host's event stream, reads each
//! far task again, and clears `host_unreachable`. A [`Link`] epoch makes a
//! return at the same endpoint a next `ready` too.

mod posts;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ariadne_api::goals::{CreateGoalRequest, GoalDto, LinkedGoalDto};
use ariadne_api::hosts::HostDto;
use ariadne_api::stream::{DomainEvent, TaskUpdatedDto};
use ariadne_api::tasks::{AgentAssignment, CreateTaskRequest, LOCAL_HOST, TaskDto};
use ariadne_client::{Client, ClientError, SseEvent};
use ariadne_core::{Actor, GoalStatus, TaskStatus};
use ariadne_store::{FarTaskState, Repository, Store, Task};
use axum::http::StatusCode;
use tokio::sync::broadcast::{self, error::RecvError};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep_until};
use tracing::{info, warn};

use crate::bus::BusEvent;
use crate::hosts::{Hosts, client_for, host_repositories, ready_endpoint};
use crate::http::AppState;
use crate::http::convert::{repository_dto, task_dto_of};
use crate::scheduler::SchedEvent;
use crate::timeouts::Timeouts;

/// The far task workers of this daemon. Cheap to clone: every clone is the
/// same.
#[derive(Clone)]
pub struct FarTasks(Arc<Mutex<Option<JoinHandle<()>>>>);

impl FarTasks {
    /// Stop every worker: the daemon is stopping.
    pub async fn shutdown(&self) {
        let task = self.0.lock().unwrap().take();
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
    }
}

/// Start a worker for each host of `state`, and one for each host added
/// later, until [`FarTasks::shutdown`].
pub fn start(state: &AppState, timeouts: Timeouts) -> FarTasks {
    // Subscribed here, not in the task: a change made after `start` returns
    // is on the receiver, however late the task runs.
    let events = state.events.subscribe();
    let ctx = Ctx {
        store: state.store.clone(),
        events: state.events.clone(),
        sched: state.sched_tx.clone(),
        launcher: state.launcher.clone(),
        timeouts,
    };
    let hosts = state.hosts.clone();
    FarTasks(Arc::new(Mutex::new(Some(tokio::spawn(supervise(
        ctx, hosts, events,
    ))))))
}

/// What every worker shares.
#[derive(Clone)]
struct Ctx {
    store: Store,
    events: crate::bus::EventBus,
    sched: Option<mpsc::UnboundedSender<SchedEvent>>,
    /// Where a worker agent's post reaches the orchestrator session (039).
    launcher: Arc<crate::launcher::Launcher>,
    timeouts: Timeouts,
}

impl Ctx {
    fn wake(&self, task_id: &str) {
        if let Some(sched) = &self.sched {
            let _ = sched.send(SchedEvent::TaskChanged(task_id.into()));
        }
    }

    async fn unreachable(&self, host: &str, unreachable: bool) {
        if let Err(error) = self.store.set_host_unreachable(host, unreachable).await {
            warn!(host, %error, "cannot record whether the host of far tasks is reachable");
        }
    }
}

/// What a worker knows of its host: the endpoint while the host is `ready`,
/// and an epoch that each change of the endpoint moves. A worker that was
/// busy when the host left `ready` and came back at the same endpoint sees
/// the same endpoint, but a later epoch: it opens the stream again and reads
/// each far task again.
#[derive(Debug, Clone, Default)]
struct Link {
    epoch: u64,
    endpoint: Option<String>,
}

/// One host's worker: its [`Link`], and a nudge to look for far tasks that
/// are free to start.
struct Worker {
    ready: watch::Sender<Link>,
    kick: mpsc::Sender<()>,
    task: JoinHandle<()>,
}

impl Worker {
    fn kick(&self) {
        // One nudge waiting is enough: the worker reads every far task.
        let _ = self.kick.try_send(());
    }
}

async fn supervise(ctx: Ctx, hosts: Hosts, mut events: broadcast::Receiver<BusEvent>) {
    let mut workers: HashMap<String, Worker> = HashMap::new();
    for host in hosts.list() {
        see(&ctx, &mut workers, &host);
    }
    loop {
        match events.recv().await {
            Ok(event) => match event.event {
                DomainEvent::HostUpdated(host) => see(&ctx, &mut workers, &host),
                // The worker sees its endpoint go, marks its tasks and ends.
                DomainEvent::HostDeleted(host) => drop(workers.remove(&host.name)),
                DomainEvent::TaskCreated(_)
                | DomainEvent::TaskUpdated(_)
                | DomainEvent::GoalUpdated(_)
                | DomainEvent::GoalDeleted(_) => workers.values().for_each(Worker::kick),
                _ => {}
            },
            Err(RecvError::Lagged(_)) => {
                // A host may have left `ready` and come back among the lost
                // events: every worker follows its host again.
                for worker in workers.values() {
                    worker.ready.send_modify(|link| link.epoch += 1);
                }
                for host in hosts.list() {
                    see(&ctx, &mut workers, &host);
                }
                workers.values().for_each(Worker::kick);
            }
            Err(RecvError::Closed) => break,
        }
    }
    for (_, worker) in workers {
        worker.task.abort();
    }
}

/// Give a host's worker the host as it stands now, and start the worker of
/// a host seen for the first time.
fn see(ctx: &Ctx, workers: &mut HashMap<String, Worker>, host: &HostDto) {
    if host.name == LOCAL_HOST {
        return;
    }
    let endpoint = ready_endpoint(host);
    if let Some(worker) = workers.get(&host.name) {
        worker.ready.send_if_modified(|link| {
            if link.endpoint == endpoint {
                return false;
            }
            link.epoch += 1;
            link.endpoint = endpoint;
            true
        });
        return;
    }
    let (ready, ready_rx) = watch::channel(Link { epoch: 0, endpoint });
    let (kick, kicks) = mpsc::channel(1);
    let task = tokio::spawn(work(ctx.clone(), host.name.clone(), ready_rx, kicks));
    workers.insert(host.name.clone(), Worker { ready, kick, task });
}

/// Why a worker stopped following a host.
enum Lost {
    /// The host is gone, or the daemon stops.
    Closed,
    /// The host left `ready`, or changed its endpoint.
    Left,
    /// The host's event stream ended, or the host did not answer.
    Stream,
}

async fn work(
    ctx: Ctx,
    host: String,
    mut ready: watch::Receiver<Link>,
    mut kicks: mpsc::Receiver<()>,
) {
    loop {
        let Link { epoch, endpoint } = ready.borrow_and_update().clone();
        let Some(endpoint) = endpoint else {
            ctx.unreachable(&host, true).await;
            tokio::select! {
                changed = ready.changed() => if changed.is_err() { return },
                kick = kicks.recv() => if kick.is_none() { return },
            }
            continue;
        };
        let follower = Follower {
            ctx: &ctx,
            host: &host,
            client: client_for(&endpoint).with_request_timeout(ctx.timeouts.far_call),
            epoch,
            link: ready.clone(),
        };
        let lost = follower.follow(epoch, &mut ready, &mut kicks).await;
        ctx.unreachable(&host, true).await;
        match lost {
            Lost::Closed => return,
            Lost::Left => {}
            Lost::Stream => {
                // The host may still be `ready`: open its stream again after
                // a wait, unless it changes first.
                let again = Instant::now() + ctx.timeouts.far_retry;
                tokio::select! {
                    changed = ready.changed() => if changed.is_err() { return },
                    _ = sleep_until(again) => {}
                }
            }
        }
    }
}

/// Why a far task did not reach its host.
enum Unsent {
    /// The host refused it. The far task fails with the reason.
    Refused(String),
    /// The host did not answer. The far task is sent again later.
    Unanswered(String),
    /// The host left `ready` during the send. Nothing it answered is
    /// written, and the send starts again on its next `ready`.
    Left,
}

impl From<ClientError> for Unsent {
    fn from(error: ClientError) -> Self {
        match &error {
            ClientError::Api {
                status, message, ..
            } if status.is_client_error() => Self::Refused(message.clone()),
            _ => Self::Unanswered(error.to_string()),
        }
    }
}

impl From<ariadne_store::StoreError> for Unsent {
    fn from(error: ariadne_store::StoreError) -> Self {
        Self::Unanswered(format!("the local store: {error}"))
    }
}

/// A worker while its host is `ready`.
struct Follower<'a> {
    ctx: &'a Ctx,
    host: &'a str,
    client: Client,
    /// The epoch this follower follows, and the host's link as it is now.
    epoch: u64,
    link: watch::Receiver<Link>,
}

impl Follower<'_> {
    /// Whether the host is still in the `ready` this follower follows. An
    /// answer that arrives after the host left it writes nothing: the far
    /// task keeps its last known state while the host is away.
    fn current(&self) -> bool {
        self.link.borrow().epoch == self.epoch
    }

    /// Run one call to the host, unless the host leaves this `ready` first.
    /// `None` where it left: the call is dropped, and nothing it answers is
    /// written. So the follower sees the host leave while a call is under
    /// way, and marks its far tasks unreachable without waiting on it.
    async fn fenced<T>(&self, call: impl Future<Output = T>) -> Option<T> {
        let mut link = self.link.clone();
        let epoch = self.epoch;
        let answer = tokio::select! {
            answer = call => answer,
            _ = link.wait_for(|link| link.epoch != epoch) => return None,
        };
        self.current().then_some(answer)
    }

    async fn follow(
        &self,
        epoch: u64,
        ready: &mut watch::Receiver<Link>,
        kicks: &mut mpsc::Receiver<()>,
    ) -> Lost {
        // The stream opens before the far tasks are read again, so no change
        // of the host falls between the read and the stream.
        // Fenced as each call is: a host that leaves `ready` while the stream
        // opens does not hold back its far tasks' `host_unreachable`.
        // The stream names this daemon as the source, so the worker sends on
        // it the posts of its agents to the orchestrator here (039).
        let path = match self.ctx.store.daemon_id().await {
            Ok(daemon_id) => format!("/v1/events/stream?source={daemon_id}"),
            Err(error) => {
                warn!(host = self.host, %error, "cannot read the daemon ID of this daemon");
                return Lost::Stream;
            }
        };
        let mut stream = match self.fenced(self.client.stream(&path)).await {
            None => return Lost::Left,
            Some(Ok(stream)) => stream,
            Some(Err(error)) => {
                warn!(host = self.host, %error, "cannot open the event stream of the host");
                return Lost::Stream;
            }
        };
        if let Err(error) = self.refresh().await {
            warn!(host = self.host, %error, "cannot read the far tasks of the host");
            return Lost::Stream;
        }
        let mut retry = self.dispatch_all().await;
        loop {
            tokio::select! {
                changed = ready.changed() => {
                    if changed.is_err() {
                        return Lost::Closed;
                    }
                    if ready.borrow_and_update().epoch != epoch {
                        return Lost::Left;
                    }
                }
                kick = kicks.recv() => {
                    if kick.is_none() {
                        return Lost::Closed;
                    }
                    retry = self.dispatch_all().await;
                }
                _ = sleep_until(retry.unwrap_or_else(Instant::now)), if retry.is_some() => {
                    retry = self.dispatch_all().await;
                }
                frame = stream.next() => match frame {
                    Some(Ok(frame)) => {
                        if let Err(error) = self.on_frame(frame).await {
                            warn!(host = self.host, %error, "cannot read the far tasks of the host");
                            return Lost::Stream;
                        }
                    }
                    Some(Err(error)) => {
                        warn!(host = self.host, %error, "the event stream of the host broke");
                        return Lost::Stream;
                    }
                    None => return Lost::Stream,
                },
            }
        }
    }

    /// Read each far task that went to the host again, whatever its state,
    /// then clear `host_unreachable` on every far task of the host. A far
    /// task whose state is final here keeps it ([`ariadne_store::is_final`]).
    /// A host that left `ready` meanwhile stops the reads, and nothing is
    /// cleared.
    async fn refresh(&self) -> Result<(), ClientError> {
        let tasks = self
            .ctx
            .store
            .list_sent_far_tasks(self.host)
            .await
            .map_err(|e| ClientError::Unreachable {
                endpoint: "the local store".into(),
                source: Box::new(e),
            })?;
        for task in tasks {
            let Some(far_id) = &task.far_task_id else {
                continue;
            };
            let read = self
                .fenced(
                    self.client
                        .get_json::<TaskDto>(&format!("/v1/tasks/{far_id}")),
                )
                .await;
            let Some(read) = read else {
                return Ok(());
            };
            match read {
                Ok(far) => self.mirror(&task.id, &far).await,
                Err(ClientError::Api { status, .. }) if status == StatusCode::NOT_FOUND => {
                    warn!(host = self.host, task = %task.id, far_task = %far_id, "the host has no such task any more");
                }
                Err(error) => return Err(error),
            }
        }
        if self.source_references().await.is_err() {
            return Err(ClientError::Unreachable {
                endpoint: self.host.into(),
                source: Box::new(std::io::Error::other("source reference refresh failed")),
            });
        }
        if self.current() {
            self.ctx.unreachable(self.host, false).await;
        }
        Ok(())
    }

    async fn on_frame(&self, frame: SseEvent) -> Result<(), ClientError> {
        match frame.event.as_str() {
            "task_updated" => {
                let update: TaskUpdatedDto = serde_json::from_str(&frame.data)?;
                if let Some(reference) = self
                    .ctx
                    .store
                    .source_task_reference(&update.task.id)
                    .await
                    .map_err(local_store)?
                {
                    if reference.host == self.host {
                        let Some(task) = self
                            .fenced(
                                self.client
                                    .get_json::<TaskDto>(&format!("/v1/tasks/{}", reference.id)),
                            )
                            .await
                        else {
                            return Ok(());
                        };
                        self.mirror(&reference.id, &task?).await;
                    }
                    return Ok(());
                }
                match self
                    .ctx
                    .store
                    .far_task_by_far_id(self.host, &update.task.id)
                    .await
                {
                    Ok(Some(task)) => self.mirror(&task.id, &update.task).await,
                    Ok(None) => {}
                    Err(error) => warn!(host = self.host, %error, "cannot find a far task"),
                }
                Ok(())
            }
            "session_created" | "session_updated" => {
                let mut session: ariadne_api::sessions::SessionDto =
                    serde_json::from_str(&frame.data)?;
                if let Some(task_id) = &session.task_id
                    && let Some(reference) = self
                        .ctx
                        .store
                        .source_task_reference(task_id)
                        .await
                        .map_err(local_store)?
                    && reference.host == self.host
                {
                    let Some(task) = self
                        .fenced(
                            self.client
                                .get_json::<TaskDto>(&format!("/v1/tasks/{task_id}")),
                        )
                        .await
                    else {
                        return Ok(());
                    };
                    self.mirror(task_id, &task?).await;
                    if !self.current() {
                        return Ok(());
                    }
                    session.id = format!("far~{task_id}~{}", session.id);
                    session.switched_from = session
                        .switched_from
                        .map(|id| format!("far~{task_id}~{id}"));
                    let event = if frame.event == "session_created" {
                        DomainEvent::SessionCreated(session)
                    } else {
                        DomainEvent::SessionUpdated(session)
                    };
                    self.ctx.events.publish(BusEvent {
                        goal_id: Some(reference.goal_id),
                        task_id: Some(reference.id),
                        recorded: None,
                        event,
                    });
                }
                Ok(())
            }
            "task_branch_updated" => {
                let branch: ariadne_api::stream::TaskBranchDto = serde_json::from_str(&frame.data)?;
                if let Some(reference) = self
                    .ctx
                    .store
                    .source_task_reference(&branch.task_id)
                    .await
                    .map_err(local_store)?
                    && reference.host == self.host
                    && self.current()
                {
                    self.ctx.events.publish(BusEvent {
                        goal_id: Some(reference.goal_id),
                        task_id: Some(reference.id),
                        recorded: None,
                        event: DomainEvent::TaskBranchUpdated(branch),
                    });
                }
                Ok(())
            }
            "source_post" => self.on_post(serde_json::from_str(&frame.data)?).await,
            // The stream lost events: read every far task again.
            "resync" => self.refresh().await,
            _ => Ok(()),
        }
    }

    /// Write what the host reports of a far task, and wake the scheduler
    /// when its status moved: a task that depends on it may start now.
    async fn mirror(&self, task_id: &str, far: &TaskDto) {
        if let Ok(Some(reference)) = self.ctx.store.source_task_reference(task_id).await {
            if let Err(error) = self.cache_reference(&reference, far).await {
                let _ = error;
                self.ctx.unreachable(self.host, true).await;
            }
            return;
        }
        if !self.current() {
            return;
        }
        let state = FarTaskState {
            status: far.status,
            step: far.step.clone(),
            branch: far.branch.clone(),
            pr_url: far.pr_url.clone(),
            merge_commit: far.merge_commit.clone(),
            reason: far.reason.clone(),
        };
        match self.ctx.store.mirror_far_task(task_id, &state).await {
            Ok(true) => self.ctx.wake(task_id),
            Ok(false) => {}
            Err(error) => {
                warn!(host = self.host, task = task_id, %error, "cannot mirror a far task")
            }
        }
    }

    /// Send each far task that is free to start to the host. Answers when to
    /// try again, where the host did not answer.
    async fn dispatch_all(&self) -> Option<Instant> {
        // A host that left `ready` gets nothing until it is back.
        if !self.current() {
            return None;
        }
        let tasks = match self.ctx.store.list_far_tasks(self.host).await {
            Ok(tasks) => tasks,
            Err(error) => {
                warn!(host = self.host, %error, "cannot list the far tasks of the host");
                return Some(Instant::now() + self.ctx.timeouts.far_retry);
            }
        };
        let source_retry = self.source_references().await.is_err();
        let mut retry = if self.close_goals().await && !source_retry {
            None
        } else {
            Some(Instant::now() + self.ctx.timeouts.far_retry)
        };
        for task in tasks {
            match self.free(&task).await {
                Ok(true) => {}
                Ok(false) => continue,
                Err(error) => {
                    warn!(task = %task.id, %error, "cannot read the dependencies of a far task");
                    continue;
                }
            }
            match self.dispatch(&task).await {
                Ok(()) => info!(host = self.host, task = %task.id, "sent a far task to its host"),
                Err(Unsent::Refused(reason)) => self.refused(&task, &reason).await,
                Err(Unsent::Left) => return None,
                Err(Unsent::Unanswered(error)) => {
                    warn!(host = self.host, task = %task.id, %error, "the host did not take a far task");
                    retry = Some(Instant::now() + self.ctx.timeouts.far_retry);
                }
            }
        }
        retry
    }

    async fn close_goals(&self) -> bool {
        let goals: Vec<GoalDto> = match self.fenced(self.client.get_json("/v1/goals")).await {
            Some(Ok(goals)) => goals,
            _ => return false,
        };
        for far in goals {
            let Some(link) = &far.linked else { continue };
            let goal = match self.ctx.store.get_goal(&link.origin_goal_id).await {
                Ok(goal) => goal,
                Err(ariadne_store::StoreError::NotFound { .. }) => continue,
                Err(_) => return false,
            };
            let action = match goal.status() {
                GoalStatus::Completed => "complete",
                GoalStatus::Cancelled => "cancel",
                _ => continue,
            };
            if far.status.is_terminal() {
                continue;
            }
            let path = format!("/v1/goals/{}", far.id);
            let result: Option<Result<GoalDto, ClientError>> = self
                .fenced(
                    self.client
                        .post_json(&format!("{path}/{action}"), &serde_json::json!({})),
                )
                .await;
            if !matches!(result, Some(Ok(_))) {
                return false;
            }
        }
        true
    }

    /// Whether a far task is free to start: not yet sent, `pending` in an
    /// `active` goal, and every task it depends on merged, on any host.
    async fn free(&self, task: &Task) -> ariadne_store::Result<bool> {
        if task.far_task_id.is_some() || task.status() != TaskStatus::Pending {
            return Ok(false);
        }
        let goal = self.ctx.store.get_goal(&task.goal_id).await?;
        Ok(goal.status() == GoalStatus::Active
            && self.ctx.store.task_dependencies_merged(&task.id).await?)
    }

    async fn refused(&self, task: &Task, reason: &str) {
        if !self.current() {
            return;
        }
        let reason = format!("host {} refused the task: {reason}", self.host);
        warn!(task = %task.id, %reason, "a far task failed");
        match self
            .ctx
            .store
            .transition_task(
                &task.id,
                TaskStatus::Failed,
                Actor::Daemon,
                Some(&reason),
                None,
            )
            .await
        {
            Ok(_) => self.ctx.wake(&task.id),
            Err(error) => warn!(task = %task.id, %error, "cannot fail a far task"),
        }
    }

    /// Create the task on the host, in the linked goal of its goal there.
    async fn dispatch(&self, task: &Task) -> Result<(), Unsent> {
        let store = &self.ctx.store;
        let goal = store.get_goal(&task.goal_id).await?;
        let local = store
            .list_repositories()
            .await?
            .into_iter()
            .map(repository_dto)
            .collect();
        // What the host already made of this task, in whatever goal and
        // status: a send whose answer was lost, even where the linked goal
        // ended since, finds it and makes nothing again.
        let made: Vec<TaskDto> = self
            .fenced(
                self.client
                    .get_json(&format!("/v1/tasks?origin_task_id={}", task.id)),
            )
            .await
            .ok_or(Unsent::Left)??;
        if let Some(far) = made.into_iter().next() {
            return self.remember(task, &far).await;
        }
        let paired = self
            .fenced(host_repositories(local, &self.client))
            .await
            .ok_or(Unsent::Left)??;
        let Some(far_repo) = paired.get(&task.repo_id).cloned() else {
            let repo = store.get_repository(&task.repo_id).await?;
            return Err(Unsent::Refused(lacks_repository(self.host, &repo)));
        };
        let dto = task_dto_of(store, task.clone()).await?;
        let agents: Vec<AgentAssignment> = dto
            .agents
            .iter()
            .map(|agent| AgentAssignment {
                step: agent.step.clone(),
                skills: agent.skills.clone(),
                model: agent.model.clone(),
                effort: agent.effort.clone(),
                brief: agent.brief.clone(),
            })
            .collect();
        // Every send makes the same two requests, and both are idempotent on
        // the host: the goal for the origin goal, the task for the origin
        // task. A send whose answer was lost finds what the host made, and a
        // repository set up on the host since joins the goal.
        let goal_repos = store.list_goal_repositories(&goal.id).await?;
        let repository_ids = std::iter::once(far_repo.clone())
            .chain(
                goal_repos
                    .iter()
                    .filter(|repo| repo.id != task.repo_id)
                    .filter_map(|repo| paired.get(&repo.id).cloned()),
            )
            .collect();
        let model = agents
            .first()
            .map(|agent| agent.model.clone())
            .unwrap_or_else(|| goal.model.clone());
        let goal_request = CreateGoalRequest {
            linked: Some(LinkedGoalDto {
                origin_goal_id: goal.id.clone(),
            }),
            workflow: Some(goal.workflow.clone()),
            title: goal.title.clone(),
            description: goal.description.clone(),
            issue_url: goal.issue_url.clone(),
            repository_ids,
            model,
            effort: None,
        };
        let linked: GoalDto = self
            .fenced(self.client.post_json("/v1/goals", &goal_request))
            .await
            .ok_or(Unsent::Left)??;
        let task_request = CreateTaskRequest {
            title: task.title.clone(),
            description: task.description.clone(),
            repo_id: Some(far_repo),
            agents,
            depends_on: Vec::new(),
            host: None,
            origin_task_id: Some(task.id.clone()),
        };
        let far: TaskDto = self
            .fenced(
                self.client
                    .post_json(&format!("/v1/goals/{}/tasks", linked.id), &task_request),
            )
            .await
            .ok_or(Unsent::Left)??;
        // The goal of the task the host answers: a repeated create answers
        // the task it already holds, in the goal it is in.
        self.remember(task, &far).await
    }

    async fn remember(&self, task: &Task, far: &TaskDto) -> Result<(), Unsent> {
        if !self.current() {
            return Err(Unsent::Left);
        }
        self.ctx
            .store
            .set_far_task(&task.id, &far.goal_id, &far.id)
            .await?;
        self.mirror(&task.id, far).await;
        Ok(())
    }
}

/// The refusal for a repository that the host has no checkout of. It names
/// the repository, the host, and the setup step that adds it (037, rule 63).
pub(crate) fn lacks_repository(host: &str, repo: &Repository) -> String {
    let name = repository_name(repo);
    format!(
        "host {host} lacks repository {name} ({id}); add it with the `repository:{id}` step of \
         the host's setup: `ariadne host setup {host}` with `{id}` in `repositories`",
        id = repo.id,
    )
}

/// A repository as a user knows it: its forge name, else its folder.
fn repository_name(repo: &Repository) -> String {
    match &repo.forge {
        Some(forge) => format!("{}/{}", forge.owner, forge.name),
        None => std::path::Path::new(&repo.path)
            .file_name()
            .map_or_else(|| repo.path.clone(), |name| name.to_string_lossy().into()),
    }
}

impl Follower<'_> {
    /// Reconcile source references, including reservations whose create response was lost.
    async fn source_references(&self) -> Result<(), Unsent> {
        let store = &self.ctx.store;
        let references = store.source_task_references(self.host).await?;
        if references.is_empty() {
            return Ok(());
        }
        let protocol: ariadne_api::tasks::RemoteTaskProtocol = self
            .fenced(self.client.get_json("/v1/remote-tasks/protocol"))
            .await
            .ok_or(Unsent::Left)??;
        for reference in references {
            if reference.worker_id != protocol.daemon_id
                || protocol.protocol != "remote-tasks-v1"
                || !protocol
                    .capabilities
                    .iter()
                    .any(|c| c == ariadne_api::tasks::SOURCE_DEPENDENCY_CONTROLS)
            {
                store.source_tasks_unreachable(self.host, true).await?;
                return Err(Unsent::Refused(
                    "worker identity changed; restore the original worker or upgrade Ariadne"
                        .into(),
                ));
            }
            let mut far: TaskDto = if let Some(request) = &reference.request {
                let request: serde_json::Value =
                    serde_json::from_str(request).map_err(|e| Unsent::Refused(e.to_string()))?;
                self.fenced(
                    self.client
                        .put_json(&format!("/v1/remote-tasks/{}", reference.id), &request),
                )
                .await
                .ok_or(Unsent::Left)??
            } else {
                self.fenced(self.client.get_json(&format!("/v1/tasks/{}", reference.id)))
                    .await
                    .ok_or(Unsent::Left)??
            };
            self.cache_reference(&reference, &far).await?;
            let Some(current_reference) = store.source_task_reference(&reference.id).await? else {
                continue;
            };
            let goal = store.get_goal(&reference.goal_id).await;
            let cancel = goal.is_err()
                || current_reference.cancel_requested != 0
                || goal
                    .as_ref()
                    .is_ok_and(|g| g.status() == GoalStatus::Cancelled);
            if cancel && !far.status.is_terminal() {
                // Completion can win this race. Read the authoritative outcome after a conflict.
                match self
                    .fenced(
                        self.client
                            .post_empty::<TaskDto>(&format!("/v1/tasks/{}/cancel", reference.id)),
                    )
                    .await
                    .ok_or(Unsent::Left)?
                {
                    Ok(task) => far = task,
                    Err(ClientError::Api { status, .. }) if status == StatusCode::CONFLICT => {
                        far = self
                            .fenced(self.client.get_json(&format!("/v1/tasks/{}", reference.id)))
                            .await
                            .ok_or(Unsent::Left)??;
                    }
                    Err(error) => return Err(error.into()),
                }
            } else if !cancel
                && let (Some(reason), Some(operation), Some(revision)) = (
                    &current_reference.dependency_failure,
                    &current_reference.failure_operation_id,
                    current_reference.failure_revision,
                )
            {
                let request = ariadne_api::tasks::FailRemoteDependencyRequest {
                    revision,
                    operation_id: operation.clone(),
                    reason: reason.clone(),
                };
                match self
                    .fenced(self.client.post_json(
                        &format!("/v1/remote-tasks/{}/dependency-failure", reference.id),
                        &request,
                    ))
                    .await
                    .ok_or(Unsent::Left)?
                {
                    Ok(task) => far = task,
                    Err(ClientError::Api { status, .. }) if status == StatusCode::CONFLICT => {
                        store
                            .clear_source_dependency_request(&reference.id, operation)
                            .await?;
                        continue;
                    }
                    Err(error) => return Err(error.into()),
                }
            } else if !cancel
                && current_reference.retry_requested != 0
                && far.status == TaskStatus::Failed
                && store.task_dependencies_merged(&reference.id).await?
            {
                far = if !far.execution_context.as_ref().is_some_and(|c| c.authorized)
                    && let Some(operation) = &current_reference.failure_operation_id
                {
                    let request = ariadne_api::tasks::RetryRemoteDependencyRequest {
                        revision: far.execution_context.as_ref().unwrap().revision,
                        operation_id: operation.clone(),
                    };
                    self.fenced(self.client.post_json(
                        &format!("/v1/remote-tasks/{}/retry-dependency", reference.id),
                        &request,
                    ))
                    .await
                    .ok_or(Unsent::Left)??
                } else {
                    self.fenced(
                        self.client
                            .post_empty(&format!("/v1/tasks/{}/retry", reference.id)),
                    )
                    .await
                    .ok_or(Unsent::Left)??
                };
            } else if !cancel
                && goal
                    .as_ref()
                    .is_ok_and(|g| g.status() == GoalStatus::Active)
                && current_reference.dependency_failure.is_none()
                && !far.execution_context.as_ref().is_some_and(|c| c.authorized)
                && far.status == TaskStatus::Pending
                && store.task_dependencies_merged(&reference.id).await?
            {
                let revision = far
                    .execution_context
                    .as_ref()
                    .ok_or_else(|| {
                        Unsent::Refused("worker omitted execution context; upgrade Ariadne".into())
                    })?
                    .revision;
                if !store.source_authorization(&reference.id, revision).await? {
                    continue;
                }
                far = self
                    .fenced(self.client.post_json(
                        &format!("/v1/remote-tasks/{}/authorize", reference.id),
                        &ariadne_api::tasks::AuthorizeRemoteTaskRequest { revision },
                    ))
                    .await
                    .ok_or(Unsent::Left)??;
            }
            self.cache_reference(&reference, &far).await?;
            if goal.is_err() && far.status.is_terminal() {
                store.remove_source_task_reference(&reference.id).await?;
            }
        }
        Ok(())
    }

    async fn cache_reference(
        &self,
        reference: &ariadne_store::SourceTaskReference,
        far: &TaskDto,
    ) -> Result<(), Unsent> {
        if !self.current() {
            return Err(Unsent::Left);
        }
        self.ctx
            .store
            .cache_source_task(
                &reference.id,
                &reference.worker_id,
                &serde_json::to_value(far).map_err(|e| Unsent::Refused(e.to_string()))?,
            )
            .await?;
        self.ctx.wake(&reference.id);
        if let Some(sched) = &self.ctx.sched {
            let _ = sched.send(SchedEvent::GoalChanged(reference.goal_id.clone()));
        }
        Ok(())
    }
}

fn local_store(error: ariadne_store::StoreError) -> ClientError {
    ClientError::Unreachable {
        endpoint: "the local store".into(),
        source: Box::new(error),
    }
}
