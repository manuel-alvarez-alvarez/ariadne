//! Task repository: creation, dependency management, and the single
//! transactional entry point for status transitions.

use std::collections::{HashMap, HashSet};

use ariadne_core::id::new_id;
use ariadne_core::{
    Actor, AttentionReason, Landing, PermissionMode, Seat, TaskStatus, check_transition,
};

use crate::query::Filtered;
use crate::{
    AgentPin, Change, NewTaskAgent, Result, Store, StoreError, Task, TaskAgent, TaskTransition,
    not_found, now,
};

#[derive(Debug, Clone)]
pub struct NewTask {
    pub goal_id: String,
    pub repo_id: String,
    pub title: String,
    pub description: String,
    /// The agents to staff: one author or more, then the reviewers in review
    /// order. What each one can do is the skills it carries. Several authors
    /// need at least one reviewer, to pick the winner.
    pub agents: Vec<NewTaskAgent>,
    pub depends_on: Vec<String>,
    /// How this task ends. None = the way its repository takes a change.
    pub landing: Option<Landing>,
    /// None takes the daemon's configured default at launch time.
    pub permission_mode: Option<PermissionMode>,
}

#[derive(Debug, Clone, Default)]
pub struct TaskUpdate {
    pub title: Option<String>,
    pub description: Option<String>,
    /// What the author runs on: `Some(pin)` moves it there, None leaves it
    /// alone.
    pub pin: Option<AgentPin>,
    /// The effort alone, for an edit that leaves the model where it is:
    /// `Some(Some(effort))` runs the pinned model at it, `Some(None)` runs it
    /// at whatever the CLI runs it at, None says nothing. Read only where
    /// `pin` says nothing — a pin that moves carries its own effort.
    pub effort: Option<Option<String>>,
    /// The whole author list, replaced: every author is staffed afresh, with
    /// the skills and the pin the caller gave it. Only a task that has not
    /// started can be edited at all, so no author session is ever replaced.
    pub authors: Option<Vec<NewTaskAgent>>,
    /// The whole reviewer list, replaced: every reviewer is staffed afresh,
    /// with the skills and the pin the caller gave it.
    pub reviewers: Option<Vec<NewTaskAgent>>,
    /// How the task ends. None leaves it where it is.
    pub landing: Option<Landing>,
}

#[derive(Debug, Clone, Default)]
pub struct TaskFilter {
    pub goal_id: Option<String>,
    pub status: Option<TaskStatus>,
}

/// How much of the title a branch name keeps, in characters. Long enough for
/// a sentence of title, short enough that the name still reads at a glance in
/// a `git branch` listing or on a pull request.
const SLUG_MAX: usize = 40;

/// How much of the task id rides at the end of a branch name. Six characters
/// of a ULID's random tail: enough that two tasks with the same title never
/// share a branch, short enough to read out.
const ID_TAIL: usize = 6;

/// The branch a task is created on: a slug of its title, then the tail of its
/// id — `fix-the-landing-briefing-real-fetch-r9jr7c`. The branch is what shows
/// on a published request, so it names the change and nothing else: no prefix,
/// no `ariadne` anywhere in it.
///
/// Only ASCII letters and digits survive, which keeps the result a valid git
/// ref (`git check-ref-format --branch`) whatever the title was. A title with
/// nothing to slug falls back to `task-<tail>`.
fn branch_name(title: &str, id: &str) -> String {
    let tail = id_tail(id);
    let slug = slug(title);
    let head = if slug.is_empty() { "task" } else { &slug };
    if tail.is_empty() {
        head.to_string()
    } else {
        format!("{head}-{tail}")
    }
}

/// The last [`ID_TAIL`] characters of an id, in the lowercase alphanumeric
/// form a branch name can carry.
fn id_tail(id: &str) -> String {
    let id: String = id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    id[id.len().saturating_sub(ID_TAIL)..].to_string()
}

/// The branch one author of a task works on.
///
/// The first author holds the task branch itself, so a one-author task reads
/// exactly as it always did. Every later author works beside it under the
/// same name with an `-a<n>` tail, numbered the way the orchestrator listed
/// them: `fix-the-parser-r9jr7c-a2` is the second author's branch.
pub fn author_branch(task_branch: &str, ordinal: i64) -> String {
    match ordinal {
        0 => task_branch.to_string(),
        n => format!("{task_branch}-a{}", n + 1),
    }
}

/// A title as lowercase kebab-case, clipped to [`SLUG_MAX`] characters on a
/// word boundary where there is one to clip on.
fn slug(title: &str) -> String {
    let mut slug = String::with_capacity(title.len());
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            // Every run of anything else collapses into one separator, and a
            // leading one never starts the slug.
            slug.push('-');
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }

    if slug.len() <= SLUG_MAX {
        return slug;
    }
    // The slug is pure ASCII, so the budget is a byte index. Cutting there can
    // land mid-word: back off to the last separator before it. A first word
    // longer than the budget has none to back off to and is cut where it falls.
    let cut = if slug.as_bytes()[SLUG_MAX] == b'-' {
        SLUG_MAX
    } else {
        slug[..SLUG_MAX].rfind('-').unwrap_or(SLUG_MAX)
    };
    slug.truncate(cut);
    slug
}

impl Store {
    /// Create a task in `pending`. Checks the staffing, and checks that the
    /// dependencies belong to the same goal and are acyclic.
    ///
    /// Nothing caps how many tasks a goal takes: how a goal breaks down is
    /// what the orchestrator settles with the user before it writes any of
    /// them (003), and a number the store enforced afterwards could only
    /// refuse a plan they had already agreed.
    pub async fn create_task(&self, new: NewTask) -> Result<Task> {
        check_staffing(&new.agents)?;
        let goal = self.get_goal(&new.goal_id).await?;
        let repo = self.get_repository(&new.repo_id).await?;
        if !self
            .list_goal_repositories(&goal.id)
            .await?
            .iter()
            .any(|r| r.id == repo.id)
        {
            return Err(StoreError::Invalid(format!(
                "repo {} does not belong to goal {}",
                repo.id, goal.id
            )));
        }

        let id = new_id();
        let ts = now();
        let branch = branch_name(&new.title, &id);

        let mut tx = self.w().begin().await?;

        sqlx::query(
            "INSERT INTO tasks (id, goal_id, repo_id, title, description, status, branch,
                                landing, permission_mode, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, 'pending', ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&goal.id)
        .bind(&repo.id)
        .bind(&new.title)
        .bind(&new.description)
        .bind(&branch)
        // Landing on the base branch is what a task ends with unless whoever
        // wrote it said otherwise: it is what most work does, and the other
        // two are the ones somebody chooses.
        .bind(new.landing.unwrap_or(Landing::Merge).as_str())
        .bind(new.permission_mode.map(|mode| mode.as_str()))
        .bind(&ts)
        .bind(&ts)
        .execute(&mut *tx)
        .await?;

        Self::staff_agents_in_tx(&mut tx, &id, &new.agents).await?;

        if !new.depends_on.is_empty() {
            Self::insert_dependencies(&mut tx, &goal.id, &id, &new.depends_on).await?;
        }

        tx.commit().await?;
        let task = self.get_task(&id).await?;
        self.publish(Change::TaskCreated(task.clone()));
        Ok(task)
    }

    /// Replace the dependency set of a task (orchestrator, pre-start only).
    pub async fn set_task_dependencies(&self, task_id: &str, depends_on: &[String]) -> Result<()> {
        let mut tx = self.w().begin().await?;
        // Status is validated on the row inside the write transaction: a check
        // against the read pool could be stale by the time we hold the lock.
        let task: Task = Self::fetch_by_in_tx(&mut tx, "task", "tasks", task_id).await?;
        if !matches!(task.status(), TaskStatus::Pending | TaskStatus::Ready) {
            return Err(StoreError::Conflict(format!(
                "dependencies can only change while pending/ready, task is {}",
                task.status
            )));
        }
        sqlx::query("DELETE FROM task_dependencies WHERE task_id = ?")
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
        Self::insert_dependencies(&mut tx, &task.goal_id, task_id, depends_on).await?;
        // A task that was already ready may need to wait again.
        let transition = if task.status() == TaskStatus::Ready && !depends_on.is_empty() {
            Some(
                Self::transition_in_tx(
                    &mut tx,
                    &task,
                    TaskStatus::Pending,
                    Actor::Orchestrator,
                    Some("dependencies changed"),
                    None,
                )
                .await?,
            )
        } else {
            None
        };
        tx.commit().await?;
        let task = self.get_task(task_id).await?;
        self.publish(Change::TaskUpdated { task, transition });
        Ok(())
    }

    /// Validate deps exist, belong to `goal_id`, and introduce no cycle;
    /// insert them.
    async fn insert_dependencies(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        goal_id: &str,
        task_id: &str,
        depends_on: &[String],
    ) -> Result<()> {
        for dep in depends_on {
            if dep == task_id {
                return Err(StoreError::Invalid("a task cannot depend on itself".into()));
            }
            let dep_goal: Option<String> =
                sqlx::query_scalar("SELECT goal_id FROM tasks WHERE id = ?")
                    .bind(dep)
                    .fetch_optional(&mut **tx)
                    .await?;
            match dep_goal {
                None => return Err(not_found("task", dep)),
                Some(g) if g != goal_id => {
                    return Err(StoreError::Invalid(format!(
                        "dependency {dep} belongs to a different goal"
                    )));
                }
                _ => {}
            }
        }

        // Cycle check over existing edges plus the new ones.
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT td.task_id, td.depends_on_task_id FROM task_dependencies td
             JOIN tasks t ON t.id = td.task_id WHERE t.goal_id = ?",
        )
        .bind(goal_id)
        .fetch_all(&mut **tx)
        .await?;
        let mut edges: HashMap<&str, Vec<&str>> = HashMap::new();
        for (from, to) in &rows {
            edges.entry(from.as_str()).or_default().push(to.as_str());
        }
        for dep in depends_on {
            edges.entry(task_id).or_default().push(dep.as_str());
        }
        // DFS from task_id: reaching task_id again means a cycle.
        let mut stack: Vec<&str> = edges.get(task_id).cloned().unwrap_or_default();
        let mut seen: HashSet<&str> = HashSet::new();
        while let Some(node) = stack.pop() {
            if node == task_id {
                return Err(StoreError::Invalid("dependency cycle detected".into()));
            }
            if seen.insert(node)
                && let Some(next) = edges.get(node)
            {
                stack.extend(next.iter().copied());
            }
        }

        for dep in depends_on {
            sqlx::query("INSERT OR IGNORE INTO task_dependencies (task_id, depends_on_task_id) VALUES (?, ?)")
                .bind(task_id)
                .bind(dep)
                .execute(&mut **tx)
                .await?;
        }
        Ok(())
    }

    pub async fn get_task(&self, id: &str) -> Result<Task> {
        self.fetch_by("task", "tasks", "id", id).await
    }

    pub async fn list_tasks(&self, filter: TaskFilter) -> Result<Vec<Task>> {
        Filtered::new("tasks")
            .maybe(" AND goal_id = ?", filter.goal_id)
            .maybe(" AND status = ?", filter.status.map(|s| s.as_str()))
            .fetch(self, " ORDER BY id", &[])
            .await
    }

    pub async fn update_task(&self, id: &str, update: TaskUpdate) -> Result<Task> {
        let mut tx = self.w().begin().await?;
        // Status is validated on the row inside the write transaction: a check
        // against the read pool could be stale by the time we hold the lock.
        let task: Task = Self::fetch_by_in_tx(&mut tx, "task", "tasks", id).await?;
        if !matches!(task.status(), TaskStatus::Pending | TaskStatus::Ready) {
            return Err(StoreError::Conflict(format!(
                "task can only be edited while pending/ready, it is {}",
                task.status
            )));
        }
        let landing = update.landing.unwrap_or_else(|| task.landing());
        let title = update.title.unwrap_or(task.title);
        let description = update.description.unwrap_or(task.description);
        sqlx::query(
            "UPDATE tasks SET title = ?, description = ?, landing = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&title)
        .bind(&description)
        .bind(landing.as_str())
        .bind(now())
        .bind(id)
        .execute(&mut *tx)
        .await?;
        if let Some(authors) = &update.authors {
            // Reassigning authors staffs them afresh, each with the skills
            // and the pin the caller gave it, the same way creation does. The
            // task has not started, so no author session is replaced by this.
            sqlx::query("DELETE FROM task_agents WHERE task_id = ? AND seat = 'author'")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            Self::staff_agents_in_tx(&mut tx, id, authors).await?;
        }
        if let Some(reviewers) = &update.reviewers {
            sqlx::query("DELETE FROM task_agents WHERE task_id = ? AND seat = 'reviewer'")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            // Reassigning reviewers staffs them afresh, each with the skills
            // and the pin the caller gave it, the same way creation does.
            Self::staff_agents_in_tx(&mut tx, id, reviewers).await?;
        }
        // What the edits above left staffed, checked whole: an edit must not
        // leave a task no creation would have taken.
        let authors: Vec<TaskAgent> = sqlx::query_as(
            "SELECT * FROM task_agents WHERE task_id = ? AND seat = 'author' ORDER BY ordinal",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let reviewers: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_agents WHERE task_id = ? AND seat = 'reviewer'",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        check_seat_counts(authors.len(), reviewers as usize)?;
        // The pins live on the author, which is the agent the task's own
        // `--model` and `--effort` have always meant. A task with several
        // authors has no such agent: each one was named with its own pin.
        if update.pin.is_some() || update.effort.is_some() {
            let [author] = authors.as_slice() else {
                return Err(StoreError::Conflict(
                    "the task has several authors; replace the whole list with `authors`, \
                     each with its own model"
                        .into(),
                ));
            };
            let (model, effort) = match &update.pin {
                // The model stands, so an effort of its own moves alone: what
                // it is run at is the model the author is already pinned to.
                None => (
                    author.model.clone(),
                    update
                        .effort
                        .clone()
                        .unwrap_or_else(|| author.effort.clone()),
                ),
                Some(pin) => AgentPin::columns(pin),
            };
            sqlx::query("UPDATE task_agents SET model = ?, effort = ? WHERE id = ?")
                .bind(&model)
                .bind(&effort)
                .bind(&author.id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        let task = self.get_task(id).await?;
        self.publish(Change::TaskUpdated {
            task: task.clone(),
            transition: None,
        });
        Ok(task)
    }

    /// The one and only way to change a task's status.
    ///
    /// Validates against the core state machine, applies side-column updates
    /// (the merge commit) and writes the audit row — all in one transaction.
    pub async fn transition_task(
        &self,
        id: &str,
        to: TaskStatus,
        actor: Actor,
        reason: Option<&str>,
        merge_commit: Option<&str>,
    ) -> Result<Task> {
        let mut tx = self.w().begin().await?;
        let task: Task = Self::fetch_by_in_tx(&mut tx, "task", "tasks", id).await?;
        let transition =
            Self::transition_in_tx(&mut tx, &task, to, actor, reason, merge_commit).await?;
        tx.commit().await?;
        let task = self.get_task(id).await?;
        self.publish(Change::TaskUpdated {
            task: task.clone(),
            transition: Some(transition),
        });
        Ok(task)
    }

    /// Validate against the state machine, apply the status change with its
    /// side-column updates, and write the audit row — the shared body of every
    /// status change, inside the caller's transaction. Returns the audit row
    /// so callers can attach it to the change notification.
    async fn transition_in_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        task: &Task,
        to: TaskStatus,
        actor: Actor,
        reason: Option<&str>,
        merge_commit: Option<&str>,
    ) -> Result<TaskTransition> {
        let from = task.status();
        check_transition(from, to, actor)?;

        // A task that lands something is finished by the sha it landed as.
        // One that lands nothing has no sha to give, and demanding one would
        // make a filed report or a cut release impossible to finish.
        if to == TaskStatus::Finished && task.landing() != Landing::None && merge_commit.is_none() {
            return Err(StoreError::Invalid(
                "finished transition requires a merge commit".into(),
            ));
        }

        // The stall is not reset here: it belongs to the agent that stopped
        // working and comes down when that agent's own flag does
        // (`sync_task_stall`).
        sqlx::query(
            "UPDATE tasks SET status = ?, merge_commit = COALESCE(?, merge_commit),
                              updated_at = ?
             WHERE id = ?",
        )
        .bind(to.as_str())
        .bind(merge_commit)
        .bind(now())
        .bind(&task.id)
        .execute(&mut **tx)
        .await?;

        let transition = TaskTransition {
            id: new_id(),
            task_id: task.id.clone(),
            from_status: from.as_str().to_string(),
            to_status: to.as_str().to_string(),
            actor: actor.as_str().to_string(),
            reason: reason.map(str::to_string),
            created_at: now(),
        };
        sqlx::query(
            "INSERT INTO task_transitions (id, task_id, from_status, to_status, actor, reason, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&transition.id)
        .bind(&transition.task_id)
        .bind(&transition.from_status)
        .bind(&transition.to_status)
        .bind(&transition.actor)
        .bind(&transition.reason)
        .bind(&transition.created_at)
        .execute(&mut **tx)
        .await?;

        Ok(transition)
    }

    pub async fn list_task_transitions(&self, task_id: &str) -> Result<Vec<TaskTransition>> {
        Ok(sqlx::query_as::<_, TaskTransition>(
            "SELECT * FROM task_transitions WHERE task_id = ? ORDER BY id",
        )
        .bind(task_id)
        .fetch_all(self.r())
        .await?)
    }

    /// The summary the author asked for review with, for the review that is
    /// open now: the reason of the most recent `under_review` transition.
    ///
    /// Read off the conversation it would be whatever the author happened to
    /// write last, and what the reviewers are handed has to be what it
    /// submitted.
    pub async fn review_summary(&self, task_id: &str) -> Result<Option<String>> {
        Ok(sqlx::query_scalar::<_, Option<String>>(
            "SELECT reason FROM task_transitions
              WHERE task_id = ? AND to_status = ?
              ORDER BY id DESC LIMIT 1",
        )
        .bind(task_id)
        .bind(TaskStatus::UnderReview.as_str())
        .fetch_optional(self.r())
        .await?
        .flatten())
    }

    /// Why a task that ended without merging ended: the reason on the
    /// transition that put it into `failed` or `cancelled`.
    ///
    /// The status is the fact and the transition is the words, so this is
    /// where the two are put back together — what an author's `fail_task`
    /// said, what a dependency that never landed said, what cancelled the
    /// goal. Read off the audit row that recorded it rather than kept in a
    /// column of its own, which would be a second copy to drift.
    ///
    /// `None` for a task that has not ended that way, and for one that ended
    /// with nothing said.
    pub async fn ended_reason(&self, task: &Task) -> Result<Option<String>> {
        let status = task.status();
        if !matches!(status, TaskStatus::Failed | TaskStatus::Cancelled) {
            return Ok(None);
        }
        Ok(sqlx::query_scalar::<_, Option<String>>(
            "SELECT reason FROM task_transitions
              WHERE task_id = ? AND to_status = ?
              ORDER BY id DESC LIMIT 1",
        )
        .bind(&task.id)
        .bind(status.as_str())
        .fetch_optional(self.r())
        .await?
        .flatten())
    }

    pub async fn list_task_dependencies(&self, task_id: &str) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT depends_on_task_id FROM task_dependencies WHERE task_id = ? ORDER BY depends_on_task_id",
        )
        .bind(task_id)
        .fetch_all(self.r())
        .await?)
    }

    /// True when every dependency of the task is finished.
    pub async fn task_dependencies_merged(&self, task_id: &str) -> Result<bool> {
        let unmerged: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_dependencies td
             JOIN tasks dep ON dep.id = td.depends_on_task_id
             WHERE td.task_id = ? AND dep.status <> 'finished'",
        )
        .bind(task_id)
        .fetch_one(self.r())
        .await?;
        Ok(unmerged == 0)
    }

    /// The first dependency of the task that ended without merging — `failed`
    /// or `cancelled` — if there is one.
    ///
    /// Such a dependency is never going to merge, so the task behind it is
    /// never going to start: the scheduler reads this to end it rather than
    /// leave it waiting for ever. `None` while every dependency can still get
    /// there, merged ones included.
    pub async fn task_dependencies_blocked(&self, task_id: &str) -> Result<Option<Task>> {
        Ok(sqlx::query_as::<_, Task>(
            "SELECT dep.* FROM task_dependencies td
             JOIN tasks dep ON dep.id = td.depends_on_task_id
             WHERE td.task_id = ? AND dep.status IN ('failed', 'cancelled')
             ORDER BY dep.id
             LIMIT 1",
        )
        .bind(task_id)
        .fetch_optional(self.r())
        .await?)
    }

    pub async fn set_task_worktree(
        &self,
        task_id: &str,
        worktree_path: Option<&str>,
    ) -> Result<()> {
        let n = sqlx::query("UPDATE tasks SET worktree_path = ?, updated_at = ? WHERE id = ?")
            .bind(worktree_path)
            .bind(now())
            .bind(task_id)
            .execute(self.w())
            .await?
            .rows_affected();
        self.publish_task_update(task_id, n).await
    }

    /// Record the pull or merge request a task was published as.
    ///
    /// The URL is the whole of it: nothing polls the request but the
    /// author's own session, and the URL is what the UI and the CLI show.
    pub async fn set_task_pull_request(&self, task_id: &str, url: &str) -> Result<()> {
        let n = sqlx::query("UPDATE tasks SET pr_url = ?, updated_at = ? WHERE id = ?")
            .bind(url)
            .bind(now())
            .bind(task_id)
            .execute(self.w())
            .await?
            .rows_affected();
        self.publish_task_update(task_id, n).await
    }

    /// Forget the request a task was published as: a task retried after its
    /// request was closed unmerged would otherwise show the user a request
    /// nobody will merge.
    pub async fn clear_task_pull_request(&self, task_id: &str) -> Result<()> {
        let n = sqlx::query(
            "UPDATE tasks SET pr_url = NULL, updated_at = ?
             WHERE id = ? AND pr_url IS NOT NULL",
        )
        .bind(now())
        .bind(task_id)
        .execute(self.w())
        .await?
        .rows_affected();
        self.publish_task_update(task_id, n).await
    }

    /// Bring a task's stall into line with what its agents' own flags say.
    ///
    /// A stalled task *is* a task whose agent stopped working: one condition,
    /// decided by the session's attention. The task's column is this
    /// projection of it, written by every write that can change what a
    /// session's attention says and by nothing else, so the two cannot drift
    /// apart. An orchestrator's session has no task to project onto and
    /// carries its stall on its own row alone.
    pub(crate) async fn sync_task_stall(&self, session_id: &str) -> Result<()> {
        let task_id: Option<String> =
            sqlx::query_scalar("SELECT task_id FROM agent_sessions WHERE id = ?")
                .bind(session_id)
                .fetch_optional(self.r())
                .await?
                .flatten();
        let Some(task_id) = task_id else {
            return Ok(());
        };
        let stalled: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1 FROM agent_sessions WHERE task_id = ? AND attention_reason = ?
             )",
        )
        .bind(&task_id)
        .bind(AttentionReason::Stalled.as_str())
        .fetch_one(self.r())
        .await?;
        // Only a change is written, so that a task nobody's stall moved is
        // neither restamped nor announced to the watchers of its row.
        let n = sqlx::query(
            "UPDATE tasks SET stalled = ?, updated_at = ? WHERE id = ? AND stalled <> ?",
        )
        .bind(stalled as i64)
        .bind(now())
        .bind(&task_id)
        .bind(stalled as i64)
        .execute(self.w())
        .await?
        .rows_affected();
        self.publish_task_update(&task_id, n).await
    }

    /// Announce a non-transitional task write, unless it matched no row.
    pub(crate) async fn publish_task_update(
        &self,
        task_id: &str,
        rows_affected: u64,
    ) -> Result<()> {
        if rows_affected > 0 {
            let task = self.get_task(task_id).await?;
            self.publish(Change::TaskUpdated {
                task,
                transition: None,
            });
        }
        Ok(())
    }
}

/// A task takes one author or more, and nothing else about the staffing is
/// the store's to insist on.
///
/// Reviewers are not required on a one-author task. Most work is worth a
/// second pair of eyes, and the orchestrator is told so; some has nothing to
/// review — a release, a dependency bump the suite already judged — and a
/// task staffed with no reviewer is approved as soon as its author asks,
/// since there is nobody to ask (`scheduler::tasks`, `approvals_needed`).
///
/// A task staffed with several authors is the exception: the reviewers are
/// what picks the branch that lands, so it needs at least one.
fn check_staffing(agents: &[NewTaskAgent]) -> Result<()> {
    check_seat_counts(
        agents.iter().filter(|a| a.seat == Seat::Author).count(),
        agents.iter().filter(|a| a.seat == Seat::Reviewer).count(),
    )
}

/// The two staffing rules, on the counts alone: creation checks the list it
/// was given, and an edit checks what its replacements left behind.
fn check_seat_counts(authors: usize, reviewers: usize) -> Result<()> {
    if authors == 0 {
        return Err(StoreError::Invalid(
            "a task takes at least one author".into(),
        ));
    }
    if authors > 1 && reviewers == 0 {
        return Err(StoreError::Invalid(
            "a task with several authors needs a reviewer to pick the winner".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    const ID: &str = "01m0sktv47w6b8ze6xf4r9jr7c";

    #[test]
    fn branch_is_the_title_slugged_and_the_id_tail() {
        assert_eq!(
            branch_name("Fix the landing briefing: real fetch/rebase", ID),
            // 45 characters of slug is over the budget, and cutting at 40
            // would land inside `rebase`, so the whole word goes.
            "fix-the-landing-briefing-real-fetch-r9jr7c"
        );
        assert_eq!(
            branch_name("Add a health check", ID),
            "add-a-health-check-r9jr7c"
        );
    }

    #[test]
    fn branch_collapses_everything_that_is_not_a_letter_or_a_digit() {
        assert_eq!(
            branch_name(r#"Don't break "feat/x"... v1.2"#, ID),
            "don-t-break-feat-x-v1-2-r9jr7c"
        );
        assert_eq!(branch_name("  padded  ", ID), "padded-r9jr7c");
        assert_eq!(
            branch_name("-leading and trailing-", ID),
            "leading-and-trailing-r9jr7c"
        );
        assert_eq!(branch_name("Grüße, cafétería", ID), "gr-e-caf-ter-a-r9jr7c");
    }

    #[test]
    fn branch_falls_back_to_the_id_when_the_title_slugs_to_nothing() {
        for title in ["", "   ", "!!!", "修复登录", "—"] {
            assert_eq!(branch_name(title, ID), "task-r9jr7c", "title {title:?}");
        }
    }

    #[test]
    fn branch_clips_a_long_title_on_a_word_boundary() {
        // Exactly the budget: nothing to clip.
        let forty = "aaaa-bbbb-cccc-dddd-eeee-ffff-gggg-hhhhh";
        assert_eq!(forty.len(), SLUG_MAX);
        assert_eq!(slug(forty), forty);
        // Over the budget, but the character at it is a separator: the words
        // inside the budget are whole already, so none of them goes.
        assert_eq!(slug(&format!("{forty}-iiii")), forty);
        // A word straddling the budget goes entirely.
        assert_eq!(
            slug("aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii"),
            "aaaa-bbbb-cccc-dddd-eeee-ffff-gggg-hhhh"
        );
        // A single word with no boundary to back off to is cut where it falls.
        let long = "x".repeat(60);
        assert_eq!(slug(&long), "x".repeat(40));
        assert_eq!(branch_name(&long, ID), format!("{}-r9jr7c", "x".repeat(40)));
    }

    /// Whatever the title, git must take the name: the store hands it straight
    /// to `git worktree add -b`, and a rejected ref would fail the task.
    #[test]
    fn every_branch_name_is_a_valid_git_ref() {
        let titles = [
            "Fix the landing briefing: real fetch/rebase",
            "",
            "   ",
            "!!!",
            "修复登录",
            "-leading dash",
            "trailing dash-",
            "...dots... and .lock",
            "refs/heads/main",
            "a//b",
            "feature@{upstream}",
            "back\\slash and ~tilde^ and :colon",
            "question? star* bracket[",
            "line\nbreak\tand\ttabs",
            "@",
            "HEAD",
            &"x".repeat(200),
        ];
        for title in titles {
            let branch = branch_name(title, ID);
            let checked = Command::new("git")
                .args(["check-ref-format", "--branch", &branch])
                .output()
                .expect("git must be on PATH to check ref formats");
            assert!(
                checked.status.success(),
                "git rejected {branch:?} from title {title:?}"
            );
        }
    }
}
