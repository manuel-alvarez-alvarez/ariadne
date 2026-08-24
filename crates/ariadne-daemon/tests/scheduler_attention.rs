//! What the scheduler notices about agents that stopped working.
//!
//! Every role can go quiet, so every role is watched: the planner of a goal
//! still being planned, the reviewers a round is waiting on, and the engineer
//! — which is the only one whose task carries a flag of its own next to the
//! session's. A pane that disappears while its work is still going says so
//! too, rather than ending quietly, and an agent that never started its turn
//! at all — the instruction still sitting in its composer — is unstuck with a
//! keystroke before the user is told about it.
//!
//! None of it waits on the keystrokes themselves: typing into a pane settles
//! for a second or two, and a pass with three agents to nudge sends all three
//! at once rather than one after another. A message that reached an agent
//! counts for the same pass as the nudge would have — it says the same thing
//! and better — so nothing tells an agent to get on with what it was asked to
//! do a moment ago.
//!
//! No tmux and no agent CLI: `tmux` is a stub script whose sessions are the
//! ones a test lists as alive, and which writes down every `send-keys` it is
//! handed — which is how "this agent was nudged" is asserted. Both clocks are
//! moved by backdating the database column they are read from
//! (`last_activity_at` for an idle stall, `launched_at` for a turn that never
//! started), since the store only ever stamps them "now" and a threshold is
//! minutes away.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ariadne_core::{
    Actor, AgentKind, AttentionReason, AuthorRole, GoalStatus, ReviewVerdict, Role, SessionStatus,
    TaskStatus,
};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_daemon::tmux::{TmuxManager, session_name};
use ariadne_store::{
    AgentSession, Goal, Message, NewAgentEvent, NewGoal, NewMessage, NewProfile, NewRepository,
    NewReview, NewSession, NewTask, Recipient, ReviewAuthor, SessionFilter, Store, Task,
};

/// Idle long enough to be past both thresholds (nudge at 300s, flag at 900s).
const LONG_IDLE_SECS: i64 = 1_000;
/// Launched long enough ago to be past both of the other watchdog's
/// thresholds (Enter at 300s, flag at 900s) — the same clock, read from the
/// launch rather than from the last thing the agent did.
const LONG_SILENCE_SECS: i64 = 1_000;
/// The first of those thresholds, for a test that wants the nudge and not the
/// escalation behind it.
const STALL_NUDGE_SECS: i64 = 300;
/// How long a test waits for a reconciliation to reach the store. Generous
/// because some of what is waited on is not the daemon thinking: a nudge no
/// composer will let go of spends several seconds of widening backoff before
/// anybody hears about it, and every test here runs beside the others.
const TIMEOUT: Duration = Duration::from_secs(30);

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
/// records the `send-keys` it is asked for so nudges can be counted. Its panes
/// draw whatever a test wrote into `composer` — nothing, unless the test is
/// about a nudge that stays in one.
fn write_tmux_stub(dir: &Path) -> TmuxManager {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(dir.join("alive"), "").unwrap();
    let bin = dir.join("tmux-stub.sh");
    let script = format!(
        "#!/bin/sh\n\
         alive='{alive}'\n\
         sent='{sent}'\n\
         composer='{composer}'\n\
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
        \x20 capture-pane) cat \"$composer\" 2>/dev/null ;;\n\
         esac\n\
         exit 0\n",
        alive = dir.join("alive").display(),
        sent = dir.join("send-keys.log").display(),
        composer = dir.join("composer").display(),
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
                integrator_profile_id: ariadne_store::defaults::INTEGRATOR_ID.into(),
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
                integrator_profile_id: ariadne_store::defaults::INTEGRATOR_ID.into(),
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
        self.backdate("last_activity_at", session, secs).await;
    }

    /// An agent launched `secs` ago and running ever since — which, until it
    /// reports something a turn is made of, is exactly what an agent holding
    /// an unsubmitted instruction looks like from outside its pane.
    async fn launched_ago(&self, session: &AgentSession, secs: i64) {
        self.store
            .set_session_status(&session.id, SessionStatus::Running)
            .await
            .unwrap();
        self.backdate("launched_at", session, secs).await;
    }

    /// Move one of a session's clocks back, since the store only ever stamps
    /// them "now" and every threshold here is minutes away.
    async fn backdate(&self, column: &str, session: &AgentSession, secs: i64) {
        let when = (chrono::Utc::now() - chrono::Duration::seconds(secs))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let pool = sqlx::SqlitePool::connect(&format!(
            "sqlite://{}",
            self.dir.path().join("test.db").display()
        ))
        .await
        .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE agent_sessions SET {column} = ? WHERE id = ?"
        )))
        .bind(when)
        .bind(&session.id)
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
    }

    /// One event reported by an agent, the way its hook or plugin would.
    async fn reports(&self, session: &AgentSession, kind: &str) {
        self.store
            .create_event(NewAgentEvent {
                session_id: Some(session.id.clone()),
                task_id: session.task_id.clone(),
                agent_kind: Some(AgentKind::ClaudeCode),
                kind: kind.into(),
                payload: serde_json::json!({}),
            })
            .await
            .unwrap();
    }

    /// Write an attention flag straight into the database, the way a daemon
    /// that did not know better left one behind. It has to go around the
    /// store, which now refuses to raise a prompt on a session that has
    /// ended — which is why there are rows like this to heal at all.
    async fn stale_attention(&self, session: &AgentSession, reason: AttentionReason) {
        let pool = sqlx::SqlitePool::connect(&format!(
            "sqlite://{}",
            self.dir.path().join("test.db").display()
        ))
        .await
        .unwrap();
        sqlx::query(
            "UPDATE agent_sessions SET attention_reason = ?, attention_since = ? WHERE id = ?",
        )
        .bind(reason.as_str())
        .bind(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
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

    /// What every pane draws: a composer holding `text`, for good. A nudge
    /// pasted into it is still there after the Enter, however many are sent.
    fn composer_keeps(&self, text: &str) {
        std::fs::write(self.dir.path().join("composer"), format!("> {text}\n")).unwrap();
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

    /// What a task's thread said to the user, in the order it was said.
    async fn user_messages(&self, task: &Task) -> Vec<Message> {
        self.store
            .list_task_messages(&task.id, None, 100)
            .await
            .unwrap()
            .into_iter()
            .filter(|m| m.recipient() == Some(Recipient::User))
            .collect()
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

/// A nudge that does not leave the composer is not a nudge. The pane keeps
/// showing it however many Enters follow, so the session is raised for the
/// user rather than counted as told — the flag says the agent is not moving,
/// which is exactly what a message it never received leaves behind.
#[tokio::test]
async fn a_nudge_that_never_submits_raises_the_session() {
    let h = harness().await;
    let (goal, task, engineer, _reviewer) = h.active_goal_with_task().await;
    h.advance(&task, TaskStatus::InProgress).await;
    let session = h
        .session(&goal, Some(&task), Role::Engineer, &engineer)
        .await;
    h.pane_exists(&session);
    // Past the nudge threshold and nowhere near the flag one: the only route
    // to a raised session here is the delivery that could not be confirmed.
    h.idle_for(&session, STALL_NUDGE_SECS + 60).await;
    // The engineer's resume template, as its profile has it: the pane is
    // holding the very words the daemon is about to type into it.
    h.composer_keeps(r#"Pick "task" up again: your worktree is on"#);

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::TaskChanged(task.id.clone()))
        .unwrap();

    eventually("the session to be raised", async || {
        h.attention(&session).await == Some(AttentionReason::Stalled)
    })
    .await;
    assert!(
        h.keystrokes(&session) > 2,
        "the paste was followed by more than one Enter"
    );
    assert!(
        !h.store.get_task(&task.id).await.unwrap().is_stalled(),
        "the task is not escalated for it: this is about the message, not the work"
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

/// A resume whose instruction never left the composer: the agent is running,
/// has reported nothing at all, and would sit there for ever. One Enter — what
/// a human does on finding such a pane — and, if that did not start it either,
/// the user.
#[tokio::test]
async fn a_resume_that_never_starts_its_turn_gets_one_enter_and_then_the_flag() {
    let h = harness().await;
    let (goal, task, _engineer, reviewer) = h.active_goal_with_task().await;
    h.advance(&task, TaskStatus::UnderReview).await;
    let session = h
        .session(&goal, Some(&task), Role::Reviewer, &reviewer)
        .await;
    h.pane_exists(&session);
    h.launched_ago(&session, LONG_SILENCE_SECS).await;

    // Two passes, as with the idle stall: the first spends the keystroke, the
    // second escalates.
    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::TaskChanged(task.id.clone()))
        .unwrap();
    eventually("the stuck composer to be submitted", async || {
        h.keystrokes(&session) > 0
    })
    .await;
    sched
        .send(SchedEvent::TaskChanged(task.id.clone()))
        .unwrap();
    eventually("the agent that never started to be raised", async || {
        h.attention(&session).await == Some(AttentionReason::Stalled)
    })
    .await;
    assert_eq!(
        h.keystrokes(&session),
        1,
        "one Enter per launch, however many passes see the same silence"
    );
}

/// A lifecycle event is a TUI that came up, not a turn that started: codex
/// reports `session_start` before there is any conversation to speak of, so it
/// buys the agent nothing here.
#[tokio::test]
async fn a_lifecycle_event_after_the_launch_is_not_turn_activity() {
    let h = harness().await;
    let (goal, task, _engineer, reviewer) = h.active_goal_with_task().await;
    h.advance(&task, TaskStatus::UnderReview).await;
    let session = h
        .session(&goal, Some(&task), Role::Reviewer, &reviewer)
        .await;
    h.pane_exists(&session);
    h.launched_ago(&session, LONG_SILENCE_SECS).await;
    h.reports(&session, "session_start").await;

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::TaskChanged(task.id.clone()))
        .unwrap();
    eventually("the stuck composer to be submitted anyway", async || {
        h.keystrokes(&session) > 0
    })
    .await;
}

/// And an agent that did start its turn is none of this watchdog's business,
/// however quiet it goes afterwards: a turn can take a long time between tool
/// calls, and typing into one is how work gets interrupted.
#[tokio::test]
async fn a_launch_followed_by_turn_activity_is_left_alone() {
    let h = harness().await;
    let (goal, task, engineer, reviewer) = h.active_goal_with_task().await;
    h.advance(&task, TaskStatus::UnderReview).await;
    let session = h
        .session(&goal, Some(&task), Role::Reviewer, &reviewer)
        .await;
    h.pane_exists(&session);
    h.launched_ago(&session, LONG_SILENCE_SECS).await;
    h.reports(&session, "pre_tool_use").await;
    // A second reviewer, silent since its own launch: its Enter is what says
    // the pass the working one went through is over.
    let control_task = h.extra_task(&goal, &engineer, &reviewer, "control").await;
    h.advance(&control_task, TaskStatus::UnderReview).await;
    let control = h
        .session(&goal, Some(&control_task), Role::Reviewer, &reviewer)
        .await;
    h.pane_exists(&control);
    h.launched_ago(&control, LONG_SILENCE_SECS).await;

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::TaskChanged(task.id.clone()))
        .unwrap();
    sched
        .send(SchedEvent::TaskChanged(control_task.id.clone()))
        .unwrap();
    eventually("the silent reviewer to be submitted", async || {
        h.keystrokes(&control) > 0
    })
    .await;

    assert_eq!(
        h.keystrokes(&session),
        0,
        "nothing is typed into an agent that is working"
    );
    assert_eq!(
        h.attention(&session).await,
        None,
        "nor is a working agent raised for the user"
    );
}

/// A failed turn is a turn. OpenCode reports one as `session.error` and the
/// session stays running with the error raised for the user — which is not a
/// composer anybody has to submit, and not a reason the user is better off
/// hearing as a stall.
#[tokio::test]
async fn a_launch_followed_by_a_failed_turn_is_left_alone() {
    let h = harness().await;
    let (goal, task, engineer, reviewer) = h.active_goal_with_task().await;
    h.advance(&task, TaskStatus::UnderReview).await;
    // What the ingest leaves behind for a failed turn: the event, and the
    // error raised on a session that is still running.
    let errored = h
        .session(&goal, Some(&task), Role::Reviewer, &reviewer)
        .await;
    h.pane_exists(&errored);
    h.launched_ago(&errored, LONG_SILENCE_SECS).await;
    h.reports(&errored, "session.error").await;
    h.store
        .set_session_attention(&errored.id, AttentionReason::AgentError)
        .await
        .unwrap();
    // The same failure with nothing on the session to show for it — a flag
    // the sweep took down, say. The event alone still speaks for the turn.
    let event_only_task = h
        .extra_task(&goal, &engineer, &reviewer, "event only")
        .await;
    h.advance(&event_only_task, TaskStatus::UnderReview).await;
    let event_only = h
        .session(&goal, Some(&event_only_task), Role::Reviewer, &reviewer)
        .await;
    h.pane_exists(&event_only);
    h.launched_ago(&event_only, LONG_SILENCE_SECS).await;
    h.reports(&event_only, "session.error").await;
    // And a silent reviewer, whose Enter says the passes are over.
    let control_task = h.extra_task(&goal, &engineer, &reviewer, "control").await;
    h.advance(&control_task, TaskStatus::UnderReview).await;
    let control = h
        .session(&goal, Some(&control_task), Role::Reviewer, &reviewer)
        .await;
    h.pane_exists(&control);
    h.launched_ago(&control, LONG_SILENCE_SECS).await;

    // Two passes, which is what it would take to reach the flag.
    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    for _ in 0..2 {
        for id in [&task.id, &event_only_task.id, &control_task.id] {
            sched.send(SchedEvent::TaskChanged(id.clone())).unwrap();
        }
    }
    eventually("the silent reviewer to be submitted", async || {
        h.keystrokes(&control) > 0
    })
    .await;
    // Whatever those passes had to say about the other two would have been
    // said by now.
    tokio::time::sleep(Duration::from_millis(300)).await;

    for errored in [&errored, &event_only] {
        assert_eq!(
            h.keystrokes(errored),
            0,
            "nothing is typed into an agent whose turn failed"
        );
    }
    assert_eq!(
        h.attention(&errored).await,
        Some(AttentionReason::AgentError),
        "and what it reported is not overwritten with a stall"
    );
    assert_eq!(
        h.attention(&event_only).await,
        None,
        "nor is one raised where the event alone said the turn ran"
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
            author: ReviewAuthor::Profile(reviewer.clone()),
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

/// A prompt is a dialog on the agent's pane: nobody can answer one on a
/// session that has ended, so retiring a session takes what it was waiting on
/// with it. Every role, and every one of them with its work still owed —
/// which is exactly when nothing else would take the flag down.
#[tokio::test]
async fn a_prompt_flag_does_not_outlive_the_session_it_was_raised_on() {
    /// Flag a session, retire it, and say what it is left carrying.
    async fn retire_on(
        h: &Harness,
        session: &AgentSession,
        reason: AttentionReason,
    ) -> Option<AttentionReason> {
        h.store
            .set_session_attention(&session.id, reason)
            .await
            .unwrap();
        h.store
            .set_session_status(&session.id, SessionStatus::Exited)
            .await
            .unwrap();
        let ended = h.store.get_session(&session.id).await.unwrap();
        assert_eq!(ended.attention_since, None, "and the clock under it");
        ended.attention_reason()
    }

    let h = harness().await;
    // One goal, walked from planning to active, so every role is retired in
    // the state its own work is still going in.
    let (goal, planner) = h.planning_goal().await;
    let planner_session = h.session(&goal, None, Role::Planner, &planner).await;
    assert_eq!(
        retire_on(&h, &planner_session, AttentionReason::WaitingInput).await,
        None,
        "the goal is still being planned, and the planner is still waiting on nobody"
    );

    h.store
        .set_goal_status(&goal.id, GoalStatus::Active)
        .await
        .unwrap();
    let goal = h.store.get_goal(&goal.id).await.unwrap();
    let engineer = h.profile("engineer", Role::Engineer).await;
    let reviewer = h.profile("reviewer", Role::Reviewer).await;
    let task = h
        .extra_task(&goal, &engineer, &reviewer, "in progress")
        .await;
    h.advance(&task, TaskStatus::InProgress).await;
    let engineer_session = h
        .session(&goal, Some(&task), Role::Engineer, &engineer)
        .await;
    let review = h
        .extra_task(&goal, &engineer, &reviewer, "under review")
        .await;
    h.advance(&review, TaskStatus::UnderReview).await;
    let reviewer_session = h
        .session(&goal, Some(&review), Role::Reviewer, &reviewer)
        .await;

    assert_eq!(
        retire_on(&h, &engineer_session, AttentionReason::WaitingPermission).await,
        None,
        "nor is the engineer of a task still in progress"
    );
    assert_eq!(
        retire_on(&h, &reviewer_session, AttentionReason::WaitingPermission).await,
        None,
        "nor the reviewer of a round it has not voted in"
    );
}

/// A pane that vanishes while its agent was sitting on a prompt still ends as
/// a disconnect: what the user has to know is that the work lost its agent,
/// not what the agent happened to be asking when it went.
#[tokio::test]
async fn a_vanished_pane_on_a_prompt_ends_disconnected() {
    let h = cannot_spawn_harness().await;
    let (goal, planner) = h.planning_goal().await;
    // Live in the database, gone as far as tmux is concerned, and blocked on
    // a dialog that died with it.
    let session = h.session(&goal, None, Role::Planner, &planner).await;
    h.store
        .set_session_attention(&session.id, AttentionReason::WaitingPermission)
        .await
        .unwrap();

    let _sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    eventually("the vanished session to be swept", async || {
        h.store.get_session(&session.id).await.unwrap().status() == SessionStatus::Exited
    })
    .await;
    // Whatever else that pass had to say about it would have been said by now.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        h.attention(&session).await,
        Some(AttentionReason::Disconnected),
        "the goal is still being planned, so its planner going away is the news"
    );
}

/// Rows that were already stale when the daemon started are healed by the
/// first sweep — and only the ones that are nonsense: a session that ended
/// reporting an error, or having stalled, ended carrying something true.
#[tokio::test]
async fn a_stale_prompt_flag_from_before_the_daemon_started_is_swept_up() {
    let h = cannot_spawn_harness().await;
    let (goal, planner) = h.planning_goal().await;

    // Written the way an older daemon left them: ended, and still saying they
    // are waiting on somebody.
    let mut sessions = Vec::new();
    for reason in [
        AttentionReason::WaitingInput,
        AttentionReason::AgentError,
        AttentionReason::Stalled,
    ] {
        let session = h.session(&goal, None, Role::Planner, &planner).await;
        h.store
            .set_session_status(&session.id, SessionStatus::Exited)
            .await
            .unwrap();
        h.stale_attention(&session, reason).await;
        sessions.push(session);
    }

    // The sweep runs on the tick, and the first tick is immediate.
    let _sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    eventually("the stale prompt flag to be dropped", async || {
        h.attention(&sessions[0]).await.is_none()
    })
    .await;
    assert_eq!(
        h.attention(&sessions[1]).await,
        Some(AttentionReason::AgentError),
        "an error the agent reported before it died is still worth reading"
    );
    assert_eq!(
        h.attention(&sessions[2]).await,
        Some(AttentionReason::Stalled),
        "and so is the stall it ended in"
    );
}

/// A finished goal owns nothing live, and the scheduler keeps it that way on
/// every pass rather than only on the way in.
///
/// The kill that runs at the transition is a one-off: a `resume` landing just
/// after it — the UI's button on the planner of a goal that had completed
/// seconds earlier — puts an agent back under a goal with no work left, where
/// it sits for ever holding the machine awake. So the completed arm reconciles
/// like every other one.
#[tokio::test]
async fn a_session_that_outlived_its_completed_goal_is_killed() {
    let h = harness().await;
    let (goal, planner) = h.planning_goal().await;
    h.store
        .set_goal_status(&goal.id, GoalStatus::Completed)
        .await
        .unwrap();
    // Live under a goal that was already finished, which is what a revive
    // racing the completion leaves behind.
    let session = h.session(&goal, None, Role::Planner, &planner).await;
    h.pane_exists(&session);

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    sched
        .send(SchedEvent::GoalChanged(goal.id.clone()))
        .unwrap();
    eventually("the leftover planner to be killed", async || {
        !h.store
            .get_session(&session.id)
            .await
            .unwrap()
            .status()
            .is_live()
    })
    .await;
}

/// The same pass on a goal that has nothing live left does nothing at all: the
/// reconciliation is convergent, not a kill re-issued every tick at a session
/// that already ended.
#[tokio::test]
async fn a_completed_goal_with_nothing_live_is_left_alone() {
    let h = harness().await;
    let (goal, planner) = h.planning_goal().await;
    let session = h.session(&goal, None, Role::Planner, &planner).await;
    h.pane_exists(&session);
    h.store
        .set_session_status(&session.id, SessionStatus::Exited)
        .await
        .unwrap();
    h.store
        .set_goal_status(&goal.id, GoalStatus::Completed)
        .await
        .unwrap();

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    for _ in 0..3 {
        sched
            .send(SchedEvent::GoalChanged(goal.id.clone()))
            .unwrap();
    }
    // Nothing to wait for, so the assertion is made after a pass has surely
    // run: the sends above are ordered ahead of this one on the same channel.
    sched
        .send(SchedEvent::GoalChanged(goal.id.clone()))
        .unwrap();
    eventually("the passes to have run", async || {
        h.store.get_goal(&goal.id).await.unwrap().status() == GoalStatus::Completed
    })
    .await;
    assert_eq!(
        h.keystrokes(&session),
        0,
        "a finished session is not typed into"
    );
    assert_eq!(
        h.store.get_session(&session.id).await.unwrap().status(),
        SessionStatus::Exited
    );
}

/// Three agents to nudge in one pass, and the pass does not wait on any of
/// them. Every delivery settles a paste and an Enter before it can say
/// whether the composer let go — a second or two each — which the loop used
/// to spend one agent at a time while every other event queued behind it.
#[tokio::test]
async fn a_pass_with_three_agents_to_nudge_does_not_wait_on_the_keystrokes() {
    let h = harness().await;
    let (goal, task, engineer, reviewer) = h.active_goal_with_task().await;
    let second = h.extra_task(&goal, &engineer, &reviewer, "second").await;
    let third = h.extra_task(&goal, &engineer, &reviewer, "third").await;
    let mut sessions = Vec::new();
    for task in [&task, &second, &third] {
        h.advance(task, TaskStatus::InProgress).await;
        let session = h
            .session(&goal, Some(task), Role::Engineer, &engineer)
            .await;
        h.pane_exists(&session);
        h.idle_for(&session, STALL_NUDGE_SECS + 60).await;
        sessions.push(session);
    }

    // The scheduler's opening reconciliation is the pass: it sees all three.
    let _sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);

    // What is measured is the pass, not the machine it runs on: how long
    // after the first pane is typed into the last one is. A delivery settles
    // for a second before it can report anything, so three taken in turn put
    // seconds between the first and the last.
    let deadline = std::time::Instant::now() + TIMEOUT;
    let mut first: Option<std::time::Instant> = None;
    let spread = loop {
        let typed = sessions.iter().filter(|s| h.keystrokes(s) > 0).count();
        if typed > 0 && first.is_none() {
            first = Some(std::time::Instant::now());
        }
        if typed == sessions.len() {
            break first.unwrap().elapsed();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for all three panes to be typed into"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert!(
        spread < Duration::from_millis(900),
        "the three nudges went out together, not one after another: {spread:?}"
    );
}

/// A message that reached an agent is a nudge, and a better one: it says what
/// to do rather than asking why nothing is being done. So the pass that would
/// have nudged this session leaves it alone, and the escalation behind the
/// nudge does not happen either — the idle clock runs from the delivery.
#[tokio::test]
async fn a_delivered_message_stands_in_for_the_stall_nudge() {
    let h = harness().await;
    let (goal, task, engineer, reviewer) = h.active_goal_with_task().await;
    h.advance(&task, TaskStatus::InProgress).await;
    let session = h
        .session(&goal, Some(&task), Role::Engineer, &engineer)
        .await;
    h.pane_exists(&session);
    h.idle_for(&session, 5).await;
    // Another task's agent, idle long enough to be nudged in the same passes:
    // what says a pass really looked at both of them.
    let other = h.extra_task(&goal, &engineer, &reviewer, "second").await;
    h.advance(&other, TaskStatus::InProgress).await;
    let canary = h
        .session(&goal, Some(&other), Role::Engineer, &engineer)
        .await;
    h.pane_exists(&canary);
    h.idle_for(&canary, LONG_IDLE_SECS).await;

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    let message = h
        .store
        .create_message(NewMessage {
            goal_id: goal.id.clone(),
            task_id: Some(task.id.clone()),
            author_role: AuthorRole::User,
            author_session_id: None,
            recipient: Some(Recipient::Profile(engineer.clone())),
            body: "Use the other endpoint.".into(),
        })
        .await
        .unwrap();
    sched
        .send(SchedEvent::MessagePosted(message.id.clone()))
        .unwrap();
    eventually("the message to reach the pane", async || {
        h.keystrokes(&session) > 1
    })
    .await;
    // The delivery is confirmed a beat after the Enter, by reading the pane
    // back; nothing here means anything until the scheduler has been told.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let after_delivery = h.keystrokes(&session);

    // And now what the agent's own clock says: nothing since long before the
    // message. Two passes, which is a nudge and then the flag behind it.
    h.idle_for(&session, LONG_IDLE_SECS).await;
    for _ in 0..2 {
        sched
            .send(SchedEvent::TaskChanged(task.id.clone()))
            .unwrap();
        sched
            .send(SchedEvent::TaskChanged(other.id.clone()))
            .unwrap();
        eventually("the other agent to be nudged", async || {
            h.keystrokes(&canary) > 0
        })
        .await;
    }

    assert_eq!(
        h.keystrokes(&session),
        after_delivery,
        "nothing was typed at an agent that has just been told what to do"
    );
    assert_eq!(
        h.attention(&session).await,
        None,
        "and it was not raised for the user either"
    );
    assert!(
        !h.store.get_task(&task.id).await.unwrap().is_stalled(),
        "nor was its task"
    );
}

/// A task nothing could be started for is a task nobody is coming back to:
/// the retry budget runs out, and the user is told once, in the task's own
/// thread, what stopped it.
#[tokio::test]
async fn a_task_that_could_never_be_started_tells_the_user_it_failed() {
    let h = cannot_spawn_harness().await;
    let (_goal, task, _engineer, _reviewer) = h.active_goal_with_task().await;

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    eventually("the retry budget to run out", async || {
        sched
            .send(SchedEvent::TaskChanged(task.id.clone()))
            .unwrap();
        h.store.get_task(&task.id).await.unwrap().status() == TaskStatus::Failed
    })
    .await;

    // Said once, however many passes ask about a task that has already ended.
    for _ in 0..3 {
        sched
            .send(SchedEvent::TaskChanged(task.id.clone()))
            .unwrap();
    }
    eventually("the failure to reach the user", async || {
        h.user_messages(&task).await.len() == 1
    })
    .await;
    let told = h.user_messages(&task).await;
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0].author_role(), AuthorRole::System);
    assert!(told[0].body.contains(&task.title), "{}", told[0].body);
    assert!(
        told[0].body.contains("the agent could not be started"),
        "the notice does not say what stopped it: {}",
        told[0].body
    );
}

/// A goal the user cancelled takes its tasks with it, and every one of them
/// says so where it happened: a cancelled task is not a task that quietly
/// stopped.
#[tokio::test]
async fn a_cancelled_goal_tells_the_user_of_every_task_it_took_with_it() {
    let h = cannot_spawn_harness().await;
    let (goal, task, engineer, reviewer) = h.active_goal_with_task().await;
    let second = h
        .extra_task(&goal, &engineer, &reviewer, "the other one")
        .await;
    h.store
        .set_goal_status(&goal.id, GoalStatus::Cancelled)
        .await
        .unwrap();

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    for _ in 0..3 {
        sched
            .send(SchedEvent::GoalChanged(goal.id.clone()))
            .unwrap();
    }
    for task in [&task, &second] {
        eventually("the task to be cancelled and said so", async || {
            h.store.get_task(&task.id).await.unwrap().status() == TaskStatus::Cancelled
                && !h.user_messages(task).await.is_empty()
        })
        .await;
        let told = h.user_messages(task).await;
        assert_eq!(told.len(), 1, "{told:?}");
        assert!(told[0].body.contains(&task.title), "{}", told[0].body);
        assert!(told[0].body.contains("goal cancelled"), "{}", told[0].body);
    }
}

/// And a goal whose tasks all landed ends in its own thread rather than in
/// the killing of its planner.
#[tokio::test]
async fn a_completed_goal_says_so_in_its_thread() {
    let h = harness().await;
    let (goal, task, _engineer, _reviewer) = h.active_goal_with_task().await;
    for (status, actor) in [
        (TaskStatus::Ready, Actor::Daemon),
        (TaskStatus::InProgress, Actor::Daemon),
        (TaskStatus::UnderReview, Actor::Engineer),
        (TaskStatus::Approved, Actor::Daemon),
        (TaskStatus::Integrating, Actor::Daemon),
    ] {
        h.store
            .transition_task(&task.id, status, actor, None, None)
            .await
            .unwrap();
    }
    h.store
        .transition_task(
            &task.id,
            TaskStatus::Merged,
            Actor::Integrator,
            None,
            Some("cafe1234"),
        )
        .await
        .unwrap();

    let sched = scheduler::start(h.store.clone(), h.launcher.clone(), false);
    for _ in 0..3 {
        sched
            .send(SchedEvent::GoalChanged(goal.id.clone()))
            .unwrap();
    }
    eventually("the goal to be completed", async || {
        h.store.get_goal(&goal.id).await.unwrap().status() == GoalStatus::Completed
    })
    .await;
    let thread = h
        .store
        .list_goal_messages(&goal.id, None, 100)
        .await
        .unwrap();
    assert_eq!(thread.len(), 1, "{thread:?}");
    assert_eq!(thread[0].author_role(), AuthorRole::System);
    assert_eq!(thread[0].recipient(), None, "it wakes nobody");
    assert!(thread[0].body.contains(&goal.title), "{}", thread[0].body);
}
