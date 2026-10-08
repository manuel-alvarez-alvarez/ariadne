//! What a pull request wants of the scheduler.
//!
//! A request of mine is the request a task opened, and the author of that
//! task keeps it until a human merges or closes it (005): the news of the
//! request is handed to that author's session, once. It gets no session of
//! its own. The task's own pass starts, resumes and watches the author; an
//! idle author on an open request waits on the forge (009 rule 41).
//!
//! A request that asks for my review wants a reviewer session on the
//! repository's `review_model` and `review_effort` (029): one detached at the
//! request's head, told of each push and of the replies in its own threads,
//! and taken down once the request is merged, closed, or no longer asks.

use std::time::{Duration, Instant};

use tracing::{info, warn};

use ariadne_core::{Actor, Seat, SessionStatus, TaskStatus};
use ariadne_store::{
    AgentPin, AgentSession, PullRequest, PullRequestFilter, SessionFilter, StoreError, Task,
};

use crate::acp::NewsDelivery;
use crate::agents::prompts;
use crate::forge::{news, poll::wants_session};

use super::{QUIET_NUDGE_SECS, SPAWN_RETRY_BUDGET};

/// How long the tick waits before it tries a failed cleanup again. A change
/// of the request tries at once.
const CLEANUP_RETRY: Duration = Duration::from_secs(60);

/// The situation a review session's watchdog spends its steps in: there is
/// one, the request, for as long as it is open.
const SITUATION: &str = "pull_request";

impl super::Scheduler {
    /// Every request that could want something done: the open ones, and the
    /// ended ones whose session or worktree is still there.
    pub(super) async fn reconcile_pull_requests(&mut self) {
        let rows = match self
            .store
            .list_pull_requests(PullRequestFilter::default())
            .await
        {
            Ok(rows) => rows,
            Err(e) => {
                warn!(error = %e, "reconcile: listing pull requests failed");
                return;
            }
        };
        for row in rows {
            if self
                .pull_request_cleanup_retry
                .get(&row.id)
                .is_some_and(|at| *at > Instant::now())
            {
                continue;
            }
            // An ended request is passed over until its work is taken down,
            // which the row records, so a restart owes it the same.
            let leftover = row.state != "open"
                && (row.cleaned_at.is_none()
                    || self.launcher.pull_request_worktree_exists(&row)
                    || self
                        .live_pull_request_sessions(&row.id)
                        .await
                        .is_ok_and(|live| !live.is_empty()));
            if row.state == "open" || leftover {
                self.reconcile_pull_request(&row.id).await;
            }
        }
    }

    /// One request reconciled, with nowhere to hand an error but the log.
    pub(super) async fn reconcile_pull_request(&mut self, id: &str) {
        if let Err(e) = self.pull_request_pass(id).await {
            warn!(pull_request = %id, error = %format!("{e:#}"), "pull request reconciliation failed");
        }
    }

    async fn pull_request_pass(&mut self, id: &str) -> anyhow::Result<()> {
        let pull = match self.store.get_pull_request(id).await {
            Ok(pull) => pull,
            Err(StoreError::NotFound { .. }) => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        // A request the user reviews has a lifecycle of its own (029).
        if pull.role != "author" {
            return self.review_pass(&pull).await;
        }
        // A request of mine the user asked Ariadne to review has a review
        // session beside its author's (029), taken down the same way.
        if pull.review_asked
            || self.launcher.pull_request_worktree_exists(&pull)
            || !self.live_pull_request_sessions(&pull.id).await?.is_empty()
        {
            self.review_pass(&pull).await?;
        }
        self.keep_pass(&pull).await
    }

    /// A request of mine: its news handed to the author of the task that
    /// opened it, and what is left of it taken down once it ended and its
    /// task is over (005). One opened by hand has nobody to keep it.
    async fn keep_pass(&mut self, pull: &PullRequest) -> anyhow::Result<()> {
        let task = match &pull.origin_task_id {
            Some(task_id) => match self.store.get_task(task_id).await {
                Ok(task) => Some(task),
                Err(StoreError::NotFound { .. }) => None,
                Err(e) => return Err(e.into()),
            },
            None => None,
        };
        // The task is over: nobody keeps the request any more, and what is
        // left of it is taken down once a human ended it.
        let Some(task) = task.filter(|task| !ends_its_work(task)) else {
            if pull.state != "open" {
                return self.end_kept_request(pull).await;
            }
            return Ok(());
        };
        let Some(integration) = self
            .store
            .forge_integration(&pull.repository_id)
            .await?
            .filter(|i| i.enabled)
        else {
            return Ok(());
        };
        let author = self
            .keeper_of(&task)
            .await?
            .filter(|author| self.launcher.acp.is_running(&author.id));
        if pull.state == "merged" && self.merge_is_read(pull, author.as_ref()) {
            // Boxed: the fetch it waits on would otherwise sit in every
            // scheduler future that reaches a request's pass.
            return Box::pin(self.finish_merged(&task, pull)).await;
        }
        let Some(author) = author else {
            // The task's own pass starts its author again, and the news
            // waits for it.
            return Ok(());
        };
        let login = integration.login.clone().unwrap_or_default();
        self.tell_pull_request(&author, pull, &login).await?;
        Ok(())
    }

    /// Whether an approved task's request merged and its pass ended the
    /// task: the author's turn that read the merge is the task's event, and
    /// the request's pass is what finishes it (005).
    pub(super) async fn merge_ended(&mut self, task: &Task) -> anyhow::Result<bool> {
        let Some(pull) = self.store.pull_request_of_task(&task.id).await? else {
            return Ok(false);
        };
        if pull.state != "merged" {
            return Ok(false);
        }
        self.reconcile_pull_request(&pull.id).await;
        Ok(self.store.get_task(&task.id).await?.status() != TaskStatus::Approved)
    }

    /// Whether the author of a task whose request merged is done with it:
    /// it was told the merge and the turn that read it has ended, or it has
    /// not answered in [`QUIET_NUDGE_SECS`] since, or no author is up to tell
    /// at all (005).
    fn merge_is_read(&self, pull: &PullRequest, author: Option<&AgentSession>) -> bool {
        let Some(author) = author else {
            return true;
        };
        if pull.told_state.as_deref() != Some("merged") {
            return false;
        }
        let stamp = |at: &Option<String>| {
            at.as_deref()
                .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                .map(|at| at.with_timezone(&chrono::Utc))
        };
        let Some(told) = stamp(&pull.news_told_at) else {
            return false;
        };
        let answered = author.status() == SessionStatus::Idle
            && stamp(&author.last_activity_at).is_some_and(|heard| heard > told);
        let waited = (chrono::Utc::now() - told).num_seconds() >= QUIET_NUDGE_SECS;
        answered || waited
    }

    /// End a task whose request a human merged (005): the author's own
    /// finish is no longer waited on, and the task's cleanup takes its
    /// sessions and its worktree down, so no agent stays up on a request
    /// that is done.
    async fn finish_merged(&mut self, task: &Task, pull: &PullRequest) -> anyhow::Result<()> {
        info!(task = %task.id, pull_request = %pull.id, "the request merged, finishing its task");
        let merge_commit = self.merged_base_tip(pull).await;
        match self
            .store
            .transition_task(
                &task.id,
                TaskStatus::Finished,
                Actor::Daemon,
                Some(&format!("{} merged", pull.url)),
                Some(&merge_commit),
            )
            .await
        {
            Ok(_) => Ok(()),
            // The author finished it in the meantime.
            Err(StoreError::Transition(_)) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// What a merged request's task is finished by: the tip of the base it
    /// merged into, read off the remote the way the author's own `finish_task`
    /// reads it, or the checkout's own base, or at the last its head (005).
    async fn merged_base_tip(&self, pull: &PullRequest) -> String {
        let Ok(repo) = self.store.get_repository(&pull.repository_id).await else {
            return pull.head_sha.clone();
        };
        let path = std::path::PathBuf::from(&repo.path);
        let remote = repo
            .forge
            .as_ref()
            .map_or_else(|| "origin".to_string(), |forge| forge.remote.clone());
        let git = &self.launcher.git;
        match git.fetched_tip(&path, &remote, &pull.base_branch).await {
            Ok(sha) => sha,
            Err(e) => {
                warn!(pull_request = %pull.id, error = %format!("{e:#}"), "the merged base could not be fetched");
                git.branch_tip(&path, &pull.base_branch)
                    .await
                    .unwrap_or_else(|_| pull.head_sha.clone())
            }
        }
    }

    /// The live session of the author that keeps a task's request: the
    /// picked winner's on a task with several authors, the lone author's
    /// everywhere else.
    async fn keeper_of(&self, task: &Task) -> anyhow::Result<Option<AgentSession>> {
        let live = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                live_only: true,
                ..Default::default()
            })
            .await?;
        Ok(live.into_iter().find(|session| {
            session.seat() == Some(Seat::Author)
                && task
                    .picked_agent_id
                    .as_ref()
                    .is_none_or(|winner| session.task_agent_id.as_deref() == Some(winner.as_str()))
        }))
    }

    /// Start a review session, or put the one it had back on its feet,
    /// within the spawn-retry budget.
    async fn start_pull_request_session(
        &mut self,
        pull: &PullRequest,
        pin: AgentPin,
    ) -> anyhow::Result<()> {
        let last = self
            .store
            .list_sessions(SessionFilter {
                pull_request_id: Some(pull.id.clone()),
                ..Default::default()
            })
            .await?
            .pop();
        if let Some(last) = &last
            && self.spent_on_a_dead_launch(&pull.id, &pull.id, last)
        {
            *self.spawn_failures.entry(pull.id.clone()).or_default() += 1;
        }
        if self.spawn_failures.get(&pull.id).copied().unwrap_or(0) >= SPAWN_RETRY_BUDGET {
            return Ok(());
        }
        match self.launcher.resume_pull_request_session(pull, pin).await {
            Ok(session) => {
                info!(pull_request = %pull.id, session = %session.id, "the request's session is up");
                Ok(())
            }
            Err(e) => {
                *self.spawn_failures.entry(pull.id.clone()).or_default() += 1;
                Err(e)
            }
        }
    }

    /// Hand the session the news it has not been told, once it has its
    /// briefing. The ACP driver marks it told right before the prompt goes
    /// out, and gives the mark back where it never did (018); news of the
    /// request still queued is not queued again. Where nothing is news but a
    /// check or the base recovered, the mark is written at once, so its next
    /// turn is news again.
    async fn tell_pull_request(
        &mut self,
        session: &AgentSession,
        pull: &PullRequest,
        login: &str,
    ) -> anyhow::Result<bool> {
        if !matches!(
            session.status(),
            SessionStatus::Running | SessionStatus::Idle
        ) {
            return Ok(false);
        }
        let news = news::untold(&self.store, pull, login, session.seat()).await?;
        if let Some(recovered) = news.recovered() {
            self.store
                .set_pull_request_told(&pull.id, recovered)
                .await?;
        }
        if news.is_empty() {
            return Ok(false);
        }
        let text = prompts::pull_request_news(
            ariadne_store::defaults::pull_request_news_prompt(),
            pull,
            &news.lines,
        );
        let delivery = NewsDelivery {
            pull_request_id: pull.id.clone(),
            comment_ids: news.comment_ids,
            told: news.told,
            before: news.before,
        };
        let handed = self.launcher.acp.send_news(&session.id, delivery, text);
        Ok(self.handed(session, handed))
    }

    /// A request a task opened that a human merged or closed, once its task
    /// is over: whatever an earlier release left of a session of its own
    /// taken down, and, where it merged a goal branch onto its base, that
    /// branch deleted (`Launcher::cleanup_kept_request`). The task's own
    /// cleanup took its worktree and its branch.
    async fn end_kept_request(&mut self, pull: &PullRequest) -> anyhow::Result<()> {
        if pull.cleaned_at.is_some()
            && !self.launcher.pull_request_worktree_exists(pull)
            && self.live_pull_request_sessions(&pull.id).await?.is_empty()
        {
            return Ok(());
        }
        let enabled = self
            .store
            .forge_integration(&pull.repository_id)
            .await?
            .is_some_and(|i| i.enabled);
        info!(pull_request = %pull.id, state = %pull.state, "the request ended, taking its work down");
        // Done only once every branch it owes is gone, which the row records,
        // so a restart owes it the same. A deletion that failed is tried
        // again, on the next change of the request at once and on the tick
        // after a wait. Where the integration was disabled no branch is
        // touched.
        let cleaned = match enabled {
            true => self.launcher.cleanup_kept_request(pull).await,
            false => self.launcher.end_pull_request_sessions(pull).await,
        };
        match cleaned {
            Ok(()) => {
                self.pull_request_cleanup_retry.remove(&pull.id);
                self.store.mark_pull_request_cleaned(&pull.id).await?;
                Ok(())
            }
            Err(e) => {
                self.pull_request_cleanup_retry
                    .insert(pull.id.clone(), Instant::now() + CLEANUP_RETRY);
                Err(e)
            }
        }
    }

    /// A request that asks for my review (029): one live reviewer session
    /// while it is open, out of draft and still asks, in a repository with
    /// a `review_model`; its sessions and worktree taken down otherwise.
    async fn review_pass(&mut self, pull: &PullRequest) -> anyhow::Result<()> {
        let integration = self.store.forge_integration(&pull.repository_id).await?;
        let live = self.live_pull_request_sessions(&pull.id).await?;
        let login = integration
            .as_ref()
            .and_then(|i| i.login.clone())
            .unwrap_or_default();
        let Some(integration) = integration.filter(|i| wants_session(pull, i)) else {
            return self.end_review(pull, &live).await;
        };
        let running = live
            .iter()
            .find(|s| s.seat() == Some(Seat::Reviewer) && self.launcher.acp.is_running(&s.id))
            .cloned();
        let Some(session) = running else {
            // A request of mine is reviewed on the pin the user picked when
            // asking (029); any other on the repository's review pin.
            let pin = match pull.role.as_str() {
                "author" => AgentPin {
                    model: pull.review_model.clone().unwrap_or_default(),
                    effort: pull.review_effort.clone(),
                },
                _ => AgentPin {
                    model: integration.review_model.clone().unwrap_or_default(),
                    effort: integration.review_effort.clone(),
                },
            };
            // The briefing names the head the worktree is put at, so only a
            // later push is news.
            self.store
                .set_pull_request_told_head(&pull.id, &pull.head_sha)
                .await?;
            return self.start_pull_request_session(pull, pin).await;
        };
        // A push moves the worktree under the agent, so it waits for the
        // turn on the old head to end.
        let pushed = pull
            .told_head_sha
            .as_deref()
            .is_some_and(|told| told != pull.head_sha);
        let idle = session.status() == SessionStatus::Idle;
        if pushed && idle {
            let repo = self.store.get_repository(&pull.repository_id).await?;
            self.launcher.review_worktree(pull, &repo).await?;
        }
        if !pushed || idle {
            self.tell_pull_request(&session, pull, &login).await?;
        }
        if session.status() == SessionStatus::Running {
            let repo = self.store.get_repository(&pull.repository_id).await?;
            let worktree = session.worktree_path.clone().unwrap_or_default();
            let briefing = self.launcher.pull_request_briefing_of(
                pull,
                &repo,
                std::path::Path::new(&worktree),
            );
            self.check_session_quiet(&session, SITUATION.to_string(), &briefing)
                .await?;
        }
        Ok(())
    }

    /// A request I review that wants no session: merged, closed, back in
    /// draft, or no longer asking for my review (029). Its sessions are
    /// killed and its worktree removed; no branch is touched, since none is
    /// mine. An ended request records the cleanup, as an author's does.
    async fn end_review(
        &mut self,
        pull: &PullRequest,
        live: &[AgentSession],
    ) -> anyhow::Result<()> {
        // An ended request of mine owes the cleanup its author's side
        // records (`end_kept_request`); here only the review is taken down.
        let owed = pull.state != "open" && pull.cleaned_at.is_none() && pull.role != "author";
        if live.is_empty() && !self.launcher.pull_request_worktree_exists(pull) && !owed {
            return Ok(());
        }
        info!(pull_request = %pull.id, state = %pull.state, "the request wants no review, taking its work down");
        match self.launcher.end_pull_request_sessions(pull).await {
            Ok(()) => {
                self.pull_request_cleanup_retry.remove(&pull.id);
                if pull.state != "open" && pull.role != "author" {
                    self.store.mark_pull_request_cleaned(&pull.id).await?;
                }
                Ok(())
            }
            Err(e) => {
                self.pull_request_cleanup_retry
                    .insert(pull.id.clone(), Instant::now() + CLEANUP_RETRY);
                Err(e)
            }
        }
    }

    async fn live_pull_request_sessions(&self, id: &str) -> anyhow::Result<Vec<AgentSession>> {
        Ok(self
            .store
            .list_sessions(SessionFilter {
                pull_request_id: Some(id.to_string()),
                live_only: true,
                ..Default::default()
            })
            .await?)
    }
}

/// Whether a task is done with its request: finished, cancelled, or failed.
/// A failed task is retried by a person, whose retry opens another request.
fn ends_its_work(task: &Task) -> bool {
    task.status().is_terminal() || task.status() == TaskStatus::Failed
}
