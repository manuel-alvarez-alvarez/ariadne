//! What a pull request wants of the scheduler.
//!
//! A request of mine is the request a task opened, and the agent of that
//! task's `pr` column keeps it until a human merges or closes it (030): the
//! news of the request is handed to that agent's session, once. It gets no
//! session of its own. The task's own pass starts, resumes and watches the
//! agent; an idle agent on an open request waits on the forge (009).
//!
//! A request that asks for my review wants a reviewer session on the pin
//! asked directly on its own row (029): one detached at the request's
//! head, told of each push and of the replies in its own threads, and
//! taken down once the request is merged, closed, or no longer asks.

use std::time::{Duration, Instant};

use anyhow::Context;
use tracing::{info, warn};

use ariadne_core::workflow::StepGate;
use ariadne_core::{Actor, Seat, SessionStatus, TaskStatus};
use ariadne_store::{
    AgentPin, AgentSession, PullRequest, PullRequestAsks, PullRequestFilter, SessionFilter,
    StoreError, Task,
};

use crate::acp::NewsDelivery;
use crate::agents::prompts;
use crate::forge::{
    news,
    poll::{wants_babysit_session, wants_session},
};

use super::{QUIET_NUDGE_SECS, SPAWN_RETRY_BUDGET};

/// How long the tick waits before it retries a failed cleanup or reads an
/// open kept request's forge state again. A cleanup wait skips its full pass;
/// a read wait skips only that read. A request change reads at once.
const CLEANUP_RETRY: Duration = Duration::from_secs(60);

/// The situation a review session's watchdog spends its steps in: there is
/// one, the request, for as long as it is open.
const SITUATION: &str = "pull_request";

impl super::Scheduler {
    /// Reconcile every request from the periodic tick at `now`.
    pub(super) async fn reconcile_pull_requests_at(&mut self, now: Instant) {
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
                .is_some_and(|at| *at > now)
            {
                continue;
            }
            self.reconcile_pull_request_on_tick(&row.id, now).await;
        }
    }

    /// One request reconciled, with nowhere to hand an error but the log.
    pub(super) async fn reconcile_pull_request(&mut self, id: &str) {
        self.reconcile_pull_request_at(id, None).await;
    }

    /// One request reconciled from the periodic tick at `now`.
    async fn reconcile_pull_request_on_tick(&mut self, id: &str, now: Instant) {
        self.reconcile_pull_request_at(id, Some(now)).await;
    }

    /// One request reconciled, with nowhere to hand an error but the log.
    async fn reconcile_pull_request_at(&mut self, id: &str, tick_now: Option<Instant>) {
        if let Err(e) = self.pull_request_pass(id, tick_now).await {
            warn!(pull_request = %id, error = %format!("{e:#}"), "pull request reconciliation failed");
        }
    }

    async fn pull_request_pass(
        &mut self,
        id: &str,
        tick_now: Option<Instant>,
    ) -> anyhow::Result<()> {
        let row = match self.store.get_pull_request(id).await {
            Ok(row) => row,
            Err(StoreError::NotFound { .. }) => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        let enabled = self
            .store
            .forge_integration(&row.repository_id)
            .await?
            .is_some_and(|i| i.enabled);
        // With the integration off nothing reads the forge for the request:
        // its review ends, and Ariadne stops working on it, but for a task's
        // request, whose agent keeps it once the integration is on again.
        if !enabled {
            self.launcher
                .end_pull_request_sessions(&row.id, &row.repository_id, None)
                .await?;
            if row.origin_task_id.is_none() {
                self.stop_working(&row.id, (&row).into()).await?;
            }
            return Ok(());
        }
        // Read off the forge by the fetch, which reads every request Ariadne
        // works on at once: until it has, there is nothing to act on.
        let Some(pull) = crate::forge::live::of_row(&self.store, &self.launcher.live, row).await?
        else {
            return Ok(());
        };
        // A request the user reviews has a lifecycle of its own (029).
        if pull.role != "author" {
            return self.review_pass(&pull).await;
        }
        // A request of mine the user asked Ariadne to review has a review
        // session beside its keeper's (029), taken down the same way.
        if pull.review_asked
            || self.launcher.pull_request_worktree_exists(&pull.id)
            || !self.live_reviewer_sessions(&pull.id).await?.is_empty()
        {
            self.review_pass(&pull).await?;
        }
        // A request of mine asked for by hand has a keeper session of its
        // own beside a task's, or on no task at all (032).
        if pull.babysit_asked
            || self.launcher.keeper_worktree_exists(&pull.id)
            || !self.live_keeper_sessions(&pull.id).await?.is_empty()
        {
            self.babysit_pass(&pull).await?;
        }
        if pull.origin_task_id.is_none() && !pull.review_asked && !pull.babysit_asked {
            // Nobody keeps it, nobody reviews it and nobody babysits it:
            // nothing to work on.
            return self.stop_working(&pull.id, (&pull).into()).await;
        }
        self.keep_pass(&pull, tick_now).await
    }

    /// The request `pull` as the forge reads it now, where that is merged
    /// or closed: the last read held for it said open. None while the forge
    /// confirms it open, or where the integration is off; a read that fails
    /// is the error, never a confirmation.
    async fn read_ended(&mut self, pull: &PullRequest) -> anyhow::Result<Option<PullRequest>> {
        let Some(integration) = self
            .store
            .forge_integration(&pull.repository_id)
            .await?
            .filter(|i| i.enabled)
        else {
            return Ok(None);
        };
        let slug = format!(
            "{}/{}/{}",
            integration.host, integration.owner, integration.name
        );
        let read = crate::forge::ForgeClient::for_repository(&self.launcher.cfg, &integration)
            .pull_request(&slug, pull.number)
            .await
            .map_err(|error| anyhow::anyhow!("cannot read whether the request ended: {error}"))?;
        if read.number != pull.number {
            anyhow::bail!("the forge returned another request number");
        }
        if read.state == "open" {
            return Ok(None);
        }
        self.launcher
            .live
            .set_pull(&pull.id, read, pull.review_requested);
        let row = self.store.get_pull_request(&pull.id).await?;
        Ok(crate::forge::live::of_row(&self.store, &self.launcher.live, row).await?)
    }

    /// Stop working on a request (026): its row goes, and what was read of
    /// it is forgotten. Its sessions stay, let go of it.
    ///
    /// The row goes only as `asked` — who wanted the request when this pass
    /// read it — still holds. A pass decides from a view read several store
    /// calls before this, and an ask by hand that lands in between (029,
    /// 032) is one the view never saw: deleted regardless, the row would
    /// take the ask with it, and the next fetch would read the request back
    /// with nothing asked of it. Refused, everything stays, the read
    /// included: the pass that ask itself woke reads the row anew and acts
    /// on it.
    async fn stop_working(&mut self, id: &str, asked: PullRequestAsks) -> anyhow::Result<()> {
        if self
            .store
            .delete_pull_request_unless_asked_since(id, &asked)
            .await?
            .is_some()
        {
            info!(pull_request = %id, "Ariadne stopped working on the request");
        } else {
            match self.store.get_pull_request(id).await {
                Ok(_) => {
                    info!(pull_request = %id, "the request was asked for since this pass read it, keeping it");
                    return Ok(());
                }
                Err(StoreError::NotFound { .. }) => {}
                Err(e) => return Err(e.into()),
            }
        }
        self.launcher.live.remove(id);
        self.pull_request_cleanup_retry.remove(id);
        self.pull_request_read_retry.remove(id);
        self.keeper_skills_applied.remove(id);
        Ok(())
    }

    /// A request of mine: its news handed to the agent of the `pr` column
    /// of the task that opened it, and what is left of it taken down once it
    /// ended and its task is over (030). One opened by hand has nobody to
    /// keep it.
    async fn keep_pass(
        &mut self,
        pull: &PullRequest,
        tick_now: Option<Instant>,
    ) -> anyhow::Result<()> {
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
            // The last read may be older than the merge that ended the task:
            // a finish reads the forge at the call. The forge is asked again,
            // so a merged request still has its work taken down.
            // Open or failed, the read proves nothing moved: the row stays.
            // A tick skips only this read until the retry wait ends, so the
            // review and keeper passes still recover each tick. A request
            // change reads at once, and an open request with no task costs
            // one forge read per wait, not one per tick.
            if tick_now.is_some_and(|now| {
                self.pull_request_read_retry
                    .get(&pull.id)
                    .is_some_and(|at| *at > now)
            }) {
                return Ok(());
            }
            let retry_at = tick_now.unwrap_or_else(Instant::now) + CLEANUP_RETRY;
            match Box::pin(self.read_ended(pull)).await {
                Ok(Some(ended)) => return Box::pin(self.end_kept_request(&ended)).await,
                Ok(None) => {
                    self.pull_request_read_retry
                        .insert(pull.id.clone(), retry_at);
                }
                Err(e) => {
                    self.pull_request_read_retry
                        .insert(pull.id.clone(), retry_at);
                    return Err(e);
                }
            }
            // Open with nobody to keep it: Ariadne stops working on it, but
            // for the review or the keeper session the user asked of it,
            // which run on (029, 032).
            if !pull.review_asked && !pull.babysit_asked {
                return self.stop_working(&pull.id, pull.into()).await;
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
        let keeper = self
            .keeper_of(&task)
            .await?
            .filter(|keeper| self.launcher.acp.is_running(&keeper.id));
        if pull.state == "merged" && self.merge_is_read(pull, keeper.as_ref()) {
            // Boxed: the fetch it waits on would otherwise sit in every
            // scheduler future that reaches a request's pass.
            return Box::pin(self.finish_merged(&task, pull)).await;
        }
        let Some(keeper) = keeper else {
            // The task's own pass starts its agent again, and the news
            // waits for it.
            return Ok(());
        };
        let login = integration.login.clone().unwrap_or_default();
        self.tell_pull_request(&keeper, pull, &login).await?;
        Ok(())
    }

    /// Whether a merged request moved its task forward or finished it.
    pub(super) async fn merge_ended(&mut self, task: &Task) -> anyhow::Result<bool> {
        let Some(row) = self.store.pull_request_of_task(&task.id).await? else {
            return Ok(false);
        };
        let Some(pull) = crate::forge::live::of_row(&self.store, &self.launcher.live, row).await?
        else {
            return Ok(false);
        };
        if pull.state != "merged" {
            return Ok(false);
        }
        self.reconcile_pull_request(&pull.id).await;
        let after = self.store.get_task(&task.id).await?;
        Ok(after.status() == TaskStatus::Finished || after.step != task.step)
    }

    /// Whether the agent keeping a request that merged is done with it: it
    /// was told the merge and the turn that read it has ended, or it has not
    /// answered in [`QUIET_NUDGE_SECS`] since, or no agent is up to tell at
    /// all (030).
    fn merge_is_read(&self, pull: &PullRequest, keeper: Option<&AgentSession>) -> bool {
        let Some(author) = keeper else {
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

    /// Advance a request column, or end its task when no column follows.
    async fn finish_merged(&mut self, task: &Task, pull: &PullRequest) -> anyhow::Result<()> {
        let Some(step) = task.step.as_deref() else {
            return Ok(());
        };
        let columns = self.store.goal_steps(&task.goal_id).await?;
        let Some(at) = columns.iter().position(|column| column.id == step) else {
            return Ok(());
        };
        if columns[at].gate.as_deref() != Some(StepGate::RequestMerged.as_str()) {
            return Ok(());
        }
        let next_step = columns.get(at + 1).map(|column| column.id.clone());
        let merge_commit = match self.land_merge_locally(pull).await {
            Ok(sha) => sha,
            Err(e) => {
                // The task waits in its column, and the next pass tries
                // again: a task finished on a base that lacks its change would
                // start its dependents without it.
                warn!(task = %task.id, pull_request = %pull.id, error = %format!("{e:#}"),
                    "the merged request is not on the local base yet");
                return Ok(());
            }
        };
        info!(task = %task.id, pull_request = %pull.id, %merge_commit, "the request merged, advancing its task");
        let reason = format!("{} merged", pull.url);
        let advanced = match next_step.as_deref() {
            Some(next) => {
                self.store
                    .move_step(&task.id, next, Actor::Daemon, &reason, Some(&merge_commit))
                    .await
            }
            None => {
                self.store
                    .end_step_by_daemon(&task.id, step, &reason, &merge_commit)
                    .await
            }
        };
        match advanced {
            Ok(_) => Ok(()),
            // The agent completed its step in the meantime.
            Err(StoreError::Transition(_) | StoreError::Conflict(_)) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Bring the merge of a request into the checkout before its step advances
    /// (030), and answer the commit it landed as. The base is fetched from
    /// the remote, the request's own merge commit — as the forge reported it —
    /// must be on it, and the local base branch is fast-forwarded to it, so
    /// the tasks that wait on this one branch from a base that holds it. A
    /// row the forge named no merge commit for is finished by its head where
    /// the head is on the base, as after a fast-forward or rebase merge, and
    /// by the fetched tip as the last resort.
    async fn land_merge_locally(&self, pull: &PullRequest) -> anyhow::Result<String> {
        let repo = self.store.get_repository(&pull.repository_id).await?;
        let path = std::path::PathBuf::from(&repo.path);
        let remote = repo
            .forge
            .as_ref()
            .map_or_else(|| "origin".to_string(), |forge| forge.remote.clone());
        let git = &self.launcher.git;
        let tip = git
            .fetched_tip(&path, &remote, &pull.base_branch)
            .await
            .with_context(|| format!("fetching {} from {remote}", pull.base_branch))?;
        let merge_commit = match &pull.merge_sha {
            Some(sha) => {
                if !git.is_ancestor(&path, sha, &tip).await? {
                    anyhow::bail!(
                        "the merge commit {sha} is not on {remote}/{}",
                        pull.base_branch
                    );
                }
                sha.clone()
            }
            None if git.is_ancestor(&path, &pull.head_sha, &tip).await? => pull.head_sha.clone(),
            None => {
                warn!(pull_request = %pull.id, "the forge named no merge commit; the base tip stands for it");
                tip.clone()
            }
        };
        git.fast_forward(&path, &pull.base_branch, &tip)
            .await
            .with_context(|| format!("fast-forwarding {} in {}", pull.base_branch, repo.path))?;
        Ok(merge_commit)
    }

    /// The live session of the agent that keeps a task's request: the agent
    /// of the column the task is in. None while the task is in no column.
    async fn keeper_of(&self, task: &Task) -> anyhow::Result<Option<AgentSession>> {
        let Some(step) = task.step.as_deref() else {
            return Ok(None);
        };
        let current = self
            .store
            .list_task_agents(&task.id)
            .await?
            .into_iter()
            .find(|agent| agent.step == step)
            .map(|agent| agent.id);
        let live = self
            .store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                live_only: true,
                ..Default::default()
            })
            .await?;
        Ok(live.into_iter().find(|session| {
            session.seat() == Some(Seat::Agent)
                && session.task_agent_id.is_some()
                && session.task_agent_id.as_deref() == current.as_deref()
        }))
    }

    /// Start a review session, or put the one it had back on its feet,
    /// within the spawn-retry budget.
    async fn start_pull_request_session(
        &mut self,
        pull: &PullRequest,
        pin: AgentPin,
    ) -> anyhow::Result<()> {
        let last = self.last_session_of(&pull.id, Seat::Reviewer).await?;
        if let Some(last) = &last
            && self.spent_on_a_dead_launch(&pull.id, &pull.id, last)
        {
            *self.spawn_failures.entry(pull.id.clone()).or_default() += 1;
        }
        if self.spawn_failures.get(&pull.id).copied().unwrap_or(0) >= SPAWN_RETRY_BUDGET {
            // Distinct from `retire_disconnected`'s own flag, which a mere
            // crash also raises and the liveness sweep may still retry:
            // this mark is the budget actually running out — the
            // attention producer's own evidence that nothing automatic is
            // coming for this request any more.
            let _ = self
                .store
                .set_pull_request_reviewer_given_up(&pull.id, false)
                .await;
            return Ok(());
        }
        match self.launcher.resume_pull_request_session(pull, pin).await {
            Ok(session) => {
                info!(pull_request = %pull.id, session = %session.id, "the request's session is up");
                let _ = self
                    .store
                    .clear_pull_request_reviewer_given_up(&pull.id)
                    .await;
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
        let news = news::untold(
            &self.store,
            &self.launcher.live,
            pull,
            login,
            session.seat(),
        )
        .await?;
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
    /// taken down (`Launcher::end_pull_request_sessions`). The task's own
    /// cleanup took its worktree and its branch.
    async fn end_kept_request(&mut self, pull: &PullRequest) -> anyhow::Result<()> {
        info!(pull_request = %pull.id, state = %pull.state, "the request ended, taking its work down");
        // Done only once its sessions and worktree are gone: until then the
        // row stays, so a restart owes it the same. A cleanup that failed is
        // tried again, on the next change of the request at once and on the
        // tick after a wait.
        let cleaned = self
            .launcher
            .end_pull_request_sessions(&pull.id, &pull.repository_id, Some(pull))
            .await;
        match cleaned {
            Ok(()) => self.stop_working(&pull.id, pull.into()).await,
            Err(e) => {
                self.pull_request_cleanup_retry
                    .insert(pull.id.clone(), Instant::now() + CLEANUP_RETRY);
                Err(e)
            }
        }
    }

    /// A request that asks for my review (029): one live reviewer session
    /// while it is open, out of draft, still asks, and was asked for
    /// directly on its own row; its sessions and worktree taken down
    /// otherwise.
    async fn review_pass(&mut self, pull: &PullRequest) -> anyhow::Result<()> {
        let integration = self.store.forge_integration(&pull.repository_id).await?;
        let live = self.live_reviewer_sessions(&pull.id).await?;
        let login = integration
            .as_ref()
            .and_then(|i| i.login.clone())
            .unwrap_or_default();
        if !integration.is_some_and(|i| wants_session(pull, &i)) {
            return self.end_review(pull, &live).await;
        }
        let running = live
            .iter()
            .find(|s| s.seat() == Some(Seat::Reviewer) && self.launcher.acp.is_running(&s.id))
            .cloned();
        // A live process is not by itself genuine recovery: the watchdog's
        // own exhausted-relaunch give-up (`quiet::relaunch_wedged`) leaves
        // a wedged session's process running, silent, exactly as it was —
        // and that same still-live, still-silent session must not clear
        // its own give-up mark on every later pass that merely finds it
        // still there. Only a session that has reported *something* since
        // the mark was set — its own `last_activity_at` moved past
        // `reviewer_given_up_at` — is the genuine progress that answers
        // for it.
        if let (Some(session), Some(given_up)) = (&running, pull.reviewer_given_up_at.as_deref())
            && session
                .last_activity_at
                .as_deref()
                .is_some_and(|at| at > given_up)
        {
            let _ = self
                .store
                .clear_pull_request_reviewer_given_up(&pull.id)
                .await;
        }
        let Some(session) = running else {
            // Every role is reviewed on the pin asked directly on its own
            // row (029): nothing else ever starts a session for it.
            let pin = AgentPin {
                model: pull.review_model.clone().unwrap_or_default(),
                effort: pull.review_effort.clone(),
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
    /// mine. Ariadne then stops working on it, but for a draft, whose review
    /// waits for it to leave draft. A request of mine ends its review alone:
    /// its row is its keeper's side to end (`end_kept_request`).
    async fn end_review(
        &mut self,
        pull: &PullRequest,
        live: &[AgentSession],
    ) -> anyhow::Result<()> {
        // Reached only once `review_pass` has already found the request no
        // longer wants a reviewer session, for any reason — closed, no
        // longer asking, back to draft, or never asked for directly on its
        // own row. Whatever earlier give-up this request's reviewer seat
        // carries is no longer this recovery's to answer for: the work it
        // was given up on does not exist any more either way.
        let _ = self
            .store
            .clear_pull_request_reviewer_given_up(&pull.id)
            .await;
        let done = pull.role == "reviewer" && (pull.state != "open" || !pull.review_requested);
        if live.is_empty() && !self.launcher.pull_request_worktree_exists(&pull.id) && !done {
            return Ok(());
        }
        info!(pull_request = %pull.id, state = %pull.state, "the request wants no review, taking its work down");
        match self
            .launcher
            .end_reviewer_sessions(&pull.id, &pull.repository_id)
            .await
        {
            Ok(()) => {
                self.pull_request_cleanup_retry.remove(&pull.id);
                if done {
                    self.stop_working(&pull.id, pull.into()).await?;
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

    /// A request of mine the user asked Ariadne to keep by hand (032): one
    /// live keeper session while it is open and still asked for directly on
    /// its own row; its session and worktree taken down otherwise.
    async fn babysit_pass(&mut self, pull: &PullRequest) -> anyhow::Result<()> {
        let integration = self.store.forge_integration(&pull.repository_id).await?;
        let live = self.live_keeper_sessions(&pull.id).await?;
        let login = integration
            .as_ref()
            .and_then(|i| i.login.clone())
            .unwrap_or_default();
        if !integration.is_some_and(|i| wants_babysit_session(pull, &i)) {
            return self.end_babysit(pull, &live).await;
        }
        let running = live
            .iter()
            .find(|s| s.seat() == Some(Seat::Keeper) && self.launcher.acp.is_running(&s.id))
            .cloned();
        let pin = AgentPin {
            model: pull.babysit_model.clone().unwrap_or_default(),
            effort: pull.babysit_effort.clone(),
        };
        let skills = pull.babysit_skills();
        let Some(session) = running else {
            self.keeper_skills_applied
                .insert(pull.id.clone(), skills.clone());
            return self.start_babysit_session(pull, pin).await;
        };
        // A second ask can move the pin or the skills while this session
        // still runs on the old ones (032): kill it and start fresh on the
        // new ones, which reuses the worktree and the local branch rather
        // than cutting them again, so its unpushed local work survives
        // the restart. The skills a live session runs are not on its own
        // row, so they are read off the last ask this map saw rather than
        // off the session: absent, as right after the daemon restarts, a
        // session already running is taken to run the current ones.
        let skills_changed = self
            .keeper_skills_applied
            .get(&pull.id)
            .is_some_and(|applied| *applied != skills);
        if session.model != pin.model || session.effort != pin.effort || skills_changed {
            info!(pull_request = %pull.id, session = %session.id, "the keeper's pin changed, restarting it on the new one");
            self.launcher.kill_session(&session.id).await?;
            self.keeper_skills_applied
                .insert(pull.id.clone(), skills.clone());
            return self.start_babysit_session(pull, pin).await;
        }
        self.keeper_skills_applied.insert(pull.id.clone(), skills);
        if matches!(
            session.status(),
            SessionStatus::Running | SessionStatus::Idle
        ) {
            self.tell_pull_request(&session, pull, &login).await?;
        }
        if session.status() == SessionStatus::Running {
            let repo = self.store.get_repository(&pull.repository_id).await?;
            let worktree = session.worktree_path.clone().unwrap_or_default();
            let branch = format!("ariadne/keep-{}", pull.id);
            let briefing = self
                .launcher
                .keeper_briefing_of(pull, &repo, std::path::Path::new(&worktree), &branch)
                .await?;
            self.check_session_quiet(&session, SITUATION.to_string(), &briefing)
                .await?;
        }
        Ok(())
    }

    /// Start a keeper session, or put the one it had back on its feet,
    /// within the spawn-retry budget: the same guard
    /// `start_pull_request_session` keeps a reviewer under, on a budget
    /// key of its own (`<id>:keep`) so a dead keeper launch never spends
    /// the reviewer's count, or the other way round, on a row that asks
    /// for both.
    async fn start_babysit_session(
        &mut self,
        pull: &PullRequest,
        pin: AgentPin,
    ) -> anyhow::Result<()> {
        let budget = format!("{}:keep", pull.id);
        let last = self.last_session_of(&pull.id, Seat::Keeper).await?;
        if let Some(last) = &last
            && self.spent_on_a_dead_launch(&budget, &budget, last)
        {
            *self.spawn_failures.entry(budget.clone()).or_default() += 1;
        }
        if self.spawn_failures.get(&budget).copied().unwrap_or(0) >= SPAWN_RETRY_BUDGET {
            warn!(pull_request = %pull.id, "the keeper session died on arrival too many times, giving up for now");
            return Ok(());
        }
        match self.launcher.resume_keeper_session(pull, pin).await {
            Ok(session) => {
                info!(pull_request = %pull.id, session = %session.id, "the request's keeper session is up");
                let _ = self
                    .store
                    .clear_pull_request_reviewer_given_up(&pull.id)
                    .await;
                Ok(())
            }
            Err(e) => {
                *self.spawn_failures.entry(budget).or_default() += 1;
                Err(e)
            }
        }
    }

    /// The newest session of `seat` on a request, if any: what the
    /// spawn-retry budget reads to tell a dead launch from a live one, kept
    /// seat-scoped since a row can carry a reviewer and a keeper session
    /// at once (032).
    async fn last_session_of(
        &self,
        pull_request_id: &str,
        seat: Seat,
    ) -> anyhow::Result<Option<AgentSession>> {
        Ok(self
            .store
            .list_sessions(SessionFilter {
                pull_request_id: Some(pull_request_id.to_string()),
                ..Default::default()
            })
            .await?
            .into_iter()
            .rfind(|s| s.seat() == Some(seat)))
    }

    /// A request asked for by hand that wants no keeper session (032):
    /// merged, closed, or no longer asking. Its session is killed, its
    /// worktree and local branch removed; Ariadne then stops working on it,
    /// but for the task that still keeps it or the review still asked of it
    /// (`pull_request_pass`'s own outer check decides that).
    async fn end_babysit(
        &mut self,
        pull: &PullRequest,
        live: &[AgentSession],
    ) -> anyhow::Result<()> {
        // Unlike a self-review, which only ends on an explicit `asked:
        // false`, babysitting ends on the merge or the close too: nothing
        // else keeps a hand-babysat row once this ends, since asking it is
        // refused wherever a task already keeps the request (032).
        let done = pull.origin_task_id.is_none() && !pull.review_asked;
        if live.is_empty() && !self.launcher.keeper_worktree_exists(&pull.id) && !done {
            return Ok(());
        }
        info!(pull_request = %pull.id, state = %pull.state, "the request wants no keeper, taking its work down");
        match self
            .launcher
            .end_keeper_sessions(&pull.id, &pull.repository_id, pull)
            .await
        {
            Ok(()) => {
                self.pull_request_cleanup_retry.remove(&pull.id);
                if done {
                    self.stop_working(&pull.id, pull.into()).await?;
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

    async fn live_reviewer_sessions(&self, id: &str) -> anyhow::Result<Vec<AgentSession>> {
        Ok(self
            .live_pull_request_sessions(id)
            .await?
            .into_iter()
            .filter(|s| s.seat() == Some(Seat::Reviewer))
            .collect())
    }

    async fn live_keeper_sessions(&self, id: &str) -> anyhow::Result<Vec<AgentSession>> {
        Ok(self
            .live_pull_request_sessions(id)
            .await?
            .into_iter()
            .filter(|s| s.seat() == Some(Seat::Keeper))
            .collect())
    }
}

/// Whether a task is done with its request: finished, cancelled, or failed.
/// A failed task is retried by a person, whose retry opens another request.
fn ends_its_work(task: &Task) -> bool {
    task.status().is_terminal() || task.status() == TaskStatus::Failed
}
