//! What happens to a message that names somebody.
//!
//! Posting one writes it to the thread; this is the other half — the addressee
//! being told. An agent with a pane is nudged with the message, one whose
//! session ended is resumed with it as its instruction, and an addressee with
//! no session at all keeps its message in the thread, where its next briefing
//! sends it to read. A message for the human wakes nobody: it goes up the
//! attention path instead, on the session of the agent that wrote it.
//!
//! The whole path is exercised, from the HTTP handler both agents and the CLI
//! post through to the keystrokes that come out the other end: the router is
//! wired to a real scheduler, and `tmux` is a stub script whose sessions are
//! the ones a test lists as alive and which writes down every `send-keys` it
//! is handed — including the hexadecimal paste bodies, which is how "this
//! agent was told what the message said" is asserted.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use tower::ServiceExt;

use ariadne_api::SESSION_HEADER;
use ariadne_api::messages::MessageDto;
use ariadne_core::spawn_plan::SpawnPlanFile;
use ariadne_core::{
    Actor, AgentKind, AttentionReason, GoalStatus, Role, SessionStatus, TaskStatus,
};
use ariadne_daemon::config::Config;
use ariadne_daemon::gitwt::GitManager;
use ariadne_daemon::http::{self, AppState};
use ariadne_daemon::launcher::Launcher;
use ariadne_daemon::logbuf::LogBuffer;
use ariadne_daemon::scheduler::{self, SchedEvent};
use ariadne_daemon::tmux::{TmuxManager, session_name};
use ariadne_store::{
    AgentSession, Goal, NewGoal, NewProfile, NewRepository, NewSession, NewTask, Profile,
    SessionFilter, Store, Task,
};

/// How long a test waits for a delivery to come out of the scheduler. A
/// confirmed paste sleeps a second inside `send_submitted` before the pane is
/// read back, so this is not as generous as it looks.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Wait for what a delivery was supposed to do, rather than guessing at how
/// long one takes.
async fn eventually(what: &str, mut check: impl AsyncFnMut() -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if check().await {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

struct Harness {
    store: Store,
    router: Router,
    launcher: Arc<Launcher>,
    sched: tokio::sync::mpsc::UnboundedSender<SchedEvent>,
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
    let sched = scheduler::start(store.clone(), launcher.clone(), false);
    let state = AppState {
        store: store.clone(),
        started_at: Instant::now(),
        launcher: launcher.clone(),
        sched_tx: Some(sched.clone()),
        events: bus.clone(),
        logs: LogBuffer::new(),
    };
    Harness {
        store,
        router: http::router(state),
        launcher,
        sched,
        dir,
        _bus: bus,
    }
}

/// A `tmux` that has exactly the sessions a test wrote into `alive`, records
/// every `send-keys` it is handed — arguments and all, so the pasted bytes can
/// be read back — and whose panes draw whatever is in `composer` (nothing,
/// unless a test is about a message that stays in one).
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
        \x20 send-keys) echo \"$@\" >> \"$sent\" ;;\n\
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

/// The people of one goal with one task, and the sessions the tests wake.
struct Cast {
    goal: Goal,
    task: Task,
    planner: Profile,
    engineer: Profile,
    reviewer: Profile,
}

impl Harness {
    async fn profile(&self, name: &str, role: Role) -> Profile {
        self.store
            .create_profile(NewProfile {
                name: name.into(),
                role,
                agent_kind: Some(AgentKind::ClaudeCode),
                model: None,
                system_prompt: format!("You are {name}."),
                prompts: vec![],
            })
            .await
            .unwrap()
    }

    /// An active goal with one task on it, in progress: the shape every test
    /// here starts from.
    async fn cast(&self) -> Cast {
        let planner = self.profile("planner", Role::Planner).await;
        let engineer = self.profile("engineer", Role::Engineer).await;
        let reviewer = self.profile("reviewer", Role::Reviewer).await;
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
                planner_profile_id: planner.id.clone(),
                max_tasks: None,
                required_approvals: 1,
                repository_ids: vec![repo.id.clone()],
            })
            .await
            .unwrap();
        self.store
            .set_goal_status(&goal.id, GoalStatus::Active)
            .await
            .unwrap();
        let task = self
            .store
            .create_task(NewTask {
                goal_id: goal.id.clone(),
                repo_id: repo.id,
                title: "task".into(),
                description: "do things".into(),
                engineer_profile_id: engineer.id.clone(),
                integrator_profile_id: None,
                reviewer_profile_ids: vec![reviewer.id.clone()],
                depends_on: vec![],
            })
            .await
            .unwrap();
        for (status, actor) in [
            (TaskStatus::Ready, Actor::Daemon),
            (TaskStatus::InProgress, Actor::Daemon),
        ] {
            self.store
                .transition_task(&task.id, status, actor, None, None)
                .await
                .unwrap();
        }
        Cast {
            goal: self.store.get_goal(&goal.id).await.unwrap(),
            task: self.store.get_task(&task.id).await.unwrap(),
            planner,
            engineer,
            reviewer,
        }
    }

    /// A session for one of the cast, in a worktree that is really there.
    async fn session(
        &self,
        goal: &Goal,
        task: Option<&Task>,
        role: Role,
        profile: &Profile,
    ) -> AgentSession {
        let worktree = self.dir.path().join(format!("wt-{}", profile.name));
        std::fs::create_dir_all(&worktree).unwrap();
        self.store
            .create_session(NewSession {
                goal_id: goal.id.clone(),
                task_id: task.map(|t| t.id.clone()),
                role,
                profile_id: profile.id.clone(),
                agent_kind: AgentKind::ClaudeCode,
                model: None,
                tmux_session: session_name(
                    &goal.id,
                    task.map(|t| t.id.as_str()),
                    role.as_str(),
                    Some(&profile.id[profile.id.len() - 4..]),
                ),
                worktree_path: Some(worktree.display().to_string()),
                review_round: None,
            })
            .await
            .unwrap()
    }

    /// A session that has already run once and ended: the agent id a resume
    /// goes back to, and no pane left.
    async fn ended(&self, session: &AgentSession) -> AgentSession {
        self.store
            .set_session_internal_id(&session.id, "uuid-1234")
            .await
            .unwrap();
        self.store
            .set_session_status(&session.id, SessionStatus::Exited)
            .await
            .unwrap();
        self.store.get_session(&session.id).await.unwrap()
    }

    /// Tell the stub tmux this pane exists.
    fn pane_exists(&self, session: &AgentSession) {
        let alive = self.dir.path().join("alive");
        let mut names = std::fs::read_to_string(&alive).unwrap();
        names.push_str(&session.tmux_session);
        names.push('\n');
        std::fs::write(&alive, names).unwrap();
    }

    /// Post into a task's conversation, as `as_session` or (None) as the user.
    async fn post_to_task(
        &self,
        task: &Task,
        body: &str,
        to: Option<&str>,
        as_session: Option<&AgentSession>,
    ) -> MessageDto {
        self.post(
            &format!("/v1/tasks/{}/messages", task.id),
            body,
            to,
            as_session,
        )
        .await
    }

    /// Post into a goal's planning thread.
    async fn post_to_goal(
        &self,
        goal: &Goal,
        body: &str,
        to: Option<&str>,
        as_session: Option<&AgentSession>,
    ) -> MessageDto {
        self.post(
            &format!("/v1/goals/{}/messages", goal.id),
            body,
            to,
            as_session,
        )
        .await
    }

    async fn post(
        &self,
        path: &str,
        body: &str,
        to: Option<&str>,
        as_session: Option<&AgentSession>,
    ) -> MessageDto {
        let payload = serde_json::json!({ "body": body, "to": to });
        let mut request = Request::builder()
            .method(Method::POST)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(session) = as_session {
            request = request.header(SESSION_HEADER, &session.id);
        }
        let response = self
            .router
            .clone()
            .oneshot(request.body(Body::from(payload.to_string())).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED, "posting {path}");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// Everything pasted into this session's pane, as the agent would have
    /// read it: the stub logs the `send-keys -H` payload one hexadecimal byte
    /// per argument, which is how the bytes travel.
    fn pasted(&self, session: &AgentSession) -> String {
        let log =
            std::fs::read_to_string(self.dir.path().join("send-keys.log")).unwrap_or_default();
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

    /// How many `send-keys` this session's pane was handed, of any kind.
    fn keystrokes(&self, session: &AgentSession) -> usize {
        std::fs::read_to_string(self.dir.path().join("send-keys.log"))
            .unwrap_or_default()
            .lines()
            .filter(|l| l.split_whitespace().nth(2) == Some(session.tmux_session.as_str()))
            .count()
    }

    /// The argv of the last launch of `session_id`, where a resumed agent's
    /// instruction rides.
    fn resume_argv(&self, session_id: &str) -> String {
        let path = self
            .launcher
            .cfg
            .run_dir
            .join(session_id)
            .join("spawn.json");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        SpawnPlanFile::from_json(&raw).unwrap().argv.join(" ")
    }

    async fn attention(&self, session: &AgentSession) -> Option<AttentionReason> {
        self.store
            .get_session(&session.id)
            .await
            .unwrap()
            .attention_reason()
    }

    /// The bodies of a task thread, as anyone reading it would see them.
    async fn thread(&self, task: &Task) -> Vec<String> {
        self.store
            .list_task_messages(&task.id, None, 50)
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.body)
            .collect()
    }
}

/// The everyday case: an agent that is running is told what was said to it,
/// with the sender named and the message quoted, so it can act without going
/// to look the message up first.
#[tokio::test]
async fn an_addressed_agent_with_a_live_pane_is_nudged_with_the_message() {
    let h = harness().await;
    let cast = h.cast().await;
    let planner = h
        .session(&cast.goal, None, Role::Planner, &cast.planner)
        .await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer)
        .await;
    h.pane_exists(&engineer);

    h.post_to_task(
        &cast.task,
        "Skip the migration: the store already has the column.",
        Some("engineer"),
        Some(&planner),
    )
    .await;

    eventually("the engineer to be nudged", async || {
        h.pasted(&engineer)
            .contains("Skip the migration: the store already has the column.")
    })
    .await;
    let pasted = h.pasted(&engineer);
    assert!(
        pasted.contains("New message from the planner in your task conversation"),
        "the sender and the thread are named: {pasted}"
    );
    assert!(
        pasted.contains("`list_messages`"),
        "and the rest of the conversation is one call away: {pasted}"
    );
}

/// An agent whose session ended is not woken by typing into a pane that is no
/// longer there: it is resumed, with the message as the instruction it comes
/// back to.
#[tokio::test]
async fn an_addressed_agent_whose_session_ended_is_resumed_with_the_message() {
    let h = harness().await;
    let cast = h.cast().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer)
        .await;
    let engineer = h.ended(&engineer).await;

    h.post_to_task(
        &cast.task,
        "Rebase before you merge.",
        Some("engineer"),
        None,
    )
    .await;

    eventually("the engineer to be resumed", async || {
        h.store
            .get_session(&engineer.id)
            .await
            .unwrap()
            .status()
            .is_live()
    })
    .await;
    let argv = h.resume_argv(&engineer.id);
    assert!(
        argv.contains("--resume uuid-1234"),
        "the same conversation, not a fresh one: {argv}"
    );
    assert!(
        argv.contains("Rebase before you merge."),
        "and it comes back to the message: {argv}"
    );
    assert!(
        argv.contains("New message from the user in your task conversation"),
        "with its sender named: {argv}"
    );
}

/// A goal's planning thread addresses its planner, whose session is the goal's
/// own — the tasks under it have sessions too, and none of them is the one
/// meant here.
#[tokio::test]
async fn a_goal_thread_message_wakes_the_planner() {
    let h = harness().await;
    let cast = h.cast().await;
    let planner = h
        .session(&cast.goal, None, Role::Planner, &cast.planner)
        .await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer)
        .await;
    h.pane_exists(&planner);
    h.pane_exists(&engineer);

    h.post_to_goal(
        &cast.goal,
        "Split the last task in two.",
        Some("planner"),
        None,
    )
    .await;

    eventually("the planner to be nudged", async || {
        h.pasted(&planner).contains("Split the last task in two.")
    })
    .await;
    let pasted = h.pasted(&planner);
    assert!(
        pasted.contains("the goal's planning thread"),
        "the thread it was said in is named: {pasted}"
    );
    assert_eq!(
        h.keystrokes(&engineer),
        0,
        "and the goal's tasks are not woken by it"
    );
}

/// An addressee with no session at all — a reviewer between rounds — is no
/// error and no message lost: it stays in the thread, which is where every
/// agent's briefing sends it to read.
#[tokio::test]
async fn an_addressee_with_no_session_leaves_the_message_in_the_thread() {
    let h = harness().await;
    let cast = h.cast().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer)
        .await;
    h.pane_exists(&engineer);

    h.post_to_task(
        &cast.task,
        "Have a look at the error handling.",
        Some("reviewer"),
        None,
    )
    .await;
    // Posted after it and delivered: the queue is in order, so the reviewer's
    // message has been through the scheduler by the time this one lands.
    h.post_to_task(&cast.task, "Carry on.", Some("engineer"), None)
        .await;

    eventually("the engineer to be nudged", async || {
        h.pasted(&engineer).contains("Carry on.")
    })
    .await;
    assert!(
        h.store
            .list_sessions(SessionFilter::default())
            .await
            .unwrap()
            .iter()
            .all(|s| s.profile_id != cast.reviewer.id),
        "no reviewer was started for a message"
    );
    assert!(
        h.thread(&cast.task)
            .await
            .contains(&"Have a look at the error handling.".to_string()),
        "and the message is where it was left"
    );
    assert!(
        !h.pasted(&engineer)
            .contains("Have a look at the error handling."),
        "certainly not typed at somebody else"
    );
}

/// A message for the human is not an agent's to answer: nobody is woken, and
/// it goes up on the session of the agent that asked — which is the pane the
/// user replies in.
#[tokio::test]
async fn a_message_for_the_user_raises_its_author_and_wakes_no_agent() {
    let h = harness().await;
    let cast = h.cast().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer)
        .await;
    let planner = h
        .session(&cast.goal, None, Role::Planner, &cast.planner)
        .await;
    h.pane_exists(&engineer);
    h.pane_exists(&planner);

    h.post_to_task(
        &cast.task,
        "Which database should this write to?",
        Some("user"),
        Some(&engineer),
    )
    .await;

    eventually("the author to be raised for the user", async || {
        h.attention(&engineer).await == Some(AttentionReason::WaitingInput)
    })
    .await;
    assert_eq!(
        h.keystrokes(&engineer),
        0,
        "the author is not nudged with its own question"
    );
    assert_eq!(h.keystrokes(&planner), 0, "and no other agent is either");
}

/// An agent sitting on a permission dialog is left alone: the Enter behind a
/// paste would answer the dialog, which is the one decision the daemon must
/// not make on the user's behalf.
#[tokio::test]
async fn an_agent_waiting_on_a_dialog_is_not_typed_into() {
    let h = harness().await;
    let cast = h.cast().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer)
        .await;
    let reviewer = h
        .session(&cast.goal, Some(&cast.task), Role::Reviewer, &cast.reviewer)
        .await;
    h.pane_exists(&engineer);
    h.pane_exists(&reviewer);
    h.store
        .set_session_attention(&engineer.id, AttentionReason::WaitingPermission)
        .await
        .unwrap();

    h.post_to_task(
        &cast.task,
        "Use the other endpoint.",
        Some("engineer"),
        None,
    )
    .await;
    h.post_to_task(&cast.task, "Start on round two.", Some("reviewer"), None)
        .await;

    eventually("the reviewer to be nudged", async || {
        h.pasted(&reviewer).contains("Start on round two.")
    })
    .await;
    assert_eq!(
        h.keystrokes(&engineer),
        0,
        "nothing was typed at the agent holding a dialog"
    );
    assert_eq!(
        h.attention(&engineer).await,
        Some(AttentionReason::WaitingPermission),
        "and what it is waiting for is still what it says"
    );
}

/// The scheduler resweeps everything every tick, and a message it sees twice
/// must not be typed in twice — the agent would read the same thing said
/// again as something new.
#[tokio::test]
async fn a_message_is_delivered_once_however_often_the_scheduler_sees_it() {
    let h = harness().await;
    let cast = h.cast().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer)
        .await;
    h.pane_exists(&engineer);

    let msg = h
        .post_to_task(&cast.task, "Only once, please.", Some("engineer"), None)
        .await;
    // The resweep: the same message, offered to the scheduler again.
    for _ in 0..3 {
        h.sched
            .send(SchedEvent::MessagePosted(msg.id.clone()))
            .unwrap();
    }
    // Behind all of them in the same queue, so its arrival means they are done.
    h.post_to_task(&cast.task, "And that is all.", Some("engineer"), None)
        .await;

    eventually("the second message to arrive", async || {
        h.pasted(&engineer).contains("And that is all.")
    })
    .await;
    assert_eq!(
        h.pasted(&engineer).matches("Only once, please.").count(),
        1,
        "the first message was typed in exactly once"
    );
}

/// A message addressed to the thread rather than to anyone in it behaves as
/// every message did before recipients existed: it is written down, and
/// nobody is woken for it.
#[tokio::test]
async fn an_unaddressed_message_wakes_nobody() {
    let h = harness().await;
    let cast = h.cast().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer)
        .await;
    h.pane_exists(&engineer);

    h.post_to_task(&cast.task, "Noting this for the record.", None, None)
        .await;
    h.post_to_task(&cast.task, "Now this is for you.", Some("engineer"), None)
        .await;

    eventually("the addressed message to arrive", async || {
        h.pasted(&engineer).contains("Now this is for you.")
    })
    .await;
    assert!(
        !h.pasted(&engineer).contains("Noting this for the record."),
        "the unaddressed one was nobody's to be woken for"
    );
    assert_eq!(
        h.attention(&engineer).await,
        None,
        "and it raised nothing for the user"
    );
}
