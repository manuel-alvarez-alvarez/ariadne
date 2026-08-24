//! What happens to a message that names somebody.
//!
//! Posting one writes it to the thread; this is the other half — the addressee
//! being told. An agent with a pane is nudged with the message, one whose
//! session ended is resumed with it as its instruction, and an addressee with
//! no session at all keeps its message in the thread, where its next briefing
//! sends it to read. A message for the human wakes nobody: it goes up the
//! attention path instead, on the session of the agent that wrote it.
//!
//! The other half is what happens when the delivery does not work: a tmux
//! that will not take the keystrokes, an agent that cannot be resumed and an
//! addressee with no session to type into are tried again, and once the
//! passes are gone somebody is told — the addressee on its own session, or
//! the author on theirs when the addressee has no session at all. Nothing is
//! ever quietly struck off.
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

/// The same, for the one test that waits on a reconciliation tick rather than
/// on an event: nothing re-posts a message in production, so a retry is the
/// tick's to make and this is how long a tick can take to come round.
const TICK_TIMEOUT: Duration = Duration::from_secs(40);

/// How many passes one message is worth before the user is told it never
/// arrived, mirroring the scheduler's `DELIVERY_ATTEMPTS`.
const DELIVERY_ATTEMPTS: usize = 3;

/// Wait for what a delivery was supposed to do, rather than guessing at how
/// long one takes.
async fn eventually(what: &str, check: impl AsyncFnMut() -> bool) {
    eventually_within(TIMEOUT, what, check).await
}

async fn eventually_within(timeout: Duration, what: &str, mut check: impl AsyncFnMut() -> bool) {
    let deadline = Instant::now() + timeout;
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
///
/// While the `refusing` file is there it takes no keystrokes at all and notes
/// what it turned away, which is what a machine briefly out of process slots
/// looks like from the daemon's side.
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
        \x20 send-keys)\n\
        \x20   if [ -f '{refusing}' ]; then echo \"$target\" >> '{refused}'; exit 1; fi\n\
        \x20   echo \"$@\" >> \"$sent\" ;;\n\
        \x20 capture-pane) cat \"$composer\" 2>/dev/null ;;\n\
         esac\n\
         exit 0\n",
        alive = dir.join("alive").display(),
        sent = dir.join("send-keys.log").display(),
        composer = dir.join("composer").display(),
        refusing = dir.join("refusing").display(),
        refused = dir.join("refused.log").display(),
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
                integrator_profile_id: ariadne_store::defaults::INTEGRATOR_ID.into(),
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

    /// Stop the stub tmux taking any keystrokes, and start it again.
    fn tmux_refuses(&self) {
        std::fs::write(self.dir.path().join("refusing"), "").unwrap();
    }

    fn tmux_answers(&self) {
        std::fs::remove_file(self.dir.path().join("refusing")).unwrap();
    }

    /// How many deliveries the stub tmux turned away.
    fn refusals(&self) -> usize {
        std::fs::read_to_string(self.dir.path().join("refused.log"))
            .unwrap_or_default()
            .lines()
            .count()
    }

    /// Take a session's row out from under the daemon, the way deleting the
    /// goal it belonged to would. Straight SQL: nothing an agent can call
    /// does this, which is the point — it is the state the daemon has to cope
    /// with, not one it is asked to produce.
    async fn forget_session(&self, session: &AgentSession) {
        let pool = sqlx::SqlitePool::connect(&format!(
            "sqlite://{}",
            self.dir.path().join("test.db").display()
        ))
        .await
        .unwrap();
        sqlx::query("DELETE FROM agent_sessions WHERE id = ?")
            .bind(&session.id)
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
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
    /// instruction rides, or `None` while the launch has yet to write its
    /// plan: the session is marked live the moment the resume starts, a while
    /// before the spawn plan reaches the disk, so this is something to wait
    /// for rather than to read once.
    fn resume_argv(&self, session_id: &str) -> Option<String> {
        let path = self
            .launcher
            .cfg
            .run_dir
            .join(session_id)
            .join("spawn.json");
        let raw = std::fs::read_to_string(&path).ok()?;
        Some(SpawnPlanFile::from_json(&raw).unwrap().argv.join(" "))
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
        h.resume_argv(&engineer.id).is_some()
    })
    .await;
    let argv = h.resume_argv(&engineer.id).unwrap();
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
/// agent's briefing sends it to read, and nobody is started for it. What
/// happens if no session ever turns up for it is the next test's.
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

/// The planner takes part in every task thread, and its session is the goal's
/// own — the task it is being written to has no session of the planner's in
/// it, and looking for one there is how a message addressed to the planner
/// used to wake nobody at all.
#[tokio::test]
async fn a_task_thread_message_addressed_to_the_planner_wakes_it() {
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

    h.post_to_task(
        &cast.task,
        "This task needs a second repository.",
        Some("planner"),
        Some(&engineer),
    )
    .await;

    eventually("the planner to be nudged", async || {
        h.pasted(&planner)
            .contains("This task needs a second repository.")
    })
    .await;
    let pasted = h.pasted(&planner);
    assert!(
        pasted.contains("New message from the engineer in your task conversation"),
        "the sender and the thread are named: {pasted}"
    );
    assert_eq!(
        h.keystrokes(&engineer),
        0,
        "and the agent that wrote it is not woken with its own message"
    );
}

/// A tmux that would not take the keystrokes has said nothing about whether
/// the agent is there to hear them: the message is not struck off for it. The
/// reconciliation tick tries again — nothing re-posts a message in production,
/// so this is the only thing that would — and the agent gets it once, whole.
#[tokio::test]
async fn a_delivery_tmux_refused_is_tried_again_on_a_later_tick() {
    let h = harness().await;
    let cast = h.cast().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer)
        .await;
    h.pane_exists(&engineer);
    h.tmux_refuses();

    h.post_to_task(
        &cast.task,
        "The store already has that column.",
        Some("engineer"),
        None,
    )
    .await;
    eventually("the delivery to be turned away", async || h.refusals() > 0).await;
    assert_eq!(
        h.pasted(&engineer),
        "",
        "nothing reached the pane on the pass that failed"
    );

    h.tmux_answers();

    eventually_within(TICK_TIMEOUT, "the tick to try again", async || {
        h.pasted(&engineer)
            .contains("The store already has that column.")
    })
    .await;
    assert_eq!(
        h.pasted(&engineer)
            .matches("The store already has that column.")
            .count(),
        1,
        "and it arrives once, not once per attempt"
    );
    assert_eq!(
        h.attention(&engineer).await,
        None,
        "a delivery that got there raises nothing"
    );
}

/// The passes are not endless. An agent whose pane cannot be reached at all
/// ends as a flag on its own session: whatever it was told, it never heard it,
/// and only a person can do anything about that.
#[tokio::test]
async fn a_delivery_that_never_gets_through_raises_the_addressee() {
    let h = harness().await;
    let cast = h.cast().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer)
        .await;
    h.pane_exists(&engineer);
    h.tmux_refuses();

    let message = h
        .post_to_task(
            &cast.task,
            "Rebase before you merge.",
            Some("engineer"),
            None,
        )
        .await;

    // The passes the tick would make, asked for without waiting a quarter of
    // a minute for each: a message already in flight or already given up on
    // is nobody's to deliver again, so the extra offers cost nothing.
    eventually("the engineer to be raised", async || {
        h.sched
            .send(SchedEvent::MessagePosted(message.id.clone()))
            .unwrap();
        h.attention(&engineer).await == Some(AttentionReason::Stalled)
    })
    .await;
    assert!(
        h.refusals() >= DELIVERY_ATTEMPTS,
        "it was tried every pass it was worth, not given up on the first: {}",
        h.refusals()
    );
    assert!(
        h.thread(&cast.task)
            .await
            .contains(&"Rebase before you merge.".to_string()),
        "and the message is still in the thread for whoever comes to look"
    );
}

/// An addressee whose session ended is resumed with the message — and when
/// that resume cannot happen (here: the worktree it would come back in is
/// gone), the message has reached nobody. The session says so, with the
/// reason its pane has: there is none.
#[tokio::test]
async fn an_addressee_that_cannot_be_resumed_is_raised_for_the_user() {
    let h = harness().await;
    let cast = h.cast().await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer)
        .await;
    let engineer = h.ended(&engineer).await;
    std::fs::remove_dir_all(engineer.worktree_path.as_ref().unwrap()).unwrap();

    let message = h
        .post_to_task(
            &cast.task,
            "Have another look at the error handling.",
            Some("engineer"),
            None,
        )
        .await;

    eventually("the engineer to be raised", async || {
        h.sched
            .send(SchedEvent::MessagePosted(message.id.clone()))
            .unwrap();
        h.attention(&engineer).await == Some(AttentionReason::Disconnected)
    })
    .await;
    assert!(
        h.resume_argv(&engineer.id).is_none(),
        "nothing was launched: there was nowhere to launch it"
    );
}

/// The last resort. An addressee that had a session and no longer has one
/// leaves nothing to flag — so the flag goes where the answer was going to be
/// read: on the session of whoever asked, as the user's to deal with.
#[tokio::test]
async fn a_message_whose_addressee_lost_its_session_raises_its_author() {
    let h = harness().await;
    let cast = h.cast().await;
    let planner = h
        .session(&cast.goal, None, Role::Planner, &cast.planner)
        .await;
    let engineer = h
        .session(&cast.goal, Some(&cast.task), Role::Engineer, &cast.engineer)
        .await;
    h.pane_exists(&engineer);
    h.pane_exists(&planner);
    h.tmux_refuses();

    let message = h
        .post_to_task(
            &cast.task,
            "Which database should this write to?",
            Some("engineer"),
            Some(&planner),
        )
        .await;
    eventually("the delivery to be turned away", async || h.refusals() > 0).await;
    // And now the addressee is gone, the way a deleted goal takes its
    // sessions with it, with the message still owed.
    h.forget_session(&engineer).await;

    eventually("the author to be raised for the user", async || {
        h.sched
            .send(SchedEvent::MessagePosted(message.id.clone()))
            .unwrap();
        h.attention(&planner).await == Some(AttentionReason::WaitingInput)
    })
    .await;
}

/// An addressee that never gets a session is the same story as one that lost
/// it. The message keeps its place in the thread, and the passes are not
/// endless: when they run out with nobody there to be woken, the agent that
/// asked is raised for the user, since it is the one waiting on an answer
/// that is not coming.
#[tokio::test]
async fn a_message_for_an_addressee_that_never_gets_a_session_raises_its_author() {
    let h = harness().await;
    let cast = h.cast().await;
    let planner = h
        .session(&cast.goal, None, Role::Planner, &cast.planner)
        .await;
    h.pane_exists(&planner);

    // The reviewer has no session at all: this round has not started one.
    let message = h
        .post_to_task(
            &cast.task,
            "Have a look at the error handling once you pick this up.",
            Some("reviewer"),
            Some(&planner),
        )
        .await;

    // The passes the tick would make, asked for without waiting a quarter of
    // a minute for each.
    eventually("the author to be raised for the user", async || {
        h.sched
            .send(SchedEvent::MessagePosted(message.id.clone()))
            .unwrap();
        h.attention(&planner).await == Some(AttentionReason::WaitingInput)
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
    assert_eq!(
        h.keystrokes(&planner),
        0,
        "and nothing was typed at the agent that wrote it"
    );
    assert!(
        h.thread(&cast.task)
            .await
            .contains(&"Have a look at the error handling once you pick this up.".to_string()),
        "the message is still there for whoever comes to read it"
    );
}

/// The goal-level fallback is the planner's alone. Profiles are reusable, so
/// another role can have a session with no task on it; a task thread's
/// message is not that conversation's, and is not typed into it.
#[tokio::test]
async fn a_task_message_is_not_typed_at_another_role_working_outside_the_task() {
    let h = harness().await;
    let cast = h.cast().await;
    let planner = h
        .session(&cast.goal, None, Role::Planner, &cast.planner)
        .await;
    // An engineer session of the goal's rather than of the task's — not where
    // this task's engineer works, whatever profile it was started with.
    let elsewhere = h
        .session(&cast.goal, None, Role::Engineer, &cast.engineer)
        .await;
    h.pane_exists(&planner);
    h.pane_exists(&elsewhere);

    let message = h
        .post_to_task(
            &cast.task,
            "Skip the migration.",
            Some("engineer"),
            Some(&planner),
        )
        .await;

    eventually("the author to be raised for the user", async || {
        h.sched
            .send(SchedEvent::MessagePosted(message.id.clone()))
            .unwrap();
        h.attention(&planner).await == Some(AttentionReason::WaitingInput)
    })
    .await;
    assert_eq!(
        h.keystrokes(&elsewhere),
        0,
        "the session outside the task was left to its own conversation"
    );
}
