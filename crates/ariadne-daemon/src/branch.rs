//! Where a task's branch points, followed rather than asked for.
//!
//! A commit an engineer makes in its worktree is not a store write: nothing in
//! the database moves, so none of the domain events the bus pump fattens says
//! that the task's diff against its base is no longer the one a client
//! fetched. Asking `git rev-parse` for every live task on the scheduler tick
//! would put the whole interval between the commit and the client learning of
//! it, for a question almost always answered "no".
//!
//! So the ref itself is watched, which fires on the commit whichever agent CLI
//! made it, and a `git rev-parse` says what actually changed. A ref lives in
//! one of two places, and both are watched.
//!
//! The loose ref is watched through its *directory*, `refs/heads`, because git
//! never writes a ref in place: it creates `<ref>.lock` and renames it over,
//! so a watch on the file itself would be left on the inode that was replaced.
//!
//! `packed-refs` is watched as the file it is, and taken again after every
//! wake-up — a `git gc` replaces it by the same rename, which takes the watch
//! with it. That is belt and braces rather than a path git takes: the files
//! ref backend only rewrites `packed-refs` to pack or to prune, never to
//! record a new sha, so every value a branch takes is written loose into
//! `refs/heads` first. Which is also what makes re-arming on a wake-up enough
//! — packing prunes the loose refs it folds in, so the directory watch is what
//! reports the gc that the file watch then has to be taken back over.
//!
//! What is *not* watched is the git dir itself, though `packed-refs` sits in
//! it. On macOS `notify` re-watches whatever it finds in a watched directory,
//! recursively — and one `.git/objects` discovered that way is every loose
//! object in the repository held open, per task, for as long as the daemon
//! runs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use ariadne_api::stream::{DomainEvent, TaskBranchDto};
use ariadne_store::Task;

use crate::bus::{BusEvent, EventBus};
use crate::gitwt::GitManager;

/// How long a burst of filesystem events is held before the branch is
/// resolved.
///
/// One commit writes the ref, its lock file and the reflog, and `git gc`
/// rewrites `packed-refs` beside them. Resolving on each would ask git the
/// same question several times over and, where the answer moved between two of
/// them, publish the same head twice. Short enough that nobody watching a diff
/// tab can tell the difference.
const DEBOUNCE: Duration = Duration::from_millis(50);

/// The task branches the daemon is following, one watch per task.
///
/// Watches live only as long as the process holding them: a daemon that
/// restarts re-establishes them from the store (see
/// [`Launcher::watch_task_branches`](crate::launcher::Launcher::watch_task_branches)).
pub struct BranchWatchers {
    events: EventBus,
    git: GitManager,
    /// One watch per task, by task id. Removing the entry ends the watch:
    /// dropping it aborts the task that owns the `notify` watcher.
    watches: Mutex<HashMap<String, Watch>>,
}

/// One live watch. What it is on is kept beside it so that asking for the same
/// branch again is a no-op rather than a second watch on the same ref.
struct Watch {
    branch: String,
    handle: JoinHandle<()>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

impl BranchWatchers {
    pub fn new(events: EventBus) -> Self {
        Self {
            events,
            git: GitManager,
            watches: Mutex::new(HashMap::new()),
        }
    }

    /// Follow `task`'s branch in `repo`, from wherever it points now.
    ///
    /// Idempotent: a task already followed on the same branch keeps the watch
    /// it has, so an engineer respawned into the worktree it left does not
    /// stack a second one. Nothing is published for where the branch stands at
    /// this moment — only for where it moves next.
    pub fn watch(&self, task: &Task, repo: &Path) {
        let mut watches = self.lock();
        if watches.get(&task.id).is_some_and(|w| w.branch == task.branch) {
            return;
        }
        debug!(task = %task.id, branch = %task.branch, "following the task branch");
        let handle = tokio::spawn(follow(
            self.events.clone(),
            self.git.clone(),
            Followed {
                task_id: task.id.clone(),
                goal_id: task.goal_id.clone(),
                branch: task.branch.clone(),
                repo: repo.to_path_buf(),
            },
        ));
        // Assigning drops the watch this replaces, if any, which ends it.
        watches.insert(
            task.id.clone(),
            Watch {
                branch: task.branch.clone(),
                handle,
            },
        );
    }

    /// Stop following a task's branch: its worktree is gone, or the task is.
    pub fn unwatch(&self, task_id: &str) {
        if self.lock().remove(task_id).is_some() {
            debug!(task = %task_id, "no longer following the task branch");
        }
    }

    /// Whether a task's branch is being followed right now.
    pub fn is_watching(&self, task_id: &str) -> bool {
        self.lock().contains_key(task_id)
    }

    /// A watch registry is only ever read and written under this lock, and
    /// nothing held across it can panic — so a poisoned lock cannot happen,
    /// and treating it as fatal would take the daemon down over a map.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Watch>> {
        self.watches.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// What one watch is about.
struct Followed {
    task_id: String,
    goal_id: String,
    branch: String,
    /// The repository the branch lives in — the checkout registered with the
    /// daemon, not the engineer's worktree.
    repo: PathBuf,
}

/// Publish a `task_branch_updated` every time the branch head moves, until the
/// watch is dropped.
///
/// Published straight onto the bus rather than through a store change: nothing
/// was written, and there is nothing to write — where a branch points is git's
/// to know, and the event exists so a client can go and ask for the diff
/// again.
async fn follow(events: EventBus, git: GitManager, what: Followed) {
    let Followed {
        task_id,
        goal_id,
        branch,
        repo,
    } = what;
    let git_dir = match git.common_dir(&repo).await {
        Ok(dir) => dir,
        Err(e) => {
            warn!(task = %task_id, error = %e, "cannot find the refs of the task's repository");
            return;
        }
    };
    // One slot is all a wake-up needs: what follows one is a fresh `git
    // rev-parse`, so "something changed" does not accumulate.
    let (tx, mut rx) = mpsc::channel(1);
    // Held for as long as this task runs; dropping it ends the watch.
    let mut refs = Refs::arm(&git_dir, tx);

    // Where the branch stands as the watch begins. A client that has just
    // fetched the diff holds this one already, so it is the baseline and not
    // an event.
    let mut head = git.branch_tip(&repo, &branch).await.ok();
    while rx.recv().await.is_some() {
        // Let the rest of the burst land, and take it all as this wake-up.
        tokio::time::sleep(DEBOUNCE).await;
        while rx.try_recv().is_ok() {}
        // A `git gc` in that burst left a `packed-refs` the watch is no longer
        // on. Cheap enough to do every time: one `open(2)` when it is already
        // held.
        if let Some(refs) = &mut refs {
            refs.rearm_packed();
        }
        let Ok(tip) = git.branch_tip(&repo, &branch).await else {
            // Deleted, or git could not be asked. Either way there is nothing
            // to report, and the next event asks again.
            continue;
        };
        if head.as_deref() == Some(tip.as_str()) {
            // A ref rewritten with the sha it already had, a `git gc` packing
            // it, some other branch in the same directory: the file changed
            // and the answer did not.
            continue;
        }
        debug!(task = %task_id, branch = %branch, head = %tip, "the task branch moved");
        head = Some(tip.clone());
        events.publish(BusEvent {
            goal_id: Some(goal_id.clone()),
            task_id: Some(task_id.clone()),
            event: DomainEvent::TaskBranchUpdated(TaskBranchDto {
                task_id: task_id.clone(),
                goal_id: goal_id.clone(),
                branch: branch.clone(),
                head: tip,
            }),
        });
    }
}

/// The `notify` watch behind one followed branch: the two places a ref can be
/// read from.
struct Refs {
    watcher: RecommendedWatcher,
    /// `packed-refs`, kept so the watch on it can be taken again once git has
    /// replaced the file it was on.
    packed: PathBuf,
}

impl Refs {
    /// Watch `refs/heads`, the directory every loose ref is written into, and
    /// `packed-refs` beside it.
    ///
    /// Only the directory has to be there: a repository that has never been
    /// packed has no `packed-refs`, and the first one it gets is taken up by
    /// [`Self::rearm_packed`]. A `refs/heads` that cannot be watched is a
    /// warning and no watch at all — the branch goes unfollowed, which is
    /// where it was before there was a watch.
    fn arm(git_dir: &Path, tx: mpsc::Sender<()>) -> Option<Self> {
        let handler = move |event: notify::Result<notify::Event>| match event {
            // Unfiltered: every other branch of the repository is written into
            // the same directory, and matching paths would mean second-guessing
            // what a backend resolved through a symlink on the way back. A
            // wake-up that was not about this branch costs one `git rev-parse`.
            Ok(_) => {
                let _ = tx.try_send(());
            }
            // A watch that breaks is not retried: what it costs is the commits
            // between here and the next daemon, and the diff tab still has its
            // Refresh button.
            Err(e) => warn!(error = %e, "watching a task branch failed"),
        };
        let watcher = notify::recommended_watcher(handler)
            .inspect_err(|e| warn!(error = %e, "cannot watch task branches"))
            .ok()?;
        let mut refs = Self {
            watcher,
            packed: git_dir.join("packed-refs"),
        };
        let heads = git_dir.join("refs").join("heads");
        if let Err(e) = refs.watcher.watch(&heads, RecursiveMode::NonRecursive) {
            warn!(path = %heads.display(), error = %e, "cannot watch for branch updates");
            return None;
        }
        refs.rearm_packed();
        Some(refs)
    }

    /// Take the `packed-refs` watch again, for when git has replaced the file
    /// it was on — or has just written the first one.
    ///
    /// Silent on failure: there is nothing to watch until a `git gc` writes
    /// one, and until then every ref the repository has is loose.
    fn rearm_packed(&mut self) {
        let _ = self.watcher.watch(&self.packed, RecursiveMode::NonRecursive);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::process::Command;

    use tokio::sync::broadcast::Receiver;

    /// How long a commit is given to come back as an event. Generous: what is
    /// waited on is a filesystem notification, a debounce and two `git`
    /// processes.
    const PATIENCE: Duration = Duration::from_secs(10);

    fn sh(dir: &Path, cmd: &str) -> String {
        let out = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "command failed in {}: {cmd}\n{}",
            dir.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A repo with one commit on `work`, which is the branch every test here
    /// follows.
    fn repo(dir: &Path) -> PathBuf {
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        sh(
            &repo,
            "git init -q -b work && echo v1 > file.txt && git add . && \
             git -c user.email=t@t -c user.name=t commit -qm init",
        );
        repo
    }

    fn commit(repo: &Path, what: &str) -> String {
        sh(
            repo,
            &format!(
                "echo {what} > file.txt && git add . && \
                 git -c user.email=t@t -c user.name=t commit -qm {what} && git rev-parse HEAD"
            ),
        )
    }

    fn task(repo: &Path) -> Task {
        Task {
            id: "task-1".into(),
            goal_id: "goal-1".into(),
            repo_id: "repo-1".into(),
            title: "work".into(),
            description: String::new(),
            status: "in_progress".into(),
            engineer_profile_id: "profile-1".into(),
            agent_kind: None,
            model: None,
            effort: None,
            branch: "work".into(),
            worktree_path: Some(repo.display().to_string()),
            review_round: 0,
            stalled: 0,
            merge_commit: None,
            pr_url: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    /// The head the next `task_branch_updated` carries.
    async fn head_of(rx: &mut Receiver<BusEvent>) -> String {
        loop {
            let event = rx.recv().await.expect("bus closed").event;
            if let DomainEvent::TaskBranchUpdated(dto) = event {
                return dto.head;
            }
        }
    }

    /// The same, or a failed test.
    async fn next_head(rx: &mut Receiver<BusEvent>) -> String {
        tokio::time::timeout(PATIENCE, head_of(rx))
            .await
            .expect("no task_branch_updated within the patience")
    }

    /// Commit until the watch answers.
    ///
    /// A watch arms on the runtime rather than on the caller's thread, so a
    /// commit made the instant after [`BranchWatchers::watch`] returns can be
    /// the one its baseline picks up rather than the one it reports.
    /// Committing again is what tells "not armed yet" from "armed and quiet".
    async fn commit_until_seen(repo: &Path, rx: &mut Receiver<BusEvent>) {
        for n in 0..40 {
            let head = commit(repo, &format!("armed{n}"));
            if let Ok(seen) = tokio::time::timeout(Duration::from_millis(500), head_of(rx)).await {
                assert_eq!(seen, head, "the watch reported a head nobody committed");
                return;
            }
        }
        panic!("the branch watch never reported a commit");
    }

    /// The watch answers what git says, not what the filesystem said: a ref
    /// rewritten with the sha it already had is a write like any other, and
    /// there is nothing to tell a client about it.
    ///
    /// The commit at the end is what keeps the assertion from passing for the
    /// wrong reason — a watch that was never armed publishes nothing either.
    #[tokio::test]
    async fn a_ref_rewritten_with_the_same_sha_publishes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo(dir.path());
        let events = EventBus::new();
        let mut rx = events.subscribe();
        let watchers = BranchWatchers::new(events);
        watchers.watch(&task(&repo), &repo);

        // Packed as well as loose, so that both watched things are written in
        // over the course of the test.
        let head = sh(&repo, "git rev-parse HEAD");
        sh(&repo, "git pack-refs --all");
        // Repeated rather than once: the watch is armed asynchronously, and a
        // single write racing ahead of it would prove nothing.
        for _ in 0..10 {
            // The ref written by the lock-and-rename git itself uses, and
            // `packed-refs` touched beside it, with nothing new to say.
            sh(
                &repo,
                &format!(
                    "printf '%s\n' {head} > .git/refs/heads/work.lock && \
                     mv .git/refs/heads/work.lock .git/refs/heads/work && \
                     touch .git/packed-refs"
                ),
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert!(
                rx.try_recv().is_err(),
                "a ref rewritten with its own sha published an event"
            );
        }

        let committed = commit(&repo, "v2");
        assert_eq!(next_head(&mut rx).await, committed);
    }

    /// `packed-refs` is watched too, and the watch on it survives git
    /// replacing the file.
    ///
    /// Git itself never records a new sha there — a value always goes into a
    /// loose ref first — so a head that moves only in `packed-refs` has to be
    /// made by hand. Which is the point: nothing else proves the file is
    /// really being watched, rather than the commit having been reported by
    /// the `refs/heads` watch beside it.
    #[tokio::test]
    async fn a_head_that_moves_only_in_packed_refs_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo(dir.path());
        let events = EventBus::new();
        let mut rx = events.subscribe();
        let watchers = BranchWatchers::new(events);
        watchers.watch(&task(&repo), &repo);
        commit_until_seen(&repo, &mut rx).await;

        // A commit `work` is deliberately not moved to, and a `packed-refs`
        // holding `work` where it still is. The loose ref goes with the
        // packing, so from here the file is the only place the branch is
        // written down.
        let ahead = sh(
            &repo,
            "git checkout -q --detach && echo more > other.txt && git add . && \
             git -c user.email=t@t -c user.name=t commit -qm ahead && git rev-parse HEAD",
        );
        sh(&repo, "git pack-refs --all");
        assert!(!repo.join(".git/refs/heads/work").exists());
        // The packing is a wake-up on `refs/heads`, and taking the new
        // `packed-refs` under the watch is what that wake-up is for. Settled
        // for, rather than raced with: there is no way to ask a watch whether
        // it is armed, and the write below is the only one the test makes.
        tokio::time::sleep(Duration::from_secs(1)).await;

        sh(
            &repo,
            &format!(
                "printf '%s refs/heads/work\n' {ahead} > .git/packed-refs.new && \
                 mv .git/packed-refs.new .git/packed-refs"
            ),
        );
        assert_eq!(next_head(&mut rx).await, ahead);
    }

    /// A dropped watch is a stopped watch: the task is over, and what happens
    /// to its branch afterwards is nobody's business.
    #[tokio::test]
    async fn unwatching_stops_the_watch() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo(dir.path());
        let events = EventBus::new();
        let mut rx = events.subscribe();
        let watchers = BranchWatchers::new(events);
        let task = task(&repo);
        watchers.watch(&task, &repo);
        commit_until_seen(&repo, &mut rx).await;

        watchers.unwatch(&task.id);
        assert!(!watchers.is_watching(&task.id));

        commit(&repo, "v3");
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(
            rx.try_recv().is_err(),
            "a commit after the watch was dropped still published an event"
        );
    }
}
