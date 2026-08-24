//! What reconciliation may conclude when `tmux` cannot be run at all.
//!
//! Nothing, is the answer. A daemon that cannot spawn a process has learned
//! neither that a pane is gone nor that it is there, and both of the decisions
//! reconciliation makes from that — retire the session, or start a second
//! agent beside it — are worse than waiting for the next tick. The liveness
//! sweep already leaves such rows alone; this pins the other half, since a
//! preserved row plus a "no live sessions" reading is exactly how one task
//! ends up with two engineers.
//!
//! The same goes for a message somebody was addressed with. An unanswerable
//! `has-session` is not an agent that has ended, so nothing is relaunched on
//! top of it and the message is not spent on the pass that could not be made:
//! it waits, and goes in the moment tmux answers again.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ariadne_core::{Actor, AgentKind, AuthorRole, GoalStatus, Role, TaskStatus};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_daemon::tmux::{TmuxManager, session_name};
use ariadne_store::{
    AgentSession, NewGoal, NewMessage, NewProfile, NewRepository, NewSession, NewTask, Recipient,
    SessionFilter, Store,
};

/// Where the `tmux` these tests run stands — or does not, until one of them
/// puts a working one there.
fn tmux_path(dir: &Path) -> std::path::PathBuf {
    dir.join("tmux-that-is-not-installed")
}

/// A `tmux` binary that is not there: every question comes back unanswered
/// rather than answered "no", which is what a machine briefly out of process
/// slots looks like from here.
fn unrunnable_tmux(dir: &Path) -> TmuxManager {
    TmuxManager::new(tmux_path(dir).display().to_string())
}

/// Put a working `tmux` where the unrunnable one was: its sessions are all
/// there, its panes draw nothing (so a delivery is confirmed on the first
/// Enter), and it writes down every `send-keys` it is handed.
fn tmux_comes_back(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let bin = tmux_path(dir);
    let script = format!(
        "#!/bin/sh\n\
         case \"$1\" in\n\
        \x20 display-message) echo '80x24 0,0' ;;\n\
        \x20 send-keys) echo \"$@\" >> '{sent}' ;;\n\
         esac\n\
         exit 0\n",
        sent = dir.join("send-keys.log").display(),
    );
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Everything pasted into a pane, as the agent would have read it: the stub
/// logs the `send-keys -H` payload one hexadecimal byte per argument, which is
/// how the bytes travel.
fn pasted(dir: &Path, session: &AgentSession) -> String {
    let log = std::fs::read_to_string(dir.join("send-keys.log")).unwrap_or_default();
    let mut bytes = Vec::new();
    for line in log.lines() {
        let args: Vec<&str> = line.split_whitespace().collect();
        let Some(hex) = args.iter().position(|a| *a == "-H") else {
            continue;
        };
        if args.get(2) != Some(&session.tmux_session.as_str()) {
            continue;
        }
        bytes.extend(
            args[hex + 1..]
                .iter()
                .filter_map(|a| u8::from_str_radix(a, 16).ok()),
        );
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Everything one of these tests works on: an active goal with a task on it,
/// an engineer already sitting in a pane, and the daemon that cannot ask
/// tmux about any of it.
struct World {
    store: Store,
    launcher: Arc<Launcher>,
    task: ariadne_store::Task,
    engineer: String,
    session: AgentSession,
    goal: String,
    _bus: ariadne_daemon::bus::EventBus,
}

/// The state both tests start from, up to but not including the transitions
/// each of them wants the task in.
async fn world(dir: &Path) -> World {
    let store = Store::open(dir.join("test.db")).await.unwrap();
    let bus = ariadne_daemon::bus::start(store.clone());
    let cfg = Arc::new(Config::load(Some(dir.join("home"))).unwrap());
    let launcher = Arc::new(Launcher {
        cfg,
        store: store.clone(),
        tmux: unrunnable_tmux(dir),
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

    let repo = store
        .create_repository(NewRepository {
            path: dir.join("repo").display().to_string(),
            base_branch: "main".into(),
            description: None,
        })
        .await
        .unwrap();
    let goal = store
        .create_goal(NewGoal {
            title: "Ship the UI".into(),
            description: "desc".into(),
            planner_profile_id: planner,
            max_tasks: None,
            required_approvals: 1,
            repository_ids: vec![repo.id.clone()],
        })
        .await
        .unwrap();
    // Planning is over: reconciliation only acts on an active goal.
    store
        .set_goal_status(&goal.id, GoalStatus::Active)
        .await
        .unwrap();
    let task = store
        .create_task(NewTask {
            goal_id: goal.id.clone(),
            repo_id: repo.id,
            title: "task".into(),
            description: "do things".into(),
            engineer_profile_id: engineer.clone(),
            integrator_profile_id: ariadne_store::defaults::INTEGRATOR_ID.into(),
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
            profile_id: engineer.clone(),
            agent_kind: AgentKind::ClaudeCode,
            model: None,
            tmux_session: session_name(&goal.id, Some(&task.id), "engineer", None),
            worktree_path: None,
            review_round: None,
        })
        .await
        .unwrap();
    World {
        store,
        launcher,
        task,
        engineer,
        session,
        goal: goal.id,
        _bus: bus,
    }
}

#[tokio::test]
async fn reconciliation_with_tmux_unavailable_neither_spawns_nor_fails_the_task() {
    let dir = tempfile::tempdir().unwrap();
    let World {
        store,
        launcher,
        task,
        session,
        ..
    } = world(dir.path()).await;
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

/// A message addressed to an agent whose pane nobody can ask about. The
/// daemon has learned nothing: not that the agent has gone (so it starts
/// nothing on top of it), and not that it is there (so the message is not
/// counted as delivered). The message waits, and the moment tmux answers
/// again it goes in.
#[tokio::test]
async fn a_message_for_an_unreachable_pane_waits_rather_than_relaunching_its_agent() {
    let dir = tempfile::tempdir().unwrap();
    let World {
        store,
        launcher,
        task,
        engineer,
        session,
        goal,
        ..
    } = world(dir.path()).await;
    for status in [TaskStatus::Ready, TaskStatus::InProgress] {
        store
            .transition_task(&task.id, status, Actor::Daemon, None, None)
            .await
            .unwrap();
    }

    // The scheduler first, and its opening reconciliation with it: what this
    // test counts is the passes made at one message, and a tick that came
    // round before the message existed makes none.
    let sched = scheduler::start(store.clone(), launcher.clone(), false);
    tokio::time::sleep(Duration::from_millis(150)).await;

    let message = store
        .create_message(NewMessage {
            goal_id: goal,
            task_id: Some(task.id.clone()),
            author_role: AuthorRole::User,
            author_session_id: None,
            recipient: Some(Recipient::Profile(engineer)),
            body: "Use the other endpoint.".into(),
        })
        .await
        .unwrap();
    // One pass at it while nothing can be asked, of the several it is worth.
    sched
        .send(SchedEvent::MessagePosted(message.id.clone()))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;

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
        "nothing was started for an agent that may well be sitting there: {sessions:#?}"
    );
    assert!(
        sessions[0].status().is_live(),
        "and it was not written off either: {:?}",
        sessions[0].status()
    );
    assert!(
        !launcher
            .cfg
            .run_dir
            .join(&session.id)
            .join("spawn.json")
            .exists(),
        "no relaunch was planned for it"
    );
    assert_eq!(
        store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason(),
        None,
        "and the user is not told about a message that still has passes left"
    );

    // tmux comes back, and with it the pane it could not answer for. The
    // reconciliation tick would offer the message again on its own; the test
    // asks for the same passes rather than waiting a quarter of a minute for
    // each of them.
    tmux_comes_back(dir.path());
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        sched
            .send(SchedEvent::MessagePosted(message.id.clone()))
            .unwrap();
        if pasted(dir.path(), &session).contains("Use the other endpoint.") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for the message to be delivered once tmux answered"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason(),
        None,
        "a message that got there in the end raises nothing"
    );
}
