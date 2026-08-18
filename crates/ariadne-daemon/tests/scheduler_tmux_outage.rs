//! What reconciliation may conclude when `tmux` cannot be run at all.
//!
//! Nothing, is the answer. A daemon that cannot spawn a process has learned
//! neither that a pane is gone nor that it is there, and both of the decisions
//! reconciliation makes from that — retire the session, or start a second
//! agent beside it — are worse than waiting for the next tick. The liveness
//! sweep already leaves such rows alone; this pins the other half, since a
//! preserved row plus a "no live sessions" reading is exactly how one task
//! ends up with two engineers.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ariadne_core::{Actor, AgentKind, GoalStatus, Role, TaskStatus};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_daemon::tmux::{TmuxManager, session_name};
use ariadne_store::{NewGoal, NewProfile, NewSession, NewTask, SessionFilter, Store};

/// A `tmux` binary that is not there: every question comes back unanswered
/// rather than answered "no", which is what a machine briefly out of process
/// slots looks like from here.
fn unrunnable_tmux(dir: &Path) -> TmuxManager {
    TmuxManager::new(dir.join("tmux-that-is-not-installed").display().to_string())
}

#[tokio::test]
async fn reconciliation_with_tmux_unavailable_neither_spawns_nor_fails_the_task() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("test.db")).await.unwrap();
    let bus = ariadne_daemon::bus::start(store.clone());
    let cfg = Arc::new(Config::load(Some(dir.path().join("home"))).unwrap());
    let launcher = Arc::new(Launcher {
        cfg,
        store: store.clone(),
        tmux: unrunnable_tmux(dir.path()),
        git: GitManager,
    });

    let profile = |name: &str, role: Role| {
        let store = store.clone();
        let name = name.to_string();
        async move {
            store
                .create_profile(NewProfile {
                    name,
                    role,
                    agent_kind: Some(AgentKind::ClaudeCode),
                    model: None,
                    system_prompt: "You work.".into(),
                    extra_flags: vec![],
                    prompts: vec![],
                })
                .await
                .unwrap()
                .id
        }
    };
    let planner = profile("planner", Role::Planner).await;
    let engineer = profile("engineer", Role::Engineer).await;
    let reviewer = profile("reviewer", Role::Reviewer).await;

    let goal = store
        .create_goal(NewGoal {
            title: "Ship the UI".into(),
            description: "desc".into(),
            planner_profile_id: planner,
            max_tasks: None,
            required_approvals: 1,
            repos: vec![(dir.path().join("repo").display().to_string(), "main".into())],
        })
        .await
        .unwrap();
    // Planning is over: reconciliation only acts on an active goal.
    store
        .set_goal_status(&goal.id, GoalStatus::Active)
        .await
        .unwrap();
    let repo = store.list_goal_repos(&goal.id).await.unwrap().remove(0);
    let task = store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id,
            title: "task".into(),
            description: "do things".into(),
            engineer_profile_id: engineer.clone(),
            reviewer_profile_ids: vec![reviewer],
            depends_on: vec![],
        })
        .await
        .unwrap();

    // An engineer is already on it, and its pane is one nobody can ask about.
    let session = store
        .create_session(NewSession {
            goal_id: goal.id.clone(),
            task_id: Some(task.id.clone()),
            role: Role::Engineer,
            profile_id: engineer,
            agent_kind: AgentKind::ClaudeCode,
            tmux_session: session_name(&goal.id, Some(&task.id), "engineer", None),
            worktree_path: None,
            review_round: None,
        })
        .await
        .unwrap();
    store
        .transition_task(&task.id, TaskStatus::Ready, Actor::Daemon, None, None)
        .await
        .unwrap();

    // More reconciliations than the spawn-retry budget allows for, so a task
    // failed by repeated attempts would have failed by the end of them.
    // No sleep inhibition: a test has no business touching power management.
    let sched = scheduler::start(store.clone(), launcher.clone(), false);
    for _ in 0..6 {
        sched
            .send(SchedEvent::TaskChanged(task.id.clone()))
            .unwrap();
        tokio::time::sleep(Duration::from_millis(120)).await;
    }
    drop(bus);

    let sessions = store
        .list_sessions(SessionFilter {
            task_id: Some(task.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        sessions.len(),
        1,
        "the session that may still be running keeps the task to itself: {sessions:#?}"
    );
    assert_eq!(sessions[0].id, session.id);
    assert!(
        sessions[0].status().is_live(),
        "a session is not retired because tmux could not be run: {:?}",
        sessions[0].status()
    );

    let task = store.get_task(&task.id).await.unwrap();
    assert_ne!(
        task.status(),
        TaskStatus::Failed,
        "an unreachable tmux is not the task's fault, and must not spend its retry budget"
    );
}
