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
    Actor, AgentKind, AttentionReason, GoalStatus, ReviewVerdict, Role, SessionStatus, TaskStatus,
};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_daemon::tmux::{TmuxManager, session_name};
use ariadne_store::{
    AgentSession, Goal, NewGoal, NewProfile, NewRepository, NewReview, NewSession, NewTask,
    SessionFilter, Store, Task,
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
    harness_with(Spawns::Work).await
}

/// A daemon that cannot start anything: `cli_bin` names no executable, so
/// every fresh session dies at the launch.
///
/// What a vanished pane leaves behind is only itself visible while nothing has
/// replaced it — a successful replacement is supposed to clear the flag — so
/// the tests about what the sweep concluded run where no replacement can
/// happen, and the one about the replacement runs where it can.
async fn cannot_spawn_harness() -> Harness {
    harness_with(Spawns::Fail).await
}

enum Spawns {
    Work,
    Fail,
}

async fn harness_with(spawns: Spawns) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    let bus = ariadne_daemon::bus::start(store.clone());
    let mut config = Config::load(Some(dir.path().join("home"))).unwrap();
    if let Spawns::Fail = spawns {
        config.cli_bin = dir.path().join("no-such-ariadne").display().to_string();
    }
    let cfg = Arc::new(config);
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
        self.planning_goal_needing(1).await
    }

    /// The same, for a goal that wants `approvals` of them: a round that one
    /// verdict does not close is where a reviewer sits with its work done.
    async fn planning_goal_needing(&self, approvals: i64) -> (Goal, String) {
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
                required_approvals: approvals,
                repository_ids: vec![repo.id.clone()],
            })
            .await
            .unwrap();
        (goal, planner)
    }

    /// An active goal with one task on it, plus the engineer and reviewer
    /// profile ids the task was created with.
    async fn active_goal_with_task(&self) -> (Goal, Task, String, String) {
        self.active_goal_with_task_needing(1).await
    }

    async fn active_goal_with_task_needing(&self, approvals: i64) -> (Goal, Task, String, String) {
        let (goal, _planner) = self.planning_goal_needing(approvals).await;
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
                model: None,
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
/// puts it back.
///
/// A planner, so that nothing but the sweep is under test: the goal's own
/// reconciliation cannot start a replacement here (the repository is not a
/// git repository) and would have nothing to say about attention if it could.
#[tokio::test]
async fn a_vanished_pane_with_work_still_active_is_flagged_disconnected() {
    let h = cannot_spawn_harness().await;
    let (goal, planner) = h.planning_goal().await;
    // Live in the database, gone as far as tmux is concerned: never added to
    // the stub's list of panes.
    let session = h.session(&goal, None, Role::Planner, &planner).await;

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
        .send(SchedEvent::GoalChanged(goal.id.clone()))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        h.attention(&session).await,
        Some(AttentionReason::Disconnected),
        "the flag outlives the session's own status"
    );
}

/// The engineer of an active task with no live session is resumed blind, and
/// when even that cannot get off the ground the session it tried to bring back
/// is the thing the user has to look at.
#[tokio::test]
async fn an_engineer_that_cannot_be_resumed_is_flagged_disconnected() {
    let h = cannot_spawn_harness().await;
    let (goal, task, engineer, _reviewer) = h.active_goal_with_task().await;
    h.advance(&task, TaskStatus::InProgress).await;
    // Ended, with no agent conversation to resume and no git repository to
    // spawn a fresh one in: the resume attempt cannot succeed.
    let session = h
        .session(&goal, Some(&task), Role::Engineer, &engineer)
        .await;
    h.store
        .set_session_status(&session.id, SessionStatus::Exited)
        .await
        .unwrap();

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::TaskChanged(task.id.clone()))
        .unwrap();
    eventually("the failed resume to be raised", async || {
        h.attention(&session).await == Some(AttentionReason::Disconnected)
    })
    .await;
}

/// A pane going away while somebody else has the work is not the user's
/// problem either: the engineer of a task under review is waiting on its
/// reviewers, and is woken by id when they answer.
#[tokio::test]
async fn a_vanished_engineer_pane_under_review_is_not_raised() {
    let h = cannot_spawn_harness().await;
    let (goal, task, engineer, _reviewer) = h.active_goal_with_task().await;
    h.advance(&task, TaskStatus::UnderReview).await;
    let session = h
        .session(&goal, Some(&task), Role::Engineer, &engineer)
        .await;

    let _sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    eventually("the vanished session to be retired", async || {
        h.store.get_session(&session.id).await.unwrap().status() == SessionStatus::Exited
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        h.attention(&session).await,
        None,
        "the round is the reviewers' to finish, not this agent's"
    );
}

/// And a reviewer that has already voted is finished, however long the round
/// it voted in runs on.
#[tokio::test]
async fn a_vanished_reviewer_pane_after_its_verdict_is_not_raised() {
    let h = cannot_spawn_harness().await;
    // Two approvals wanted, one given: the round stays open around a reviewer
    // that has nothing left to do.
    let (goal, task, _engineer, reviewer) = h.active_goal_with_task_needing(2).await;
    h.advance(&task, TaskStatus::UnderReview).await;
    // Entering review opens a round: the verdict belongs to that one.
    let task = h.store.get_task(&task.id).await.unwrap();
    let session = h
        .session(&goal, Some(&task), Role::Reviewer, &reviewer)
        .await;
    h.store
        .create_review(NewReview {
            task_id: task.id.clone(),
            round: task.review_round,
            reviewer_profile_id: reviewer.clone(),
            session_id: Some(session.id.clone()),
            verdict: ReviewVerdict::Approve,
            body: None,
        })
        .await
        .unwrap();

    let _sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    eventually("the vanished session to be retired", async || {
        h.store.get_session(&session.id).await.unwrap().status() == SessionStatus::Exited
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        h.store.get_task(&task.id).await.unwrap().status(),
        TaskStatus::UnderReview,
        "the round is still open, so the status is not what makes this quiet"
    );
    assert_eq!(
        h.attention(&session).await,
        None,
        "a reviewer that has voted is not an agent anybody is waiting on"
    );
}

/// A replacement is a recovery too: the session a fresh spawn supersedes stops
/// asking for the user, but only once the replacement is actually up.
#[tokio::test]
async fn a_superseded_session_drops_its_attention_when_the_replacement_starts() {
    let h = harness().await;
    let (goal, planner) = h.planning_goal().await;
    // The planner cwd has to exist for the spawn to get off the ground.
    std::fs::create_dir_all(h.dir.path().join("repo")).unwrap();
    let session = h.session(&goal, None, Role::Planner, &planner).await;
    h.store
        .set_session_status(&session.id, SessionStatus::Exited)
        .await
        .unwrap();
    h.store
        .set_session_attention(&session.id, AttentionReason::Disconnected)
        .await
        .unwrap();

    // Nothing live for the goal, so reconciliation starts a new planner.
    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::GoalChanged(goal.id.clone()))
        .unwrap();
    eventually("the replacement planner to be running", async || {
        h.store
            .list_sessions(SessionFilter {
                goal_id: Some(goal.id.clone()),
                live_only: true,
                ..Default::default()
            })
            .await
            .unwrap()
            .iter()
            .any(|s| s.id != session.id)
    })
    .await;
    eventually("the superseded session to be let go", async || {
        h.attention(&session).await.is_none()
    })
    .await;
}

/// A pane going away after the work is over is just a session ending.
#[tokio::test]
async fn a_vanished_pane_on_finished_work_is_not_raised() {
    let h = cannot_spawn_harness().await;
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

/// A flag raised by an agent event is only ever taken down by another one,
/// and a session sitting on a dialog reports nothing: the sweep is what lets
/// go of an engineer that was blocked on a permission prompt when its task
/// moved on to its reviewers.
#[tokio::test]
async fn a_blocked_engineer_is_let_go_once_its_task_goes_under_review() {
    let h = cannot_spawn_harness().await;
    let (goal, task, engineer, _reviewer) = h.active_goal_with_task().await;
    h.advance(&task, TaskStatus::InProgress).await;
    let session = h
        .session(&goal, Some(&task), Role::Engineer, &engineer)
        .await;
    h.pane_exists(&session);
    h.store
        .set_session_attention(&session.id, AttentionReason::WaitingPermission)
        .await
        .unwrap();

    // Whatever the prompt was about, the engineer got past it and sent the
    // task for review; nothing more will ever be reported on that session.
    h.store
        .transition_task(
            &task.id,
            TaskStatus::UnderReview,
            Actor::Engineer,
            None,
            None,
        )
        .await
        .unwrap();

    // The sweep runs on the tick, and the first tick is immediate.
    let _sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    eventually("the stale flag to be dropped", async || {
        h.attention(&session).await.is_none()
    })
    .await;
}

/// And only then: an agent the work is still waiting on keeps its flag, down
/// to the moment it went up — how long it has been stuck is the half of it
/// the user acts on.
#[tokio::test]
async fn attention_survives_the_sweep_while_the_work_is_still_owed() {
    let h = cannot_spawn_harness().await;
    let (goal, task, engineer, reviewer) = h.active_goal_with_task().await;
    h.advance(&task, TaskStatus::InProgress).await;
    let session = h
        .session(&goal, Some(&task), Role::Engineer, &engineer)
        .await;
    h.pane_exists(&session);
    h.store
        .set_session_attention(&session.id, AttentionReason::WaitingPermission)
        .await
        .unwrap();
    let raised_at = h
        .store
        .get_session(&session.id)
        .await
        .unwrap()
        .attention_since;

    // A second engineer, blocked in exactly the same way but on a task that
    // has gone to its reviewers: its flag coming down is what says the sweep
    // the first one went through is over.
    let handed_over = h
        .extra_task(&goal, &engineer, &reviewer, "under review")
        .await;
    h.advance(&handed_over, TaskStatus::UnderReview).await;
    let control = h
        .session(&goal, Some(&handed_over), Role::Engineer, &engineer)
        .await;
    h.pane_exists(&control);
    h.store
        .set_session_attention(&control.id, AttentionReason::WaitingPermission)
        .await
        .unwrap();

    let _sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    eventually("the finished engineer to be let go", async || {
        h.attention(&control).await.is_none()
    })
    .await;

    let kept = h.store.get_session(&session.id).await.unwrap();
    assert_eq!(
        kept.attention_reason(),
        Some(AttentionReason::WaitingPermission),
        "the work is still this agent's, so what it is waiting on stands"
    );
    assert_eq!(
        kept.attention_since, raised_at,
        "and how long it has been waiting is not reset under it"
    );
}
