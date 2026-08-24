//! Task repository: creation, dependency management, and the single
//! transactional entry point for status transitions.

use std::collections::{HashMap, HashSet};

use ariadne_core::id::new_id;
use ariadne_core::{Actor, TaskStatus, check_transition};

use crate::{
    Change, NewReview, Result, Store, StoreError, Task, TaskReviewer, TaskTransition, not_found,
    now,
};

#[derive(Debug, Clone)]
pub struct NewTask {
    pub goal_id: String,
    pub repo_id: String,
    pub title: String,
    pub description: String,
    pub engineer_profile_id: String,
    /// Profile that lands the task once it is approved.
    pub integrator_profile_id: String,
    pub reviewer_profile_ids: Vec<String>,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TaskUpdate {
    pub title: Option<String>,
    pub description: Option<String>,
    pub reviewer_profile_ids: Option<Vec<String>>,
    pub integrator_profile_id: Option<String>,
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
/// id — `fix-the-integrator-briefing-real-fetch-r9jr7c`. The branch is what
/// shows on the pull request the integrator opens, so it names the change and
/// nothing else: no prefix, no `ariadne` anywhere in it.
///
/// Only ASCII letters and digits survive; every run of anything else becomes a
/// single `-`, which keeps the result a valid git ref (`git check-ref-format
/// --branch`) whatever the title was. A title with nothing to slug — one with
/// no ASCII alphanumerics at all — falls back to `task-<tail>`.
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
    /// Create a task in `pending`. Enforces the goal's `max_tasks`, validates
    /// reviewers are non-empty and deps belong to the same goal and are acyclic.
    pub async fn create_task(&self, new: NewTask) -> Result<Task> {
        if new.reviewer_profile_ids.is_empty() {
            return Err(StoreError::Invalid(
                "a task needs at least one reviewer".into(),
            ));
        }
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

        if let Some(max) = goal.max_tasks {
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE goal_id = ?")
                .bind(&goal.id)
                .fetch_one(&mut *tx)
                .await?;
            if count >= max {
                return Err(StoreError::Conflict(format!(
                    "goal {} already has {count} of max {max} tasks",
                    goal.id
                )));
            }
        }

        // The engineer's agent and model are copied onto the task here and
        // never re-read: editing the profile later must not move a task that
        // is already defined, let alone one mid-flight.
        let engineer = Self::get_profile_in_tx(&mut tx, &new.engineer_profile_id).await?;

        sqlx::query(
            "INSERT INTO tasks (id, goal_id, repo_id, title, description, status,
                                engineer_profile_id, integrator_profile_id, agent_kind,
                                model, branch, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, 'pending', ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&goal.id)
        .bind(&repo.id)
        .bind(&new.title)
        .bind(&new.description)
        .bind(&new.engineer_profile_id)
        .bind(&new.integrator_profile_id)
        .bind(&engineer.agent_kind)
        .bind(&engineer.model)
        .bind(&branch)
        .bind(&ts)
        .bind(&ts)
        .execute(&mut *tx)
        .await?;

        Self::insert_reviewers(&mut tx, &id, &new.reviewer_profile_ids).await?;

        if !new.depends_on.is_empty() {
            Self::insert_dependencies(&mut tx, &goal.id, &id, &new.depends_on).await?;
        }

        tx.commit().await?;
        let task = self.get_task(&id).await?;
        self.publish(Change::TaskCreated(task.clone()));
        Ok(task)
    }

    /// Replace the dependency set of a task (planner, pre-start only).
    pub async fn set_task_dependencies(&self, task_id: &str, depends_on: &[String]) -> Result<()> {
        let mut tx = self.w().begin().await?;
        // Status is validated on the row inside the write transaction: a check
        // against the read pool could be stale by the time we hold the lock.
        let task = Self::get_task_in_tx(&mut tx, task_id).await?;
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
                    Actor::Planner,
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

    /// Validate deps exist, belong to `goal_id`, and introduce no cycle; insert them.
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
        sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?")
            .bind(id)
            .fetch_optional(self.r())
            .await?
            .ok_or_else(|| not_found("task", id))
    }

    pub async fn list_tasks(&self, filter: TaskFilter) -> Result<Vec<Task>> {
        let mut sql = String::from("SELECT * FROM tasks WHERE 1=1");
        if filter.goal_id.is_some() {
            sql.push_str(" AND goal_id = ?");
        }
        if filter.status.is_some() {
            sql.push_str(" AND status = ?");
        }
        sql.push_str(" ORDER BY id");
        // Safe: only fixed clause fragments are appended; values are bound.
        let mut q = sqlx::query_as::<_, Task>(sqlx::AssertSqlSafe(sql));
        if let Some(g) = &filter.goal_id {
            q = q.bind(g.clone());
        }
        if let Some(s) = filter.status {
            q = q.bind(s.as_str());
        }
        Ok(q.fetch_all(self.r()).await?)
    }

    pub async fn update_task(&self, id: &str, update: TaskUpdate) -> Result<Task> {
        let mut tx = self.w().begin().await?;
        // Status is validated on the row inside the write transaction: a check
        // against the read pool could be stale by the time we hold the lock.
        let task = Self::get_task_in_tx(&mut tx, id).await?;
        if !matches!(task.status(), TaskStatus::Pending | TaskStatus::Ready) {
            return Err(StoreError::Conflict(format!(
                "task can only be edited while pending/ready, it is {}",
                task.status
            )));
        }
        let title = update.title.unwrap_or(task.title);
        let description = update.description.unwrap_or(task.description);
        // The integrator is reassignable while the task has not started, the
        // way the reviewers below are: nothing of it is pinned onto the task,
        // so the swap is the id and nothing else.
        let integrator = update
            .integrator_profile_id
            .unwrap_or(task.integrator_profile_id);
        sqlx::query(
            "UPDATE tasks SET title = ?, description = ?, integrator_profile_id = ?,
                              updated_at = ?
             WHERE id = ?",
        )
        .bind(&title)
        .bind(&description)
        .bind(&integrator)
        .bind(now())
        .bind(id)
        .execute(&mut *tx)
        .await?;
        if let Some(reviewers) = update.reviewer_profile_ids {
            if reviewers.is_empty() {
                return Err(StoreError::Invalid(
                    "a task needs at least one reviewer".into(),
                ));
            }
            sqlx::query("DELETE FROM task_reviewers WHERE task_id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            // Reassigning reviewers writes new slots, so each one pins the
            // profile as it stands now, the same way creation does.
            Self::insert_reviewers(&mut tx, id, &reviewers).await?;
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
    /// (review round bump, merge commit, stalled reset) and writes the audit
    /// row — all in one transaction.
    pub async fn transition_task(
        &self,
        id: &str,
        to: TaskStatus,
        actor: Actor,
        reason: Option<&str>,
        merge_commit: Option<&str>,
    ) -> Result<Task> {
        let mut tx = self.w().begin().await?;
        let task = Self::get_task_in_tx(&mut tx, id).await?;
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

    /// Fetch a task row inside an open write transaction, so status checks see
    /// the state the transaction will actually commit against.
    async fn get_task_in_tx(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        id: &str,
    ) -> Result<Task> {
        sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(|| not_found("task", id))
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

        if to == TaskStatus::Merged && merge_commit.is_none() {
            return Err(StoreError::Invalid(
                "merged transition requires a merge commit".into(),
            ));
        }

        let review_round = if to == TaskStatus::UnderReview {
            task.review_round + 1
        } else {
            task.review_round
        };

        sqlx::query(
            "UPDATE tasks SET status = ?, review_round = ?, merge_commit = COALESCE(?, merge_commit),
                              stalled = 0, updated_at = ?
             WHERE id = ?",
        )
        .bind(to.as_str())
        .bind(review_round)
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

    /// The summary the engineer asked for review with, for the round that is
    /// open now: the reason of the most recent `under_review` transition.
    ///
    /// The round records it because the round is what it belongs to. Read off
    /// the conversation instead — the last thing an engineer happened to say —
    /// it would be whatever it wrote after asking, and what the reviewers and
    /// the people on a published request are handed has to be what it
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

    /// Reviewer profile ids in planner-assigned order.
    pub async fn list_task_reviewers(&self, task_id: &str) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT profile_id FROM task_reviewers WHERE task_id = ? ORDER BY position",
        )
        .bind(task_id)
        .fetch_all(self.r())
        .await?)
    }

    /// The reviewer slots themselves, in the same order, each carrying the
    /// agent and model it was pinned to when it was assigned.
    pub async fn list_task_reviewer_pins(&self, task_id: &str) -> Result<Vec<TaskReviewer>> {
        Ok(sqlx::query_as::<_, TaskReviewer>(
            "SELECT * FROM task_reviewers WHERE task_id = ? ORDER BY position",
        )
        .bind(task_id)
        .fetch_all(self.r())
        .await?)
    }

    /// Write one reviewer slot per profile, in the order given, pinning each
    /// profile's agent and model onto the slot. Read inside the transaction
    /// that writes the slots, so a profile edited in between cannot land
    /// half-applied across them.
    async fn insert_reviewers(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        task_id: &str,
        reviewer_profile_ids: &[String],
    ) -> Result<()> {
        for (position, profile_id) in reviewer_profile_ids.iter().enumerate() {
            let profile = Self::get_profile_in_tx(tx, profile_id).await?;
            sqlx::query(
                "INSERT INTO task_reviewers (task_id, profile_id, position, agent_kind, model)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(task_id)
            .bind(&profile.id)
            .bind(position as i64)
            .bind(&profile.agent_kind)
            .bind(&profile.model)
            .execute(&mut **tx)
            .await?;
        }
        Ok(())
    }

    pub async fn list_task_dependencies(&self, task_id: &str) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT depends_on_task_id FROM task_dependencies WHERE task_id = ? ORDER BY depends_on_task_id",
        )
        .bind(task_id)
        .fetch_all(self.r())
        .await?)
    }

    /// True when every dependency of the task is merged.
    pub async fn task_dependencies_merged(&self, task_id: &str) -> Result<bool> {
        let unmerged: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM task_dependencies td
             JOIN tasks dep ON dep.id = td.depends_on_task_id
             WHERE td.task_id = ? AND dep.status <> 'merged'",
        )
        .bind(task_id)
        .fetch_one(self.r())
        .await?;
        Ok(unmerged == 0)
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

    /// Record the pull request a task was published as, in full: the number
    /// the daemon polls with and the URL that says which forge it is on.
    ///
    /// Idempotent by construction — the integrator reports the pull request it
    /// opened, and re-reporting the same one writes the same row. Recording a
    /// *different* pull request resets the bookkeeping with it: the comments
    /// relayed and the approval announced belonged to the old one.
    pub async fn set_task_pull_request(&self, task_id: &str, number: i64, url: &str) -> Result<()> {
        let task = self.get_task(task_id).await?;
        let same = task.pr_url.as_deref() == Some(url);
        let n = sqlx::query(
            "UPDATE tasks
             SET pr_number = ?, pr_url = ?, pr_relayed_comments = ?,
                 pr_approved_notified = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(number)
        .bind(url)
        .bind(same.then(|| task.pr_relayed_comments.clone()).flatten())
        .bind(i64::from(same && task.pr_approved_notified()))
        .bind(now())
        .bind(task_id)
        .execute(self.w())
        .await?
        .rows_affected();
        self.publish_task_update(task_id, n).await
    }

    /// Forget the pull request a task was published as, and everything
    /// remembered about it.
    ///
    /// A published task is one with a request recorded on it: that is what
    /// makes the daemon poll a forge, and what makes its integrator push a
    /// revision to a request rather than open one. A task that is starting
    /// over — retried after the request it was published as was closed
    /// unmerged — is not that task any more, and leaving the record on it
    /// would send the next integrator to push at a request nobody will merge.
    ///
    /// The counterpart of [`Store::set_task_pull_request`]'s reset: recording
    /// a *different* request drops the same bookkeeping, for the same reason.
    pub async fn clear_task_pull_request(&self, task_id: &str) -> Result<()> {
        let n = sqlx::query(
            "UPDATE tasks
             SET pr_number = NULL, pr_url = NULL, pr_relayed_comments = NULL,
                 pr_approved_notified = 0, updated_at = ?
             WHERE id = ? AND (pr_number IS NOT NULL OR pr_url IS NOT NULL)",
        )
        .bind(now())
        .bind(task_id)
        .execute(self.w())
        .await?
        .rows_affected();
        self.publish_task_update(task_id, n).await
    }

    /// Mark what a poll of the pull request has handed to the engineer as
    /// relayed — a comment, a failing check, a conflict with the base —
    /// adding to whatever was relayed before: what keeps the daemon from
    /// relaying the same one twice as it polls.
    pub async fn add_task_pr_relayed_comments(&self, task_id: &str, ids: &[String]) -> Result<()> {
        let task = self.get_task(task_id).await?;
        let mut relayed = task.pr_relayed_comments();
        for id in ids {
            if !relayed.contains(id) {
                relayed.push(id.clone());
            }
        }
        let json = serde_json::to_string(&relayed).unwrap_or_else(|_| "[]".into());
        let n =
            sqlx::query("UPDATE tasks SET pr_relayed_comments = ?, updated_at = ? WHERE id = ?")
                .bind(&json)
                .bind(now())
                .bind(task_id)
                .execute(self.w())
                .await?
                .rows_affected();
        self.publish_task_update(task_id, n).await
    }

    /// Hand a round of what a published request said to the engineer — the
    /// comments on it, a branch that no longer merges, a check that failed —
    /// in one write or in none.
    ///
    /// Three records make one send-back: the review row the engineer is
    /// resumed with, the ids that keep any of it from being relayed into a
    /// second round, and the transition that wakes it. Written one after
    /// another, a daemon that failed halfway left the task in a state no
    /// later poll could repair — the ids marked relayed with no round
    /// carrying them, or a second round of the same failure — so they are one
    /// transaction, and a failure leaves the task exactly as the next poll
    /// expects to find it: still `integrating`, with nothing relayed.
    ///
    /// `relayed_ids` are added to whatever was relayed before, the way
    /// [`Store::add_task_pr_relayed_comments`] adds them; `reason` is the
    /// transition's own audit line. Returns the task as it now stands.
    pub async fn relay_pull_request_feedback(
        &self,
        review: NewReview,
        relayed_ids: &[String],
        reason: &str,
    ) -> Result<Task> {
        let mut tx = self.w().begin().await?;
        let task = Self::get_task_in_tx(&mut tx, &review.task_id).await?;
        let created = Self::insert_review_in_tx(&mut tx, &review).await?;

        let mut relayed = task.pr_relayed_comments();
        for id in relayed_ids {
            if !relayed.contains(id) {
                relayed.push(id.clone());
            }
        }
        let json = serde_json::to_string(&relayed).unwrap_or_else(|_| "[]".into());
        sqlx::query("UPDATE tasks SET pr_relayed_comments = ?, updated_at = ? WHERE id = ?")
            .bind(&json)
            .bind(now())
            .bind(&task.id)
            .execute(&mut *tx)
            .await?;

        let transition = Self::transition_in_tx(
            &mut tx,
            &task,
            TaskStatus::ChangesRequested,
            Actor::Daemon,
            Some(reason),
            None,
        )
        .await?;
        tx.commit().await?;

        let task = self.get_task(&review.task_id).await?;
        self.publish(Change::ReviewCreated(created));
        self.publish(Change::TaskUpdated {
            task: task.clone(),
            transition: Some(transition),
        });
        Ok(task)
    }

    /// Whether the user has been told this pull request is theirs: set when
    /// they are told, cleared when whatever made it theirs goes away again, so
    /// a second approval is announced and a poll that changes nothing is not.
    ///
    /// Two tellings set it, because they are the same news at different
    /// moments. One is the approval, announced as a poll reads it. The other
    /// is the request being opened at all — on a repository that gates
    /// nothing there is no approval coming, and the notice that goes out with
    /// the recorded URL is the whole of it. Where a review *is* required the
    /// first poll reads the request as unapproved and clears this again, and
    /// the approval is announced when it arrives.
    pub async fn set_task_pr_approved_notified(&self, task_id: &str, notified: bool) -> Result<()> {
        let n =
            sqlx::query("UPDATE tasks SET pr_approved_notified = ?, updated_at = ? WHERE id = ?")
                .bind(notified as i64)
                .bind(now())
                .bind(task_id)
                .execute(self.w())
                .await?
                .rows_affected();
        self.publish_task_update(task_id, n).await
    }

    pub async fn set_task_stalled(&self, task_id: &str, stalled: bool) -> Result<()> {
        let n = sqlx::query("UPDATE tasks SET stalled = ?, updated_at = ? WHERE id = ?")
            .bind(stalled as i64)
            .bind(now())
            .bind(task_id)
            .execute(self.w())
            .await?
            .rows_affected();
        self.publish_task_update(task_id, n).await
    }

    /// Announce a non-transitional task write, unless it matched no row.
    async fn publish_task_update(&self, task_id: &str, rows_affected: u64) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    const ID: &str = "01m0sktv47w6b8ze6xf4r9jr7c";

    #[test]
    fn branch_is_the_title_slugged_and_the_id_tail() {
        assert_eq!(
            branch_name("Fix the integrator briefing: real fetch/rebase", ID),
            // 45 characters of slug is over the budget, and cutting at 40
            // would land inside `rebase`, so the whole word goes.
            "fix-the-integrator-briefing-real-fetch-r9jr7c"
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
            "Fix the integrator briefing: real fetch/rebase",
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
