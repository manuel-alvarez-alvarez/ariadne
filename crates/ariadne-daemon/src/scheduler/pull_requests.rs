//! What a pull request of mine wants (026): one live session while it is
//! open, the news handed to that session once, and its work taken down once
//! a human merged or closed it.
//!
//! The daemon staffs the session on the repository's `babysit_model` and
//! `babysit_effort` (025, 017), never an orchestrator. The session waits on
//! the forge between turns, so an idle one is never nudged, flagged or
//! relaunched for sitting idle (009 rule 41); a turn that never ends is
//! watched as any other agent's is.

use std::time::{Duration, Instant};

use tracing::{info, warn};

use ariadne_core::{Seat, SessionStatus};
use ariadne_store::{
    AgentPin, AgentSession, PullRequest, PullRequestFilter, SessionFilter, StoreError,
};

use crate::acp::NewsDelivery;
use crate::agents::prompts;
use crate::forge::{news, poll::wants_session};
use crate::launcher::HeadBranchOccupied;

use super::{QUIET_NUDGE_SECS, SPAWN_RETRY_BUDGET};

/// How long the tick waits before it tries a failed cleanup again. A change
/// of the request tries at once.
const CLEANUP_RETRY: Duration = Duration::from_secs(60);

/// The situation a pull request session's watchdog spends its steps in:
/// there is one, the request, for as long as it is open.
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
        for row in rows.into_iter().filter(|row| row.role == "author") {
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
        // A request the user reviews is another lifecycle's (a later task):
        // nothing here starts, tells or takes down anything of it.
        if pull.role != "author" {
            return Ok(());
        }
        let integration = self.store.forge_integration(&pull.repository_id).await?;
        let live = self.live_pull_request_sessions(&pull.id).await?;
        if pull.state != "open" {
            return self.end_pull_request(&pull, &live).await;
        }
        let Some(integration) = integration.filter(|i| wants_session(&pull, i)) else {
            // A request of nobody's session: one that started under another
            // setting is ended, and nothing new starts.
            for session in live {
                info!(pull_request = %pull.id, session = %session.id, "the request wants no session, killing it");
                self.launcher.kill_session(&session.id).await.ok();
            }
            return Ok(());
        };
        let login = integration.login.clone().unwrap_or_default();
        let running = live
            .iter()
            .find(|s| s.seat() == Some(Seat::Author) && self.launcher.acp.is_running(&s.id))
            .cloned();
        let Some(session) = running else {
            let pin = AgentPin {
                model: integration.babysit_model.clone().unwrap_or_default(),
                effort: integration.babysit_effort.clone(),
            };
            return self.start_pull_request_session(&pull, pin).await;
        };
        self.tell_pull_request(&session, &pull, &login).await?;
        // Idle is the session waiting on the forge, not silence: only a turn
        // that never ends is watched, and a relaunch is briefed again.
        if session.status() == SessionStatus::Running {
            let repo = self.store.get_repository(&pull.repository_id).await?;
            let worktree = session.worktree_path.clone().unwrap_or_default();
            let briefing = self.launcher.pull_request_briefing_of(
                &pull,
                &repo,
                std::path::Path::new(&worktree),
            );
            self.check_session_quiet(&session, SITUATION.to_string(), &briefing)
                .await?;
        }
        Ok(())
    }

    /// Start the request's session, or put the one it had back on its feet,
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
        let skill = crate::launcher::pull_request_skill(Seat::Author).unwrap_or_default();
        match self
            .launcher
            .resume_pull_request_session(pull, Seat::Author, pin, skill)
            .await
        {
            Ok(session) => {
                info!(pull_request = %pull.id, session = %session.id, "the request's session is up");
                Ok(())
            }
            // The head branch is checked out somewhere else — the author of
            // the task that opened the request may still be finishing it.
            // That is a wait, not a launch that failed: no attempt is spent,
            // and the next pass tries again.
            Err(e) if e.downcast_ref::<HeadBranchOccupied>().is_some() => {
                info!(pull_request = %pull.id, error = %format!("{e:#}"), "the request's head branch is in use, waiting");
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
        let news = news::untold(&self.store, pull, login).await?;
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

    /// A request a human merged or closed: its session told so once, and
    /// killed once the turn that reads it ended; then its worktree removed
    /// and its branch taken down (`Launcher::cleanup_pull_request`).
    ///
    /// Told is what the row says, not what was queued: older news queued
    /// behind a running turn holds the request's place in the queue, so the
    /// end is handed again on every pass until a claim wrote it (018). The
    /// turn that reads it is the one that ends after that claim.
    async fn end_pull_request(
        &mut self,
        pull: &PullRequest,
        live: &[AgentSession],
    ) -> anyhow::Result<()> {
        let integration = self.store.forge_integration(&pull.repository_id).await?;
        let enabled = integration.as_ref().is_some_and(|i| i.enabled);
        let login = integration.and_then(|i| i.login).unwrap_or_default();
        for session in live {
            if !self.launcher.acp.is_running(&session.id) {
                continue;
            }
            let since = *self
                .pull_request_farewell
                .entry(session.id.clone())
                .or_insert_with(Instant::now);
            let waited = since.elapsed().as_secs() >= QUIET_NUDGE_SECS as u64;
            // A disabled integration closed the row (rule 25): nobody merged
            // or closed anything, so there is nothing to tell.
            if !enabled || waited {
                continue;
            }
            if pull.told_state.as_deref() != Some(pull.state.as_str()) {
                self.tell_pull_request(session, pull, &login).await?;
                return Ok(());
            }
            let read = session.status() == SessionStatus::Idle
                && match (&session.last_activity_at, &pull.news_told_at) {
                    (Some(heard), Some(told)) => heard > told,
                    _ => false,
                };
            if !read {
                return Ok(());
            }
        }
        if live.is_empty()
            && !self.launcher.pull_request_worktree_exists(pull)
            && pull.cleaned_at.is_some()
        {
            return Ok(());
        }
        for session in live {
            self.pull_request_farewell.remove(&session.id);
        }
        info!(pull_request = %pull.id, state = %pull.state, "the request ended, taking its work down");
        // Done only once every branch it owes is gone, which the row records,
        // so a restart owes it the same. A deletion that failed is tried
        // again, on the next change of the request at once and on the tick
        // after a wait. Where the integration was disabled the sessions end
        // and no branch is touched.
        let cleaned = match enabled {
            true => self.launcher.cleanup_pull_request(pull).await,
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
