//! Resuming an agent keeps its session row.
//!
//! A task bounced back by its reviewers is the same engineer, in the same
//! conversation, in the same worktree — so it stays one session however many
//! rounds it takes, rather than growing a sibling row per round.
//!
//! No tmux and no agent CLI needed: `tmux` is a stub script that records the
//! commands the launcher issues, which is also how the console-log wiring is
//! checked without a pane to pipe.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast::Receiver;

use ariadne_api::stream::DomainEvent;
use ariadne_core::{AgentKind, Role, SessionStatus};
use ariadne_daemon::bus::{BusEvent, EventBus};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::tmux::{TmuxManager, session_name};
use ariadne_store::{
    AgentSession, NewGoal, NewProfile, NewSession, NewTask, SessionFilter, Store, Task,
};

/// How long a test waits for an event before giving up.
const TIMEOUT: Duration = Duration::from_secs(5);

struct Harness {
    store: Store,
    bus: EventBus,
    launcher: Arc<Launcher>,
    dir: tempfile::TempDir,
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
        bus,
        launcher,
        dir,
    }
}

/// A `tmux` that has no sessions and records every command it is given, so a
/// test can read back what the launcher asked for.
fn write_tmux_stub(dir: &Path) -> TmuxManager {
    use std::os::unix::fs::PermissionsExt;

    let bin = dir.join("tmux-stub.sh");
    let script = format!(
        "#!/bin/sh\n\
         echo \"$@\" >> '{log}'\n\
         case \"$1\" in\n\
         \x20 has-session) exit 1 ;;\n\
         esac\n\
         exit 0\n",
        log = dir.join("tmux-commands.log").display(),
    );
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    TmuxManager::new(bin.display().to_string())
}

impl Harness {
    /// A task with an engineer session that has already run once: a worktree
    /// on disk and a tmux that is no longer alive.
    async fn task_with_engineer_session(&self) -> (Task, AgentSession) {
        let planner = self.profile("planner", Role::Planner).await;
        let engineer = self.profile("engineer", Role::Engineer).await;
        let reviewer = self.profile("reviewer", Role::Reviewer).await;
        let goal = self
            .store
            .create_goal(NewGoal {
                title: "Ship the UI".into(),
                description: "desc".into(),
                planner_profile_id: planner,
                max_tasks: None,
                required_approvals: 1,
                // Not a git repo: a fresh spawn cannot get off the ground
                // here, which is what the fallback test leans on.
                repos: vec![(
                    self.dir.path().join("repo").display().to_string(),
                    "main".into(),
                )],
            })
            .await
            .unwrap();
        let repo = self
            .store
            .list_goal_repos(&goal.id)
            .await
            .unwrap()
            .remove(0);
        let task = self
            .store
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

        let worktree = self.dir.path().join("wt-eng");
        std::fs::create_dir_all(&worktree).unwrap();
        self.store
            .set_task_worktree(&task.id, Some(&worktree.display().to_string()))
            .await
            .unwrap();
        let session = self
            .store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: Some(task.id.clone()),
                role: Role::Engineer,
                profile_id: engineer,
                agent_kind: AgentKind::ClaudeCode,
                tmux_session: session_name(&goal.id, Some(&task.id), "engineer", None),
                worktree_path: Some(worktree.display().to_string()),
                review_round: None,
            })
            .await
            .unwrap();
        (self.store.get_task(&task.id).await.unwrap(), session)
    }

    /// The same, with the agent id a first run would have reported: the
    /// conversation there is to resume.
    async fn task_with_resumable_engineer(&self) -> (Task, AgentSession) {
        let (task, session) = self.task_with_engineer_session().await;
        self.store
            .set_session_internal_id(&session.id, "uuid-1234")
            .await
            .unwrap();
        self.store
            .set_session_status(&session.id, SessionStatus::Exited)
            .await
            .unwrap();
        (task, self.store.get_session(&session.id).await.unwrap())
    }

    async fn profile(&self, name: &str, role: Role) -> String {
        self.store
            .create_profile(NewProfile {
                name: name.into(),
                role,
                agent_kind: Some(AgentKind::ClaudeCode),
                model: None,
                system_prompt: format!("You are {name}."),
                extra_flags: vec![],
            })
            .await
            .unwrap()
            .id
    }

    async fn sessions_of(&self, task: &Task) -> Vec<AgentSession> {
        self.store
            .list_sessions(SessionFilter {
                task_id: Some(task.id.clone()),
                ..Default::default()
            })
            .await
            .unwrap()
    }

    /// Every command the launcher gave the stub `tmux`, one per line.
    fn tmux_commands(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("tmux-commands.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn console_log(&self, session_id: &str) -> PathBuf {
        self.launcher
            .cfg
            .run_dir
            .join(session_id)
            .join("console.log")
    }
}

/// Wait for the first event matching `pred`, skipping unrelated ones.
async fn next_event(rx: &mut Receiver<BusEvent>, pred: impl Fn(&BusEvent) -> bool) -> BusEvent {
    tokio::time::timeout(TIMEOUT, async {
        loop {
            let event = rx.recv().await.expect("event bus closed");
            if pred(&event) {
                return event;
            }
        }
    })
    .await
    .expect("timed out waiting for a matching domain event")
}

/// The changes-requested bounce, twice over: the task panel's Sessions tab
/// must still list one engineer, live again, on the same conversation.
#[tokio::test]
async fn resuming_the_engineer_reuses_its_session_across_review_rounds() {
    let h = harness().await;
    let (task, first) = h.task_with_resumable_engineer().await;

    for round in 1..=2 {
        let resumed = h
            .launcher
            .resume_engineer(&task.id, &format!("Round {round}: please fix things."))
            .await
            .unwrap();
        assert_eq!(resumed.id, first.id, "round {round} reused the session");
        assert_eq!(resumed.status(), SessionStatus::Running);
        assert_eq!(resumed.ended_at, None, "the session is live again");
        assert_eq!(
            resumed.tmux_session, first.tmux_session,
            "and keeps its tmux name"
        );
        assert_eq!(
            resumed.internal_session_id.as_deref(),
            Some("uuid-1234"),
            "on the same agent conversation"
        );
        assert!(resumed.last_activity_at.is_some(), "and is stamped live");
        let sessions = h.sessions_of(&task).await;
        assert_eq!(
            sessions.len(),
            1,
            "round {round} left more than one engineer session: {sessions:?}"
        );
    }

    // Both relaunches resumed the stored conversation rather than starting one.
    let commands = h.tmux_commands();
    let resumes = commands
        .iter()
        .filter(|c| c.contains("--resume uuid-1234"))
        .count();
    assert_eq!(resumes, 2, "commands: {commands:?}");
}

/// The UI's caches are driven by domain events, and a reused row only ever
/// gets updates — so the relaunch has to announce itself as one.
#[tokio::test]
async fn a_relaunch_announces_the_session_as_updated() {
    let h = harness().await;
    let (task, first) = h.task_with_resumable_engineer().await;
    let mut rx = h.bus.subscribe();

    h.launcher
        .resume_engineer(&task.id, "fix things")
        .await
        .unwrap();

    let event = next_event(
        &mut rx,
        |e| matches!(&e.event, DomainEvent::SessionUpdated(s) if s.status.is_live()),
    )
    .await;
    let DomainEvent::SessionUpdated(session) = event.event else {
        unreachable!("filtered above")
    };
    assert_eq!(session.id, first.id);
    assert!(
        !rx.try_recv()
            .is_ok_and(|e| matches!(e.event, DomainEvent::SessionCreated(_))),
        "a relaunch creates nothing"
    );
}

/// Console-log continuity: with the id reused, both runs pipe into the one
/// file, and deliberately append to it — the log stays the whole transcript of
/// the one session, in the order the terminal produced it.
#[tokio::test]
async fn relaunches_append_to_the_same_console_log() {
    let h = harness().await;
    let (task, first) = h.task_with_resumable_engineer().await;

    h.launcher.resume_engineer(&task.id, "again").await.unwrap();
    h.launcher
        .resume_engineer(&task.id, "and again")
        .await
        .unwrap();

    let expected = format!("cat >> '{}'", h.console_log(&first.id).display());
    let commands = h.tmux_commands();
    let pipes: Vec<&String> = commands
        .iter()
        .filter(|c| c.starts_with("pipe-pane"))
        .collect();
    assert_eq!(pipes.len(), 2, "one pipe-pane per launch: {commands:?}");
    for pipe in pipes {
        assert!(
            pipe.contains(&expected),
            "a relaunch must append to the session's own console log: {pipe}"
        );
    }
}

/// Manual resume (the UI's button, `ariadne attach`): the caller gets the very
/// session it named back, live again, not a sibling to go and find.
#[tokio::test]
async fn reviving_a_session_revives_it_in_place() {
    let h = harness().await;
    let (task, first) = h.task_with_resumable_engineer().await;

    let revived = h.launcher.revive_session(&first.id, None).await.unwrap();
    assert_eq!(revived.id, first.id);
    assert_eq!(revived.status(), SessionStatus::Running);
    assert_eq!(revived.ended_at, None);
    assert_eq!(revived.worktree_path, first.worktree_path);
    assert_eq!(h.sessions_of(&task).await.len(), 1);
}

/// Nothing to resume from: an engineer session that never reported an agent id
/// is not a conversation, so it is left alone and a fresh spawn is what runs
/// (which fails here for want of a git repo — the point is the path taken).
#[tokio::test]
async fn a_session_without_an_agent_id_is_not_revived() {
    let h = harness().await;
    let (task, first) = h.task_with_engineer_session().await;
    h.store
        .set_session_status(&first.id, SessionStatus::Exited)
        .await
        .unwrap();

    assert!(
        h.launcher
            .resume_engineer(&task.id, "carry on")
            .await
            .is_err(),
        "there is no repo to spawn a fresh engineer in"
    );
    let after = h.store.get_session(&first.id).await.unwrap();
    assert_eq!(
        after.status(),
        SessionStatus::Exited,
        "an un-resumable session stays finished"
    );
    assert_eq!(h.sessions_of(&task).await.len(), 1);
}
