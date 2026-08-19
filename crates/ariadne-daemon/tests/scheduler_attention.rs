//! What the scheduler notices about agents that stopped working.
//!
//! Every role can go quiet, so every role is watched: the planner of a goal
//! still being planned, the reviewers a round is waiting on, and the engineer
//! — which is the only one whose task carries a flag of its own next to the
//! session's. A pane that disappears while its work is still going says so
//! too, rather than ending quietly.
//!
//! No tmux and no agent CLI: `tmux` is a stub script whose sessions are the
//! ones a test lists as alive, and which writes down every `send-keys` it is
//! handed — which is how "this agent was nudged" is asserted. The idle clock
//! is moved by backdating `last_activity_at` in the database, since the store
//! only ever stamps it "now" and a stall is fifteen minutes away.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ariadne_core::{
    Actor, AgentKind, AttentionReason, GoalStatus, Role, SessionStatus, TaskStatus,
};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_daemon::tmux::{TmuxManager, session_name};
use ariadne_store::{
    AgentSession, Goal, NewGoal, NewProfile, NewRepository, NewSession, NewTask, Store, Task,
};

/// Idle long enough to be past both thresholds (nudge at 300s, flag at 900s).
const LONG_IDLE_SECS: i64 = 1_000;
/// How long a test waits for a reconciliation to reach the store.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Wait for what a reconciliation was supposed to do, rather than guessing at
/// how long a pass takes.
async fn eventually(what: &str, mut check: impl AsyncFnMut() -> bool) {
    let deadline = std::time::Instant::now() + TIMEOUT;
    loop {
        if check().await {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

struct Harness {
    store: Store,
    launcher: Arc<Launcher>,
    dir: tempfile::TempDir,
    _bus: ariadne_daemon::bus::EventBus,
}

async fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    let bus = ariadne_daemon::bus::start(store.clone());
    let cfg = Arc::new(Config::load(Some(dir.path().join("home"))).unwrap());
    let launcher = Arc::new(Launcher {
        cfg,
        store: store.clone(),
        tmux: write_tmux_stub(dir.path()),
        git: GitManager,
    });
    Harness {
        store,
        launcher,
        dir,
        _bus: bus,
    }
}

/// A `tmux` that has exactly the sessions a test wrote into `alive`, and that
/// records the `send-keys` it is asked for so nudges can be counted.
fn write_tmux_stub(dir: &Path) -> TmuxManager {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(dir.join("alive"), "").unwrap();
    let bin = dir.join("tmux-stub.sh");
    let script = format!(
        "#!/bin/sh\n\
         alive='{alive}'\n\
         sent='{sent}'\n\
         target=''\n\
         prev=''\n\
         for a in \"$@\"; do\n\
        \x20 if [ \"$prev\" = \"-t\" ]; then target=\"$a\"; fi\n\
        \x20 prev=\"$a\"\n\
         done\n\
         case \"$1\" in\n\
        \x20 has-session) grep -qx \"$target\" \"$alive\" || exit 1 ;;\n\
        \x20 display-message) grep -qx \"$target\" \"$alive\" || exit 1; echo '80x24 0,0' ;;\n\
        \x20 send-keys) echo \"$target\" >> \"$sent\" ;;\n\
         esac\n\
         exit 0\n",
        alive = dir.join("alive").display(),
        sent = dir.join("send-keys.log").display(),
    );
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    TmuxManager::new(bin.display().to_string())
}

impl Harness {
    async fn profile(&self, name: &str, role: Role) -> String {
        self.store
            .create_profile(NewProfile {
                name: name.into(),
                role,
                agent_kind: Some(AgentKind::ClaudeCode),
                model: None,
                system_prompt: "You work.".into(),
                prompts: vec![],
            })
            .await
            .unwrap()
            .id
    }

    /// A goal still in planning, with a repository behind it.
    async fn planning_goal(&self) -> (Goal, String) {
        let planner = self.profile("planner", Role::Planner).await;
        let repo = self
            .store
            .create_repository(NewRepository {
                path: self.dir.path().join("repo").display().to_string(),
                base_branch: "main".into(),
                description: None,
            })
            .await
            .unwrap();
        let goal = self
            .store
            .create_goal(NewGoal {
                title: "Ship the UI".into(),
                description: "desc".into(),
                planner_profile_id: planner.clone(),
                max_tasks: None,
                required_approvals: 1,
                repository_ids: vec![repo.id.clone()],
            })
            .await
            .unwrap();
        (goal, planner)
    }

    /// An active goal with one task on it, plus the engineer and reviewer
    /// profile ids the task was created with.
    async fn active_goal_with_task(&self) -> (Goal, Task, String, String) {
        let (goal, _planner) = self.planning_goal().await;
        self.store
            .set_goal_status(&goal.id, GoalStatus::Active)
            .await
            .unwrap();
        let repo = self.store.list_goal_repositories(&goal.id).await.unwrap()[0]
            .id
            .clone();
        let engineer = self.profile("engineer", Role::Engineer).await;
        let reviewer = self.profile("reviewer", Role::Reviewer).await;
        let task = self
            .store
            .create_task(NewTask {
                goal_id: goal.id.clone(),
                repo_id: repo,
                title: "task".into(),
                description: "do things".into(),
                engineer_profile_id: engineer.clone(),
                reviewer_profile_ids: vec![reviewer.clone()],
                depends_on: vec![],
            })
            .await
            .unwrap();
        let goal = self.store.get_goal(&goal.id).await.unwrap();
        (goal, task, engineer, reviewer)
    }

    async fn session(
        &self,
        goal: &Goal,
        task: Option<&Task>,
        role: Role,
        profile_id: &str,
    ) -> AgentSession {
        let tmux = session_name(
            &goal.id,
            task.map(|t| t.id.as_str()),
            role.as_str(),
            Some(&profile_id[profile_id.len() - 4..]),
        );
        self.store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: task.map(|t| t.id.clone()),
                role,
                profile_id: profile_id.to_string(),
                agent_kind: AgentKind::ClaudeCode,
                tmux_session: tmux,
                worktree_path: Some(self.dir.path().join("wt").display().to_string()),
                review_round: None,
            })
            .await
            .unwrap()
    }

    /// Another task on the same goal, with the same agents behind it.
    async fn extra_task(&self, goal: &Goal, engineer: &str, reviewer: &str, title: &str) -> Task {
        let repo = self.store.list_goal_repositories(&goal.id).await.unwrap()[0]
            .id
            .clone();
        self.store
            .create_task(NewTask {
                goal_id: goal.id.clone(),
                repo_id: repo,
                title: title.into(),
                description: "do things".into(),
                engineer_profile_id: engineer.to_string(),
                reviewer_profile_ids: vec![reviewer.to_string()],
                depends_on: vec![],
            })
            .await
            .unwrap()
    }

    /// Walk a fresh task up to the status the scheduler is being watched in.
    async fn advance(&self, task: &Task, to: TaskStatus) {
        for (status, actor) in [
            (TaskStatus::Ready, Actor::Daemon),
            (TaskStatus::InProgress, Actor::Daemon),
            (TaskStatus::UnderReview, Actor::Engineer),
        ] {
            self.store
                .transition_task(&task.id, status, actor, None, None)
                .await
                .unwrap();
            if status == to {
                return;
            }
        }
    }

    /// An agent that has been sitting there doing nothing for `secs`.
    async fn idle_for(&self, session: &AgentSession, secs: i64) {
        self.store
            .set_session_status(&session.id, SessionStatus::Idle)
            .await
            .unwrap();
        let when = (chrono::Utc::now() - chrono::Duration::seconds(secs))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let pool = sqlx::SqlitePool::connect(&format!(
            "sqlite://{}",
            self.dir.path().join("test.db").display()
        ))
        .await
        .unwrap();
        sqlx::query("UPDATE agent_sessions SET last_activity_at = ? WHERE id = ?")
            .bind(when)
            .bind(&session.id)
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }

    /// Tell the stub tmux this pane exists.
    fn pane_exists(&self, session: &AgentSession) {
        let alive = self.dir.path().join("alive");
        let mut names = std::fs::read_to_string(&alive).unwrap();
        names.push_str(&session.tmux_session);
        names.push('\n');
        std::fs::write(&alive, names).unwrap();
    }

    /// How many `send-keys` this session's pane was handed.
    fn keystrokes(&self, session: &AgentSession) -> usize {
        std::fs::read_to_string(self.dir.path().join("send-keys.log"))
            .unwrap_or_default()
            .lines()
            .filter(|l| *l == session.tmux_session)
            .count()
    }

    async fn attention(&self, session: &AgentSession) -> Option<AttentionReason> {
        self.store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason()
    }
}

/// A planner has no task to flag, so its own session is where a goal that
/// stopped being planned says so.
#[tokio::test]
async fn a_planner_idle_past_the_threshold_is_raised_on_its_session() {
    let h = harness().await;
    let (goal, planner) = h.planning_goal().await;
    let session = h.session(&goal, None, Role::Planner, &planner).await;
    h.pane_exists(&session);
    h.idle_for(&session, LONG_IDLE_SECS).await;

    // Two passes: the first spends the one nudge, the second escalates.
    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::GoalChanged(goal.id.clone()))
        .unwrap();
    eventually("the planner to be nudged", async || {
        h.keystrokes(&session) > 0
    })
    .await;
    sched
        .send(SchedEvent::GoalChanged(goal.id.clone()))
        .unwrap();
    eventually("the planner to be raised", async || {
        h.attention(&session).await == Some(AttentionReason::Stalled)
    })
    .await;
}

/// A reviewer the round is still waiting on is watched the same way.
#[tokio::test]
async fn a_reviewer_idle_past_the_threshold_is_raised_on_its_session() {
    let h = harness().await;
    let (goal, task, _engineer, reviewer) = h.active_goal_with_task().await;
    h.advance(&task, TaskStatus::UnderReview).await;
    let session = h
        .session(&goal, Some(&task), Role::Reviewer, &reviewer)
        .await;
    h.pane_exists(&session);
    h.idle_for(&session, LONG_IDLE_SECS).await;

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::TaskChanged(task.id.clone()))
        .unwrap();
    eventually("the reviewer to be nudged", async || {
        h.keystrokes(&session) > 0
    })
    .await;
    sched
        .send(SchedEvent::TaskChanged(task.id.clone()))
        .unwrap();
    eventually("the reviewer to be raised", async || {
        h.attention(&session).await == Some(AttentionReason::Stalled)
    })
    .await;
}

/// The engineer keeps its task-level flag, and now says it on its session too.
#[tokio::test]
async fn an_engineer_stall_flags_the_task_and_its_session() {
    let h = harness().await;
    let (goal, task, engineer, _reviewer) = h.active_goal_with_task().await;
    h.advance(&task, TaskStatus::InProgress).await;
    let session = h
        .session(&goal, Some(&task), Role::Engineer, &engineer)
        .await;
    h.pane_exists(&session);
    h.idle_for(&session, LONG_IDLE_SECS).await;

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::TaskChanged(task.id.clone()))
        .unwrap();
    eventually("the engineer to be nudged", async || {
        h.keystrokes(&session) > 0
    })
    .await;
    sched
        .send(SchedEvent::TaskChanged(task.id.clone()))
        .unwrap();
    eventually("the task to be flagged", async || {
        h.store.get_task(&task.id).await.unwrap().is_stalled()
    })
    .await;
    assert_eq!(
        h.attention(&session).await,
        Some(AttentionReason::Stalled),
        "and the session carries the reason as well"
    );
}

/// An agent waiting on a person is blocked, not stalled. Typing into it would
/// answer whatever it is waiting on — a permission prompt takes Enter for a
/// yes — so it is left alone, flag and all.
#[tokio::test]
async fn a_session_waiting_on_a_person_is_never_nudged() {
    let h = harness().await;
    let (goal, task, engineer, reviewer) = h.active_goal_with_task().await;
    h.advance(&task, TaskStatus::InProgress).await;
    let session = h
        .session(&goal, Some(&task), Role::Engineer, &engineer)
        .await;
    h.pane_exists(&session);
    h.idle_for(&session, LONG_IDLE_SECS).await;
    h.store
        .set_session_attention(&session.id, AttentionReason::WaitingPermission)
        .await
        .unwrap();
    // A second task, idle in exactly the same way but blocked on nothing: its
    // nudge is what says the pass the blocked one went through is over.
    let control_task = h.extra_task(&goal, &engineer, &reviewer, "control").await;
    h.advance(&control_task, TaskStatus::InProgress).await;
    let control = h
        .session(&goal, Some(&control_task), Role::Engineer, &engineer)
        .await;
    h.pane_exists(&control);
    h.idle_for(&control, LONG_IDLE_SECS).await;

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::TaskChanged(task.id.clone()))
        .unwrap();
    sched
        .send(SchedEvent::TaskChanged(control_task.id.clone()))
        .unwrap();
    eventually("the unblocked engineer to be nudged", async || {
        h.keystrokes(&control) > 0
    })
    .await;

    assert_eq!(
        h.keystrokes(&session),
        0,
        "no keystroke is sent into a pane that is asking the user something"
    );
    assert_eq!(
        h.attention(&session).await,
        Some(AttentionReason::WaitingPermission),
        "and the reason it is waiting is not overwritten with a stall"
    );
    assert!(
        !h.store.get_task(&task.id).await.unwrap().is_stalled(),
        "nor is the task escalated behind it"
    );
}

/// A pane that vanished while its work was still going is not a session that
/// finished: it is an agent the user has lost, and it says so until something
/// puts it back. (Here the blind resume cannot succeed — the repository is not
/// a git repository — which is exactly the case the flag is for.)
#[tokio::test]
async fn a_vanished_pane_with_work_still_active_is_flagged_disconnected() {
    let h = harness().await;
    let (goal, task, engineer, _reviewer) = h.active_goal_with_task().await;
    h.advance(&task, TaskStatus::InProgress).await;
    // Live in the database, gone as far as tmux is concerned: never added to
    // the stub's list of panes.
    let session = h
        .session(&goal, Some(&task), Role::Engineer, &engineer)
        .await;

    // The sweep runs on the tick, and the first tick is immediate.
    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    eventually("the vanished session to be swept", async || {
        h.attention(&session).await == Some(AttentionReason::Disconnected)
    })
    .await;
    assert_eq!(
        h.store.get_session(&session.id).await.unwrap().status(),
        SessionStatus::Exited,
        "the session is retired as well as raised"
    );

    // And it stays raised: a session that ended needing attention keeps the
    // reason until it is resumed or replaced.
    sched
        .send(SchedEvent::TaskChanged(task.id.clone()))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        h.attention(&session).await,
        Some(AttentionReason::Disconnected),
        "the flag outlives the session's own status"
    );
}

/// A pane going away after the work is over is just a session ending.
#[tokio::test]
async fn a_vanished_pane_on_finished_work_is_not_raised() {
    let h = harness().await;
    let (goal, task, engineer, _reviewer) = h.active_goal_with_task().await;
    let session = h
        .session(&goal, Some(&task), Role::Engineer, &engineer)
        .await;
    h.store
        .transition_task(&task.id, TaskStatus::Cancelled, Actor::User, None, None)
        .await
        .unwrap();

    let _sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    eventually("the vanished session to be retired", async || {
        h.store.get_session(&session.id).await.unwrap().status() == SessionStatus::Exited
    })
    .await;
    // Whatever else that pass had to say about it would have been said by now.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        h.attention(&session).await,
        None,
        "nothing is waiting on this agent, so nobody has to be told"
    );
}

/// Resuming an agent is the recovery: whatever it needed the user for goes
/// with the relaunch, so a session that came back drops off the attention
/// list.
#[tokio::test]
async fn resuming_a_session_clears_its_attention() {
    let h = harness().await;
    let (goal, task, engineer, _reviewer) = h.active_goal_with_task().await;
    let worktree = h.dir.path().join("wt");
    std::fs::create_dir_all(&worktree).unwrap();
    h.store
        .set_task_worktree(&task.id, Some(&worktree.display().to_string()))
        .await
        .unwrap();
    let session = h
        .session(&goal, Some(&task), Role::Engineer, &engineer)
        .await;
    h.store
        .set_session_internal_id(&session.id, "uuid-1234")
        .await
        .unwrap();
    h.store
        .set_session_status(&session.id, SessionStatus::Exited)
        .await
        .unwrap();
    h.store
        .set_session_attention(&session.id, AttentionReason::Disconnected)
        .await
        .unwrap();

    let resumed = h
        .launcher
        .resume_engineer(&task.id, "Continue where you left off.")
        .await
        .unwrap();

    assert_eq!(resumed.id, session.id, "the same session, put back on air");
    assert_eq!(
        resumed.attention_reason(),
        None,
        "an agent that is running again needs nobody"
    );
    assert_eq!(resumed.attention_since, None);
}
